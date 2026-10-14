//! CPU module - 6502 processor emulation
//!
//! The Ricoh 2A03 (NTSC) / 2A07 (PAL) is a modified 6502 processor
//! used in the NES. It lacks decimal mode but includes an APU.

use crate::Bus;

pub mod addressing;
pub mod instructions;

pub use addressing::AddressingResult;

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
    halted
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
