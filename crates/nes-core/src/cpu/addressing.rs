//! CPU Addressing Modes - 6502 processor addressing mode implementations
//!
//! The 6502 processor has 13 addressing modes that determine how operands
//! are fetched for instructions. This module implements all modes with
//! hardware-accurate cycle counting and dummy read behavior.
//!
//! Reference: https://www.nesdev.org/wiki/CPU_addressing_modes

use super::Cpu;
use crate::Bus;

/// Result of an addressing mode calculation
///
/// Contains the effective address, additional cycles due to page crossing,
/// and whether a page boundary was crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressingResult {
    /// The effective memory address (or operand value for immediate mode)
    pub address: u16,
    /// Additional cycles due to page crossing (0 or 1)
    pub extra_cycles: u8,
    /// Whether a page boundary was crossed
    pub page_crossed: bool,
}

impl AddressingResult {
    /// Create a new addressing result
    fn new(address: u16, extra_cycles: u8, page_crossed: bool) -> Self {
        Self {
            address,
            extra_cycles,
            page_crossed,
        }
    }
}

/// Addressing mode implementations for the 6502 CPU
///
/// Each method calculates the effective address for an instruction based on
/// the current CPU state and the operand bytes following the opcode.
///
/// The 6502 has 13 addressing modes:
/// 1. Immediate - value is next byte
/// 2. Zero Page - $00 + offset
/// 3. Zero Page X - $00 + offset + X
/// 4. Zero Page Y - $00 + offset + Y
/// 5. Absolute - full 16-bit address
/// 6. Absolute X - 16-bit address + X
/// 7. Absolute Y - 16-bit address + Y
/// 8. Indirect - pointer at 16-bit address (JMP only)
/// 9. Indirect X - pointer at ZP + X
/// 10. Indirect Y - pointer at ZP, then + Y
/// 11. Relative - branch offset (signed)
/// 12. Accumulator - operation on A register
/// 13. Implied - no operand
impl Cpu {
    /// Immediate addressing mode
    ///
    /// The operand is the byte immediately following the opcode.
    /// Used for instructions like LDA #$44 (load immediate value).
    ///
    /// # Arguments
    /// * `pc` - Current program counter (points to operand byte)
    ///
    /// # Returns
    /// AddressingResult with the PC as the address (operand location)
    pub fn addr_immediate(&self, pc: u16) -> AddressingResult {
        // The operand is at PC, no extra cycles
        AddressingResult::new(pc, 0, false)
    }

    /// Zero Page addressing mode
    ///
    /// The operand is an 8-bit zero-page address ($00-$FF).
    /// Used for instructions like LDA $44.
    ///
    /// # Arguments
    /// * `operand` - The zero-page offset byte
    ///
    /// # Returns
    /// AddressingResult with the zero-page address
    pub fn addr_zero_page(&self, operand: u8) -> AddressingResult {
        let addr = operand as u16;
        AddressingResult::new(addr, 0, false)
    }

    /// Zero Page X addressing mode
    ///
    /// The operand is an 8-bit zero-page address plus the X register.
    /// Wraps within zero page (no page crossing possible).
    /// Used for instructions like LDA $44,X.
    ///
    /// # Arguments
    /// * `bus` - Memory bus for dummy read
    /// * `operand` - The zero-page offset byte
    ///
    /// # Returns
    /// AddressingResult with the indexed zero-page address
    pub fn addr_zero_page_x(&mut self, bus: &mut dyn Bus, operand: u8) -> AddressingResult {
        // The 6502 reads from the unindexed zero-page address while adding X.
        // This "dummy read" is a real bus cycle that can trigger I/O side effects.
        // See https://www.nesdev.org/wiki/CPU_addressing_modes
        bus.read(operand as u16);
        
        let addr = operand.wrapping_add(self.x) as u16;
        AddressingResult::new(addr, 0, false)
    }

