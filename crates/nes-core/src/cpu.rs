//! CPU module - 6502 processor emulation
//!
//! The Ricoh 2A03 (NTSC) / 2A07 (PAL) is a modified 6502 processor
//! used in the NES. It lacks decimal mode but includes an APU.

use crate::Bus;

pub mod addressing;
pub mod instructions;

pub use addressing::AddressingResult;
use instructions::InstructionHandler;

/// A read the CPU leaves for the machine to place, on the dot the chip
/// performs it.
///
/// The PPU's registers, `$2000-$3FFF`, are what the window is for: a `$2002`
/// read clears the VBlank flag and takes the frame's NMI with it, and a `$2007`
/// read moves the VRAM address, so *when* in its cycle the read happens is
/// observable. The APU's `$4015` is deliberately not in the window yet: blargg's
/// `apu_test` synchronises through it, and the frame-counter phase it measures
/// is a cycle or two out anyway (§4.3 of the cycle plan), so moving the read
/// now would only move that error around.
///
/// Written and read inside a single instruction, so it stays out of save
/// states: the machine runs it before [`Cpu::execute_instruction`] returns.
#[derive(Debug)]
pub(crate) struct PendingAccess {
    /// The handler that performs the access, held back until its cycle.
    handler: InstructionHandler,
    /// Effective address the addressing mode resolved.
    addr: u16,
    /// The page-crossing penalty the addressing mode reported.
    extra_cycles: u8,
    /// 1-based cycle of the instruction the handler runs on.
    pub(crate) at_cycle: u8,
    /// The cycle count the plan expects the handler to report.
    ///
    /// Read handlers return the addressing mode's page-crossing penalty and
    /// the rest return zero; the plan's cycle arithmetic depends on which, so
    /// a mismatch means the access is on the wrong cycle.
    expected_handler_cycles: u8,
}

/// Which interrupt the CPU is in the middle of taking.
///
/// The sequence itself is the same seven cycles for both; the source only
/// decides which vector the last two cycles fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum InterruptSource {
    /// The maskable IRQ line, through the vector at $FFFE.
    #[default]
    Irq,
    /// The non-maskable NMI, through the vector at $FFFA.
    Nmi,
}

impl crate::state::StateField for InterruptSource {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u8(match self {
            InterruptSource::Irq => 0,
            InterruptSource::Nmi => 1,
        });
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        *self = match r.u8() {
            1 => InterruptSource::Nmi,
            _ => InterruptSource::Irq,
        };
    }
}

/// 6502 CPU state
#[derive(Debug)]
pub struct Cpu {
    /// Accumulator register
    a: u8,
    /// X index register
    x: u8,
    /// Y index register
    y: u8,
    /// Stack pointer (points to $0100-$01FF)
    sp: u8,
    /// Program counter
    pc: u16,
    /// Processor status register
    status: StatusFlags,
    /// Total cycles executed
    cycles: u64,
    /// Set when a KIL/JAM opcode stops the CPU.
    ///
    /// The 2A03 parks here until an interrupt (or a reset) arrives; the rest of
    /// the machine — PPU, APU, mappers — keeps running while it hangs.
    halted: bool,
    /// Address an indexed addressing mode started from, before the index was
    /// added.
    ///
    /// The SHx stores (SHX/SHY/SHA/SHS) corrupt their target when the index
    /// addition carries out of the low byte, and the corruption depends on the
    /// page the *base* address was in, which the handler cannot recover from
    /// the effective address alone. It is written and read inside a single
    /// instruction, so it stays out of save states.
    addr_base: u16,
    /// Whether an IRQ is inhibited right now, as the interrupt decision sees it.
    ///
    /// Normally this is just the I flag, but CLI, SEI and PLP only change what
    /// interrupts are allowed *after* the next instruction has run, while RTI
    /// takes effect immediately. Keeping the decision its own copy is what lets
    /// the flag itself update at once — `CLI` followed by `PHP` still shows I
    /// clear in the pushed status — without the IRQ handler being entered an
    /// instruction too early.
    irq_inhibit: bool,
    /// Set by a taken branch that does not cross a page, to skip the interrupt
    /// poll the next instruction would otherwise make.
    ///
    /// The hardware polls interrupts on the penultimate cycle of an instruction,
    /// and a branch's last cycle is not one: the instruction after the branch
    /// gets to run first.
    skip_interrupt_poll: bool,
    /// The interrupt sequence being taken, if any, and how many of its seven
    /// cycles have run.
    interrupt_source: Option<InterruptSource>,
    interrupt_step: u8,
    /// Set when an NMI asserted during a sequence takes its vector over.
    ///
    /// The pushes the sequence already made stay what they were - an IRQ's
    /// status has B clear - and only the address the last two cycles fetch
    /// changes. Written and read inside one sequence, so it stays out of save
    /// states.
    nmi_hijack: bool,
    /// Set by BRK while the vector it read is still provisional.
    ///
    /// BRK reads the IRQ vector as part of the instruction, but that is not
    /// settled until the instruction's last cycles: an NMI asserted before then
    /// takes the vector over, with the frame BRK pushed (B set) left as it is.
    /// Cleared at the start of every instruction, so it never outlives the one
    /// that set it.
    brk_vector_pending: bool,
    /// Set while the machine is driving the cycles of one instruction.
    ///
    /// With it set, a read of `$2000-$401F` is left to the machine rather than
    /// performed where the instruction asks for it: the machine runs the
    /// handler on the cycle the chip performs the access on. A bus used on its
    /// own - the CPU's unit tests - leaves it clear and gets the whole
    /// instruction executed in one piece.
    defer_register_reads: bool,
    /// The access the instruction has left for the machine, if any.
    pending_access: Option<PendingAccess>,
    /// Low half of the vector, fetched on the sequence's sixth cycle and used
    /// on the seventh.
    vector_low: u8,
}

