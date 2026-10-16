//! CPU module - 6502 processor emulation
//!
//! The Ricoh 2A03 (NTSC) / 2A07 (PAL) is a modified 6502 processor
//! used in the NES. It lacks decimal mode but includes an APU.

use crate::Bus;

pub mod addressing;
pub mod instructions;

pub use addressing::AddressingResult;
use instructions::AddressingMode;

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
    /// Whether a DMA interrupted the addressing's discarded read.
    ///
    /// The SHx stores skip the mask on the value they write when one did
    /// (Mesen's `hadDma`), and the discarded read happens in the addressing
    /// rather than in the handler, so the fact is carried across the two.
    /// Written and read inside a single instruction: out of save states too.
    dummy_read_had_dma: bool,
    /// An NMI has been asserted and not taken yet. Mesen's `_needNmi`.
    ///
    /// The line is sampled at the end of every CPU cycle and only a *rising*
    /// edge latches, so a line that comes and goes inside one cycle is never
    /// seen, and a line that is seen stays latched until the handler is
    /// entered.
    need_nmi: bool,
    /// [`Cpu::need_nmi`] as of the previous cycle's end.
    ///
    /// The post-instruction decision reads this one, which is what makes the
    /// sample from the instruction's *last* cycle too late to be taken — the
    /// "it's really the status of the interrupt lines at the end of the
    /// second-to-last cycle that matters" rule.
    prev_need_nmi: bool,
    /// The NMI line's level when it was last sampled, so the next cycle can
    /// tell an edge from a level. Mesen's `_prevNmiFlag`.
    prev_nmi_flag: bool,
    /// Whether an interrupt is asked for right now: the IRQ line asserted with
    /// I clear. Mesen's `_runIrq`.
    run_irq: bool,
    /// [`Cpu::run_irq`] as of the previous cycle's end, i.e. the value the
    /// post-instruction decision reads.
    prev_run_irq: bool,
    /// The addressing mode of the instruction being run.
    ///
    /// Mesen's `_instAddrMode`: a handler asks this when the same code serves
    /// both a memory operand and a register (the NOPs are what needs it).
    /// Written and read inside one instruction, so it stays out of save states.
    addr_mode: AddressingMode,
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
    need_nmi,
    prev_need_nmi,
    prev_nmi_flag,
    run_irq,
    prev_run_irq
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
            dummy_read_had_dma: false,
            addr_mode: AddressingMode::Implied,
            need_nmi: false,
            prev_need_nmi: false,
            prev_nmi_flag: false,
            run_irq: false,
            prev_run_irq: false,
        }
    }

    /// Reset the CPU
    /// Power-on reset: the registers take their documented power-on values —
    /// `A/X/Y = 0`, `SP = $FD`, `P = I` — and the stack pointer is *not* walked
    /// down. RAM is untouched either way (`NesCpu::Reset(false, ...)`).
    pub fn reset(&mut self, bus: &mut dyn Bus) {
        self.reset_internal(bus, false);
    }

    /// Soft reset, the reset button: the registers and the RAM stay as they
    /// are, the interrupt-disable flag goes up, and the stack pointer walks
    /// down three bytes with nothing written to the stack
    /// (`NesCpu::Reset(true, ...)`, which is what a console's reset line does).
    pub fn soft_reset(&mut self, bus: &mut dyn Bus) {
        self.reset_internal(bus, true);
    }

    fn reset_internal(&mut self, bus: &mut dyn Bus, soft: bool) {
        self.halted = false;
        self.need_nmi = false;
        self.prev_need_nmi = false;
        self.prev_nmi_flag = false;
        self.run_irq = false;
        self.prev_run_irq = false;

        // Read reset vector. The plain access, not a cycle: the two reads must
        // not clock the PPU or the APU (Mesen's comment on the same reads).
        let lo = bus.read(0xFFFC) as u16;
        let hi = bus.read(0xFFFD) as u16;
        self.pc = (hi << 8) | lo;

        if soft {
            self.status.insert(StatusFlags::I);
            self.sp = self.sp.wrapping_sub(3);
        } else {
            self.a = 0;
            self.x = 0;
            self.y = 0;
            self.sp = 0xFD;
            self.status = StatusFlags::I;
            self.cycles = 0;
        }
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

    /// Set the cycle count to the machine's.
    ///
    /// The machine counts every cycle it advances — the instruction's own, the
    /// ones an interrupt sequence takes, the ones DMA steals — so the CPU's
    /// counter follows it rather than keeping a tally of its own.
    pub(crate) fn set_cycle_count(&mut self, cycles: u64) {
        self.cycles = cycles;
    }

    /// Whether an NMI has been asserted and not taken yet.
    ///
    /// Mesen's `_needNmi`: a rising edge of the line latches it, and the
    /// handler's entry clears it. The machine does not consult this - the
    /// decision is the CPU's - so it is here for the tests that pin the latch's
    /// behaviour.
    #[cfg(test)]
    pub(crate) fn nmi_asserted(&self) -> bool {
        self.need_nmi
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

    /// Read one byte the way the chip does: this call *is* one CPU cycle, and
    /// the access sits inside it (Mesen's `MemoryRead`, which brackets the
    /// access with `StartCpuCycle`/`EndCpuCycle`).
    pub(crate) fn read(&mut self, bus: &mut dyn Bus, addr: u16) -> u8 {
        let value = bus.read_cycle(addr);
        self.end_cycle(bus);
        value
    }

    /// Write one byte the way the chip does: one CPU cycle with the write
    /// strobed inside it (Mesen's `MemoryWrite`).
    pub(crate) fn write(&mut self, bus: &mut dyn Bus, addr: u16, value: u8) {
        bus.write_cycle(addr, value);
        self.end_cycle(bus);
    }

    /// The discarded read the 6502 spends its internal cycles on.
    ///
    /// It reads the byte the program counter points at and throws it away —
    /// the access is real, so the cartridge and the open-bus latch see it, but
    /// the PC does not move.
    pub(crate) fn dummy_read(&mut self, bus: &mut dyn Bus) {
        self.read(bus, self.pc);
    }

    /// Read the byte at the program counter and advance it: an operand fetch.
    pub(crate) fn read_byte(&mut self, bus: &mut dyn Bus) -> u8 {
        let value = self.read(bus, self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    /// Two operand fetches, little endian first.
    pub(crate) fn read_word(&mut self, bus: &mut dyn Bus) -> u16 {
        let low = self.read_byte(bus) as u16;
        let high = self.read_byte(bus) as u16;
        (high << 8) | low
    }

    /// The bookkeeping the chip does at the end of every CPU cycle.
    ///
    /// Mesen's `EndCpuCycle`: the NMI line is sampled for an edge, and the IRQ
    /// decision is kept one cycle behind so that the sample from an
    /// instruction's last cycle is never the one it is judged by.
    /// Whether the instruction being run has a memory operand to read.
    ///
    /// The accumulator and implied modes do not: their "operand" is a register
    /// or nothing at all, and the byte the chip reads on their extra cycles is
    /// discarded.
    pub(crate) fn operand_is_memory(&self) -> bool {
        !matches!(
            self.addr_mode,
            AddressingMode::Implied | AddressingMode::Accumulator
        )
    }

    pub(crate) fn end_cycle(&mut self, bus: &mut dyn Bus) {
        self.prev_need_nmi = self.need_nmi;

        let nmi_flag = bus.nmi_line();
        if !self.prev_nmi_flag && nmi_flag {
            self.need_nmi = true;
        }
        self.prev_nmi_flag = nmi_flag;

        self.prev_run_irq = self.run_irq;
        self.run_irq = bus.irq_pending() && !self.status.contains(StatusFlags::I);
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

        fn nmi_line(&self) -> bool {
            false
        }

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