    /// Zero Page Y addressing mode
    ///
    /// The operand is an 8-bit zero-page address plus the Y register.
    /// Wraps within zero page (no page crossing possible).
    /// Used for instructions like LDX $44,Y.
    ///
    /// # Arguments
    /// * `bus` - Memory bus for dummy read
    /// * `operand` - The zero-page offset byte
    ///
    /// # Returns
    /// AddressingResult with the indexed zero-page address
    pub fn addr_zero_page_y(&mut self, bus: &mut dyn Bus, operand: u8) -> AddressingResult {
        // Dummy read from unindexed zero-page address
        bus.read(operand as u16);
        
        let addr = operand.wrapping_add(self.y) as u16;
        AddressingResult::new(addr, 0, false)
    }

    /// Absolute addressing mode
    ///
    /// The operand is a full 16-bit address (low byte first).
    /// Used for instructions like LDA $4400.
    ///
    /// # Arguments
    /// * `operand_lo` - Low byte of the address
    /// * `operand_hi` - High byte of the address
    ///
    /// # Returns
    /// AddressingResult with the 16-bit address
    pub fn addr_absolute(&self, operand_lo: u8, operand_hi: u8) -> AddressingResult {
        let addr = ((operand_hi as u16) << 8) | (operand_lo as u16);
        AddressingResult::new(addr, 0, false)
    }

    /// Absolute X addressing mode
    ///
    /// The operand is a full 16-bit address plus the X register.
    /// May cross page boundaries, adding an extra cycle.
    /// Used for instructions like LDA $4400,X.
    ///
    /// # Arguments
    /// * `bus` - Memory bus for dummy read on page cross
    /// * `operand_lo` - Low byte of the base address
    /// * `operand_hi` - High byte of the base address
    ///
    /// # Returns
    /// AddressingResult with the indexed address and page cross info
    pub fn addr_absolute_x(&mut self, bus: &mut dyn Bus, operand_lo: u8, operand_hi: u8) -> AddressingResult {
        let base_addr = ((operand_hi as u16) << 8) | (operand_lo as u16);
        let indexed_addr = base_addr.wrapping_add(self.x as u16);
        
        // Check for page crossing
        let page_crossed = (base_addr & 0xFF00) != (indexed_addr & 0xFF00);
        
        if page_crossed {
            // Page boundary crossed: the 6502 first reads from the "wrong"
            // address (correct low byte, uncorrected high byte) before reading
            // the correct one. This dummy read is a real bus cycle.
            // See https://www.nesdev.org/wiki/CPU_addressing_modes
            let dummy_addr = (base_addr & 0xFF00) | (indexed_addr & 0x00FF);
            bus.read(dummy_addr);
            AddressingResult::new(indexed_addr, 1, true)
        } else {
            AddressingResult::new(indexed_addr, 0, false)
        }
    }

    /// Absolute Y addressing mode
    ///
    /// The operand is a full 16-bit address plus the Y register.
    /// May cross page boundaries, adding an extra cycle.
    /// Used for instructions like LDA $4400,Y.
    ///
    /// # Arguments
    /// * `bus` - Memory bus for dummy read on page cross
    /// * `operand_lo` - Low byte of the base address
    /// * `operand_hi` - High byte of the base address
    ///
    /// # Returns
    /// AddressingResult with the indexed address and page cross info
    pub fn addr_absolute_y(&mut self, bus: &mut dyn Bus, operand_lo: u8, operand_hi: u8) -> AddressingResult {
        let base_addr = ((operand_hi as u16) << 8) | (operand_lo as u16);
        let indexed_addr = base_addr.wrapping_add(self.y as u16);
        
        // Check for page crossing
        let page_crossed = (base_addr & 0xFF00) != (indexed_addr & 0xFF00);
        
        if page_crossed {
            // Same dummy read behavior as Absolute X
            let dummy_addr = (base_addr & 0xFF00) | (indexed_addr & 0x00FF);
            bus.read(dummy_addr);
            AddressingResult::new(indexed_addr, 1, true)
        } else {
            AddressingResult::new(indexed_addr, 0, false)
        }
    }