bitflags::bitflags! {
    /// 6502 status register flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StatusFlags: u8 {
        /// Carry flag
        const C = 0b0000_0001;
        /// Zero flag
        const Z = 0b0000_0010;
        /// Interrupt disable flag
        const I = 0b0000_0100;
        /// Decimal mode flag (not functional on NES)
        const D = 0b0000_1000;
        /// Break flag
        const B = 0b0001_0000;
        /// Unused flag (always set)
        const U = 0b0010_0000;
        /// Overflow flag
        const V = 0b0100_0000;
        /// Negative flag
        const N = 0b1000_0000;
    }
}

impl crate::state::StateField for StatusFlags {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u8(self.bits());
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        *self = StatusFlags::from_bits_truncate(r.u8());
    }
}

crate::state::state_fields!(Cpu {
    a,
    x,
    y,
    sp,
    pc,
    status,
    cycles,
    halted,
    irq_inhibit,
    skip_interrupt_poll,
    interrupt_source,
    interrupt_step,
    vector_low
});

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    /// Create a new CPU instance
    ///
    /// Initializes all registers to 0 except SP which is set to 0xFD
    /// (the standard value after reset). RAM is zeroed.
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0,
            status: StatusFlags::from_bits_truncate(0x24),
            cycles: 0,
            halted: false,
            addr_base: 0,
            irq_inhibit: true,
            skip_interrupt_poll: false,
            interrupt_source: None,
            interrupt_step: 0,
            nmi_hijack: false,
            brk_vector_pending: false,
            defer_register_reads: false,
            pending_access: None,
            vector_low: 0,
        }
    }

    /// Reset the CPU
    pub fn reset(&mut self, bus: &mut dyn Bus) {
        self.sp = self.sp.wrapping_sub(3);
        self.status.insert(StatusFlags::I);

        // Read reset vector
        let lo = bus.read(0xFFFC) as u16;
        let hi = bus.read(0xFFFD) as u16;
        self.pc = (hi << 8) | lo;

        self.halted = false;
        self.irq_inhibit = true;
        self.skip_interrupt_poll = false;
        self.cycles = 0;
    }

    /// Get the accumulator value
    pub fn a(&self) -> u8 {
        self.a
    }

    /// Get the X register value
    pub fn x(&self) -> u8 {
        self.x
    }

    /// Get the Y register value
    pub fn y(&self) -> u8 {
        self.y
    }

    /// Get the program counter
    pub fn pc(&self) -> u16 {
        self.pc
    }

    /// Get the stack pointer
    pub fn sp(&self) -> u8 {
        self.sp
    }

    /// Get the status flags
    pub fn status(&self) -> StatusFlags {
        self.status
    }

    /// Get total cycles executed
    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Whether a KIL/JAM opcode has stopped the CPU.
    ///
    /// The machine keeps running — the PPU, APU and mappers keep ticking — but
    /// no instruction is fetched until an interrupt or a reset arrives.
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Set the program counter (for testing purposes)
    ///
    /// This is used by test harnesses like nestest to set a specific
    /// starting address for automation mode.
    ///
    /// # Arguments
    /// * `pc` - The program counter value to set
    pub fn set_pc(&mut self, pc: u16) {
        self.pc = pc;
    }

    /// Tell the CPU the machine is driving this instruction's cycles.
    ///
    /// See [`Cpu::defer_register_reads`]: the machine places a read of the
    /// PPU/APU registers on the cycle the chip performs it, so it has to be the
    /// one that runs the handler.
    pub(crate) fn set_defer_register_reads(&mut self, defer: bool) {
        self.defer_register_reads = defer;
    }

    /// The cycle of the instruction being run on which its handler has to run,
    /// if it left one for the machine.
    pub(crate) fn pending_access_cycle(&self) -> Option<u8> {
        self.pending_access.as_ref().map(|access| access.at_cycle)
    }

    /// Run the handler the instruction held back, on the cycle its access
    /// belongs to.
    ///
    /// The machine calls this once, on the cycle
    /// [`Cpu::pending_access_cycle`] names; any register write the handler
    /// makes is parked by the bus as usual, which is how a read-modify-write's
    /// two writes still land on the two cycles after the read.
    pub(crate) fn perform_pending_access(&mut self, bus: &mut dyn Bus) {
        let pending = self
            .pending_access
            .take()
            .expect("the machine only runs an access it was told about");
        let handler_cycles = (pending.handler)(self, bus, pending.addr, pending.extra_cycles);
        debug_assert_eq!(
            handler_cycles, pending.expected_handler_cycles,
            "the handler's cycle count has to match the cycle the access was placed on",
        );
        let _ = handler_cycles;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBus;

    impl Bus for MockBus {
        fn read(&mut self, _addr: u16) -> u8 {
            0
        }

        fn write(&mut self, _addr: u16, _value: u8) {}

        fn on_cpu_cycle(&mut self, _cycles: u8) {}

        fn nmi_pending(&self) -> bool {
            false
        }

        fn clear_nmi(&mut self) {}

        fn irq_pending(&self) -> bool {
            false
        }
    }

    #[test]
    fn test_new() {
        let cpu = Cpu::new();
        assert_eq!(cpu.a, 0);
        assert_eq!(cpu.x, 0);
        assert_eq!(cpu.y, 0);
        assert_eq!(cpu.sp, 0xFD);
        assert_eq!(cpu.pc, 0);
        assert_eq!(cpu.cycles, 0);
    }

    #[test]
    fn test_cpu_reset() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus;
        cpu.reset(&mut bus);
        assert!(cpu.status.contains(StatusFlags::I));
    }
}