    /// Indirect addressing mode (JMP only)
    ///
    /// The operand is a 16-bit address containing a pointer to the
    /// actual target address. The 6502 has a famous bug: when the
    /// pointer's low byte is $FF, the high byte wraps within the
    /// same page instead of crossing to the next page.
    /// Used only for JMP ($4400).
    ///
    /// # Arguments
    /// * `bus` - Memory bus for reading the pointer
    /// * `ptr_lo` - Low byte of the pointer address
    /// * `ptr_hi` - High byte of the pointer address
    ///
    /// # Returns
    /// AddressingResult with the target address
    pub fn addr_indirect(&mut self, bus: &mut dyn Bus, ptr_lo: u8, ptr_hi: u8) -> AddressingResult {
        let ptr_addr = ((ptr_hi as u16) << 8) | (ptr_lo as u16);
        
        // Read low byte of target address
        let target_lo = bus.read(ptr_addr);
        
        // The 6502 bug: when pointer low byte is $FF, high byte wraps
        // within the same page instead of crossing to next page
        let ptr_hi_addr = (ptr_addr & 0xFF00) | ((ptr_addr.wrapping_add(1)) & 0x00FF);
        let target_hi = bus.read(ptr_hi_addr);
        
        let target_addr = ((target_hi as u16) << 8) | (target_lo as u16);
        AddressingResult::new(target_addr, 0, false)
    }

    /// Indirect X addressing mode (Pre-indexed)
    ///
    /// The operand is a zero-page address. X is added to this to get
    /// the pointer address (wrapping in zero page). The 16-bit value
    /// at this pointer is the effective address.
    /// Used for instructions like LDA ($44,X).
    ///
    /// # Arguments
    /// * `bus` - Memory bus for reading the pointer
    /// * `operand` - The zero-page pointer base
    ///
    /// # Returns
    /// AddressingResult with the effective address
    pub fn addr_indirect_x(&mut self, bus: &mut dyn Bus, operand: u8) -> AddressingResult {
        // Dummy read from the unindexed pointer address while adding X
        bus.read(operand as u16);
        
        // Add X to get the zero-page pointer address (wraps in zero page)
        let zp_ptr = operand.wrapping_add(self.x);
        
        // Read the 16-bit effective address from zero page
        let addr_lo = bus.read(zp_ptr as u16);
        let addr_hi = bus.read(zp_ptr.wrapping_add(1) as u16);
        
        let addr = ((addr_hi as u16) << 8) | (addr_lo as u16);
        AddressingResult::new(addr, 0, false)
    }

    /// Indirect Y addressing mode (Post-indexed)
    ///
    /// The operand is a zero-page address containing a 16-bit pointer.
    /// Y is added to this pointer to get the effective address.
    /// May cross page boundaries, adding an extra cycle.
    /// Used for instructions like LDA ($44),Y.
    ///
    /// # Arguments
    /// * `bus` - Memory bus for reading the pointer
    /// * `operand` - The zero-page pointer address
    ///
    /// # Returns
    /// AddressingResult with the effective address and page cross info
    pub fn addr_indirect_y(&mut self, bus: &mut dyn Bus, operand: u8) -> AddressingResult {
        // Read the 16-bit base address from zero page
        let base_lo = bus.read(operand as u16);
        let base_hi = bus.read(operand.wrapping_add(1) as u16);
        let base_addr = ((base_hi as u16) << 8) | (base_lo as u16);
        
        // Add Y to get the effective address
        let effective_addr = base_addr.wrapping_add(self.y as u16);
        
        // Check for page crossing
        let page_crossed = (base_addr & 0xFF00) != (effective_addr & 0xFF00);
        
        if page_crossed {
            // Page boundary crossed: dummy read from wrong address
            let dummy_addr = (base_addr & 0xFF00) | (effective_addr & 0x00FF);
            bus.read(dummy_addr);
            AddressingResult::new(effective_addr, 1, true)
        } else {
            AddressingResult::new(effective_addr, 0, false)
        }
    }

    /// Relative addressing mode
    ///
    /// The operand is a signed 8-bit offset (-128 to +127).
    /// The offset is added to the program counter (PC) to get
    /// the branch target address.
    /// Used for branch instructions like BNE, BEQ, etc.
    ///
    /// # Arguments
    /// * `offset` - The signed offset byte
    /// * `pc` - Current program counter (points to next instruction)
    ///
    /// # Returns
    /// AddressingResult with the target address
    pub fn addr_relative(&self, offset: i8, pc: u16) -> AddressingResult {
        // Sign-extend the offset and add to PC
        let target = pc.wrapping_add(offset as u16);
        AddressingResult::new(target, 0, false)
    }

    /// Accumulator addressing mode
    ///
    /// The operation is performed on the accumulator register.
    /// The 6502 still performs a dummy read of the next byte.
    /// Used for instructions like ASL A, ROL A, etc.
    ///
    /// # Arguments
    /// * `bus` - Memory bus for dummy read
    /// * `pc` - Current program counter (for dummy read)
    ///
    /// # Returns
    /// AddressingResult with accumulator as the "address" (special case)
    pub fn addr_accumulator(&mut self, bus: &mut dyn Bus, pc: u16) -> AddressingResult {
        // The 6502 performs a dummy read of the byte at PC during its second cycle
        bus.read(pc);
        
        // Return a special value indicating accumulator mode
        // The actual address doesn't matter - the instruction will use self.a
        AddressingResult::new(0xFFFF, 0, false)
    }

    /// Implied addressing mode
    ///
    /// The instruction has no operand. The 6502 still performs
    /// a dummy read of the next byte (the next opcode).
    /// Used for instructions like NOP, CLC, SEC, etc.
    ///
    /// # Arguments
    /// * `bus` - Memory bus for dummy read
    /// * `pc` - Current program counter (for dummy read)
    ///
    /// # Returns
    /// AddressingResult with no meaningful address
    pub fn addr_implied(&mut self, bus: &mut dyn Bus, pc: u16) -> AddressingResult {
        // The 6502's second cycle performs a dummy read of the byte at PC
        // (the next opcode). This is a real bus operation.
        // See https://www.nesdev.org/wiki/CPU_addressing_modes
        bus.read(pc);
        
        AddressingResult::new(0, 0, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bus;

    /// Mock bus for testing
    struct MockBus {
        memory: [u8; 65536],
        reads: Vec<u16>,
    }

    impl MockBus {
        fn new() -> Self {
            Self {
                memory: [0; 65536],
                reads: Vec::new(),
            }
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.memory[addr as usize] = value;
        }

        fn get_reads(&self) -> &[u16] {
            &self.reads
        }
    }

    impl Bus for MockBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.reads.push(addr);
            self.memory[addr as usize]
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
    fn test_immediate() {
        let cpu = Cpu::new();
        let result = cpu.addr_immediate(0x1234);
        assert_eq!(result.address, 0x1234);
        assert_eq!(result.extra_cycles, 0);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_zero_page() {
        let cpu = Cpu::new();
        let result = cpu.addr_zero_page(0x44);
        assert_eq!(result.address, 0x0044);
        assert_eq!(result.extra_cycles, 0);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_zero_page_x() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.x = 0x10;
        let result = cpu.addr_zero_page_x(&mut bus, 0x44);
        
        assert_eq!(result.address, 0x0054);
        assert_eq!(result.extra_cycles, 0);
        assert!(!result.page_crossed);
        assert_eq!(bus.get_reads(), &[0x44]); // Dummy read
    }

    #[test]
    fn test_zero_page_x_wrap() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.x = 0xFF;
        let result = cpu.addr_zero_page_x(&mut bus, 0x80);
        
        // 0x80 + 0xFF = 0x17F, wraps to 0x7F in zero page
        assert_eq!(result.address, 0x007F);
        assert_eq!(result.extra_cycles, 0);
    }

    #[test]
    fn test_zero_page_y() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.y = 0x20;
        let result = cpu.addr_zero_page_y(&mut bus, 0x30);
        
        assert_eq!(result.address, 0x0050);
        assert_eq!(result.extra_cycles, 0);
        assert!(!result.page_crossed);
        assert_eq!(bus.get_reads(), &[0x30]); // Dummy read
    }

    #[test]
    fn test_absolute() {
        let cpu = Cpu::new();
        let result = cpu.addr_absolute(0x34, 0x12);
        assert_eq!(result.address, 0x1234);
        assert_eq!(result.extra_cycles, 0);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_absolute_x_no_cross() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.x = 0x10;
        let result = cpu.addr_absolute_x(&mut bus, 0x40, 0x12);
        
        assert_eq!(result.address, 0x1250);
        assert_eq!(result.extra_cycles, 0);
        assert!(!result.page_crossed);
        assert!(bus.get_reads().is_empty()); // No dummy read when no page cross
    }

    #[test]
    fn test_absolute_x_with_cross() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.x = 0xFF;
        let result = cpu.addr_absolute_x(&mut bus, 0x80, 0x12);
        
        // 0x1280 + 0xFF = 0x137F, crosses from page 0x12 to 0x13
        assert_eq!(result.address, 0x137F);
        assert_eq!(result.extra_cycles, 1);
        assert!(result.page_crossed);
        assert_eq!(bus.get_reads(), &[0x127F]); // Dummy read at wrong address
    }

    #[test]
    fn test_absolute_y_no_cross() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.y = 0x10;
        let result = cpu.addr_absolute_y(&mut bus, 0x40, 0x12);
        
        assert_eq!(result.address, 0x1250);
        assert_eq!(result.extra_cycles, 0);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_absolute_y_with_cross() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.y = 0xFF;
        let result = cpu.addr_absolute_y(&mut bus, 0x80, 0x12);
        
        assert_eq!(result.address, 0x137F);
        assert_eq!(result.extra_cycles, 1);
        assert!(result.page_crossed);
        assert_eq!(bus.get_reads(), &[0x127F]); // Dummy read
    }

    #[test]
    fn test_indirect_normal() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        // Set up pointer at 0x4400 pointing to 0x1234
        bus.write(0x4400, 0x34);
        bus.write(0x4401, 0x12);
        
        let result = cpu.addr_indirect(&mut bus, 0x00, 0x44);
        
        assert_eq!(result.address, 0x1234);
        assert_eq!(result.extra_cycles, 0);
        assert_eq!(bus.get_reads(), &[0x4400, 0x4401]);
    }

    #[test]
    fn test_indirect_bug() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        // Set up pointer at 0x44FF with the bug
        // Low byte at 0x44FF, high byte should be at 0x4500 but wraps to 0x4400
        bus.write(0x44FF, 0x34);
        bus.write(0x4400, 0x12); // This is where the bug reads the high byte
        
        let result = cpu.addr_indirect(&mut bus, 0xFF, 0x44);
        
        // Should read 0x1234 (with high byte from 0x4400, not 0x4500)
        assert_eq!(result.address, 0x1234);
        assert_eq!(bus.get_reads(), &[0x44FF, 0x4400]);
    }

    #[test]
    fn test_indirect_x() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.x = 0x02;
        
        // Set up pointer at 0x0046 (0x44 + 0x02) pointing to 0xABCD
        bus.write(0x0046, 0xCD);
        bus.write(0x0047, 0xAB);
        
        let result = cpu.addr_indirect_x(&mut bus, 0x44);
        
        assert_eq!(result.address, 0xABCD);
        assert_eq!(result.extra_cycles, 0);
        // Dummy read at 0x44, then reads at 0x46 and 0x47
        assert_eq!(bus.get_reads(), &[0x44, 0x46, 0x47]);
    }

    #[test]
    fn test_indirect_x_wrap() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.x = 0x05;
        
        // Pointer at 0x0004 (0xFF + 0x05 wraps in zero page)
        bus.write(0x0004, 0x78);
        bus.write(0x0005, 0x56);
        
        let result = cpu.addr_indirect_x(&mut bus, 0xFF);
        
        assert_eq!(result.address, 0x5678);
    }

    #[test]
    fn test_indirect_y_no_cross() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.y = 0x10;
        
        // Set up pointer at 0x0044 pointing to 0x1200
        bus.write(0x0044, 0x00);
        bus.write(0x0045, 0x12);
        
        let result = cpu.addr_indirect_y(&mut bus, 0x44);
        
        assert_eq!(result.address, 0x1210);
        assert_eq!(result.extra_cycles, 0);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_indirect_y_with_cross() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.y = 0xFF;
        
        // Set up pointer at 0x0044 pointing to 0x1280
        bus.write(0x0044, 0x80);
        bus.write(0x0045, 0x12);
        
        let result = cpu.addr_indirect_y(&mut bus, 0x44);
        
        // 0x1280 + 0xFF = 0x137F, crosses page boundary
        assert_eq!(result.address, 0x137F);
        assert_eq!(result.extra_cycles, 1);
        assert!(result.page_crossed);
        // Dummy read at wrong address
        assert_eq!(bus.get_reads(), &[0x44, 0x45, 0x127F]);
    }

    #[test]
    fn test_relative_positive() {
        let cpu = Cpu::new();
        let result = cpu.addr_relative(0x10, 0x0200);
        assert_eq!(result.address, 0x0210);
        assert_eq!(result.extra_cycles, 0);
    }

    #[test]
    fn test_relative_negative() {
        let cpu = Cpu::new();
        let result = cpu.addr_relative(-0x10, 0x0200);
        // -0x10 as i8 = 0xF0, sign-extended to 0xFFF0
        assert_eq!(result.address, 0x01F0);
        assert_eq!(result.extra_cycles, 0);
    }

    #[test]
    fn test_relative_zero() {
        let cpu = Cpu::new();
        let result = cpu.addr_relative(0, 0x0200);
        assert_eq!(result.address, 0x0200);
    }

    #[test]
    fn test_accumulator() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        cpu.a = 0x42;
        let result = cpu.addr_accumulator(&mut bus, 0x0200);
        
        // Returns special marker address
        assert_eq!(result.address, 0xFFFF);
        assert_eq!(result.extra_cycles, 0);
        assert_eq!(bus.get_reads(), &[0x0200]); // Dummy read
    }

    #[test]
    fn test_implied() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        let result = cpu.addr_implied(&mut bus, 0x0200);
        
        assert_eq!(result.address, 0);
        assert_eq!(result.extra_cycles, 0);
        assert_eq!(bus.get_reads(), &[0x0200]); // Dummy read
    }

    #[test]
    fn test_page_cross_boundary() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        // Test crossing from 0x12FF to 0x1300
        cpu.x = 0x01;
        let result = cpu.addr_absolute_x(&mut bus, 0xFF, 0x12);
        
        assert_eq!(result.address, 0x1300);
        assert_eq!(result.extra_cycles, 1);
        assert!(result.page_crossed);
    }

    #[test]
    fn test_no_page_cross_same_page() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        // Test within same page: 0x1200 to 0x12FF
        cpu.x = 0xFF;
        let result = cpu.addr_absolute_x(&mut bus, 0x00, 0x12);
        
        assert_eq!(result.address, 0x12FF);
        assert_eq!(result.extra_cycles, 0);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_absolute_x_wrap_16bit() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        
        // Test wrap at 0xFFFF
        cpu.x = 0x01;
        let result = cpu.addr_absolute_x(&mut bus, 0xFF, 0xFF);
        
        // 0xFFFF + 1 wraps to 0x0000
        assert_eq!(result.address, 0x0000);
        // Crosses from page 0xFF to 0x00
        assert_eq!(result.extra_cycles, 1);
        assert!(result.page_crossed);
    }
}
