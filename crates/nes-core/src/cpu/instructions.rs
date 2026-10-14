//! CPU Instructions - 6502 processor instruction implementations
//!
//! This module implements all 56 official 6502 opcodes with proper
//! flag handling, cycle counting, and addressing mode integration.
//!
//! Reference: https://www.nesdev.org/wiki/CPU_instructions

use super::{Cpu, StatusFlags};
use crate::Bus;

/// Opcode table entry - defines instruction metadata
#[derive(Debug, Clone, Copy)]
pub struct Opcode {
    /// Instruction mnemonic
    pub mnemonic: &'static str,
    /// Addressing mode function
    pub addr_mode: AddressingMode,
    /// Base cycle count
    pub base_cycles: u8,
    /// Instruction handler
    pub handler: InstructionHandler,
}

/// Addressing mode types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressingMode {
    /// Immediate - value is next byte
    Immediate,
    /// Zero Page - $00 + offset
    ZeroPage,
    /// Zero Page X - $00 + offset + X
    ZeroPageX,
    /// Zero Page Y - $00 + offset + Y
    ZeroPageY,
    /// Absolute - full 16-bit address
    Absolute,
    /// Absolute X - 16-bit address + X
    AbsoluteX,
    /// Absolute Y - 16-bit address + Y
    AbsoluteY,
    /// Indirect - pointer at 16-bit address (JMP only)
    Indirect,
    /// Indirect X - pointer at ZP + X
    IndirectX,
    /// Indirect Y - pointer at ZP, then + Y
    IndirectY,
    /// Relative - branch offset (signed)
    Relative,
    /// Accumulator - operation on A register
    Accumulator,
    /// Implied - no operand
    Implied,
}

/// Instruction handler function type
pub type InstructionHandler = fn(&mut Cpu, &mut dyn Bus, u16, u8) -> u8;

impl Cpu {
    /// Execute a single instruction
    ///
    /// This is the main instruction execution entry point. It fetches the opcode,
    /// decodes the addressing mode, and dispatches to the appropriate handler.
    ///
    /// # Arguments
    /// * `bus` - Memory bus for read/write operations
    /// * `opcode` - The opcode byte to execute
    ///
    /// # Returns
    /// Total cycles consumed by the instruction
    pub fn execute_instruction(&mut self, bus: &mut dyn Bus, opcode: u8) -> u8 {
        // Interrupts are taken between instructions.
        //
        // NMI is edge-triggered and cannot be masked. IRQ is level-triggered
        // and is ignored while the I flag is set.
        //
        // Taking one is also what restarts a halted CPU, which is why the halt
        // check sits below rather than above these.
        if bus.nmi_pending() {
            bus.clear_nmi();
            self.halted = false;
            return self.service_interrupt(bus, 0xFFFA);
        }
        if bus.irq_pending() && !self.status.contains(StatusFlags::I) {
            self.halted = false;
            return self.service_interrupt(bus, 0xFFFE);
        }

        // A KIL/JAM opcode stops the CPU: it keeps burning cycles but fetches
        // nothing until one of the interrupt paths above fires.
        if self.halted {
            return 2;
        }

        let op = &OPCODE_TABLE[opcode as usize];

        // Fetch operands based on addressing mode
        let (addr, extra_cycles) = match op.addr_mode {
            AddressingMode::Immediate => {
                let addr = self.pc.wrapping_add(1);
                self.pc = self.pc.wrapping_add(2);
                (addr, 0)
            }
            AddressingMode::ZeroPage => {
                let operand = bus.read(self.pc.wrapping_add(1));
                self.pc = self.pc.wrapping_add(2);
                (operand as u16, 0)
            }
            AddressingMode::ZeroPageX => {
                let operand = bus.read(self.pc.wrapping_add(1));
                self.pc = self.pc.wrapping_add(2);
                // Dummy read at unindexed address
                bus.read(operand as u16);
                let addr = operand.wrapping_add(self.x) as u16;
                (addr, 0)
            }
            AddressingMode::ZeroPageY => {
                let operand = bus.read(self.pc.wrapping_add(1));
                self.pc = self.pc.wrapping_add(2);
                // Dummy read at unindexed address
                bus.read(operand as u16);
                let addr = operand.wrapping_add(self.y) as u16;
                (addr, 0)
            }
            AddressingMode::Absolute => {
                let lo = bus.read(self.pc.wrapping_add(1));
                let hi = bus.read(self.pc.wrapping_add(2));
                self.pc = self.pc.wrapping_add(3);
                let addr = ((hi as u16) << 8) | (lo as u16);
                (addr, 0)
            }
            AddressingMode::AbsoluteX => {
                let lo = bus.read(self.pc.wrapping_add(1));
                let hi = bus.read(self.pc.wrapping_add(2));
                self.pc = self.pc.wrapping_add(3);
                let base_addr = ((hi as u16) << 8) | (lo as u16);
                let addr = base_addr.wrapping_add(self.x as u16);
                let page_crossed = (base_addr & 0xFF00) != (addr & 0xFF00);
                if page_crossed {
                    // Dummy read at wrong address
                    let dummy_addr = (base_addr & 0xFF00) | (addr & 0x00FF);
                    bus.read(dummy_addr);
                    (addr, 1)
                } else {
                    (addr, 0)
                }
            }
            AddressingMode::AbsoluteY => {
                let lo = bus.read(self.pc.wrapping_add(1));
                let hi = bus.read(self.pc.wrapping_add(2));
                self.pc = self.pc.wrapping_add(3);
                let base_addr = ((hi as u16) << 8) | (lo as u16);
                let addr = base_addr.wrapping_add(self.y as u16);
                let page_crossed = (base_addr & 0xFF00) != (addr & 0xFF00);
                if page_crossed {
                    // Dummy read at wrong address
                    let dummy_addr = (base_addr & 0xFF00) | (addr & 0x00FF);
                    bus.read(dummy_addr);
                    (addr, 1)
                } else {
                    (addr, 0)
                }
            }
            AddressingMode::Indirect => {
                // JMP indirect - has the famous 6502 bug
                let ptr_lo = bus.read(self.pc.wrapping_add(1));
                let ptr_hi = bus.read(self.pc.wrapping_add(2));
                self.pc = self.pc.wrapping_add(3);
                let ptr_addr = ((ptr_hi as u16) << 8) | (ptr_lo as u16);
                let target_lo = bus.read(ptr_addr);
                // Bug: when pointer low byte is $FF, high byte wraps within page
                let ptr_hi_addr = (ptr_addr & 0xFF00) | ((ptr_addr.wrapping_add(1)) & 0x00FF);
                let target_hi = bus.read(ptr_hi_addr);
                let addr = ((target_hi as u16) << 8) | (target_lo as u16);
                (addr, 0)
            }
            AddressingMode::IndirectX => {
                let operand = bus.read(self.pc.wrapping_add(1));
                self.pc = self.pc.wrapping_add(2);
                // Dummy read at unindexed address
                bus.read(operand as u16);
                let zp_ptr = operand.wrapping_add(self.x);
                let addr_lo = bus.read(zp_ptr as u16);
                let addr_hi = bus.read(zp_ptr.wrapping_add(1) as u16);
                let addr = ((addr_hi as u16) << 8) | (addr_lo as u16);
                (addr, 0)
            }
            AddressingMode::IndirectY => {
                let operand = bus.read(self.pc.wrapping_add(1));
                self.pc = self.pc.wrapping_add(2);
                let base_lo = bus.read(operand as u16);
                let base_hi = bus.read(operand.wrapping_add(1) as u16);
                let base_addr = ((base_hi as u16) << 8) | (base_lo as u16);
                let addr = base_addr.wrapping_add(self.y as u16);
                let page_crossed = (base_addr & 0xFF00) != (addr & 0xFF00);
                if page_crossed {
                    // Dummy read at wrong address
                    let dummy_addr = (base_addr & 0xFF00) | (addr & 0x00FF);
                    bus.read(dummy_addr);
                    (addr, 1)
                } else {
                    (addr, 0)
                }
            }
            AddressingMode::Relative => {
                let offset = bus.read(self.pc.wrapping_add(1)) as i8;
                self.pc = self.pc.wrapping_add(2);
                let target = self.pc.wrapping_add(offset as u16);
                (target, 0)
            }
            AddressingMode::Accumulator => {
                // Dummy read of next byte
                bus.read(self.pc.wrapping_add(1));
                self.pc = self.pc.wrapping_add(1);
                // Use special marker for accumulator mode
                (0xFFFF, 0)
            }
            AddressingMode::Implied => {
                // Dummy read of next byte (the next opcode)
                bus.read(self.pc.wrapping_add(1));
                self.pc = self.pc.wrapping_add(1);
                (0, 0)
            }
        };

        // Execute the instruction handler
        let handler_cycles = (op.handler)(self, bus, addr, extra_cycles);

        // The addressing mode reports the page-crossing penalty as
        // `extra_cycles`, and it is the *handler* that decides whether this
        // opcode pays it: the read handlers return it, while stores and
        // read-modify-writes (which always take their full cycle count) return
        // zero. Adding it here as well charged every page-crossed indexed read
        // one cycle too many.
        let total_cycles = op.base_cycles + handler_cycles;
        self.cycles += total_cycles as u64;

        total_cycles
    }

    /// Update N (negative) and Z (zero) flags based on value
    fn update_nz_flags(&mut self, value: u8) {
        self.status.set(StatusFlags::N, value & 0x80 != 0);
        self.status.set(StatusFlags::Z, value == 0);
    }

    // ========================================================================
    // Load/Store Instructions
    // ========================================================================

    /// LDA - Load Accumulator
    fn lda(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = if addr == 0xFFFF {
            // Accumulator mode - shouldn't happen for LDA
            self.a
        } else {
            bus.read(addr)
        };
        self.a = value;
        self.update_nz_flags(value);
        extra_cycles
    }

    /// LDX - Load X Register
    fn ldx(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = if addr == 0xFFFF {
            self.x
        } else {
            bus.read(addr)
        };
        self.x = value;
        self.update_nz_flags(value);
        extra_cycles
    }

    /// LDY - Load Y Register
    fn ldy(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = if addr == 0xFFFF {
            self.y
        } else {
            bus.read(addr)
        };
        self.y = value;
        self.update_nz_flags(value);
        extra_cycles
    }

    /// STA - Store Accumulator
    fn sta(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // Stores always do dummy read on indexed modes even without page cross
        bus.write(addr, self.a);
        0
    }

    /// STX - Store X Register
    fn stx(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        bus.write(addr, self.x);
        0
    }

    /// STY - Store Y Register
    fn sty(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        bus.write(addr, self.y);
        0
    }

    // ========================================================================
    // Register Transfer Instructions
    // ========================================================================

    /// TAX - Transfer Accumulator to X
    fn tax(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.x = self.a;
        self.update_nz_flags(self.x);
        0
    }

    /// TAY - Transfer Accumulator to Y
    fn tay(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.y = self.a;
        self.update_nz_flags(self.y);
        0
    }

    /// TXA - Transfer X to Accumulator
    fn txa(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.a = self.x;
        self.update_nz_flags(self.a);
        0
    }

    /// TYA - Transfer Y to Accumulator
    fn tya(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.a = self.y;
        self.update_nz_flags(self.a);
        0
    }

    /// TSX - Transfer Stack Pointer to X
    fn tsx(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.x = self.sp;
        self.update_nz_flags(self.x);
        0
    }

    /// TXS - Transfer X to Stack Pointer
    fn txs(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.sp = self.x;
        // TXS does not affect flags
        0
    }

    // ========================================================================
    // Stack Operations
    // ========================================================================

    /// Push a byte onto the stack
    fn push(&mut self, bus: &mut dyn Bus, value: u8) {
        let addr = 0x0100 | (self.sp as u16);
        bus.write(addr, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pull a byte from the stack
    fn pull(&mut self, bus: &mut dyn Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        let addr = 0x0100 | (self.sp as u16);
        bus.read(addr)
    }

    /// PHA - Push Accumulator
    fn pha(&mut self, bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.push(bus, self.a);
        0
    }

    /// PHP - Push Processor Status
    fn php(&mut self, bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        // PHP always sets B flag when pushing
        let status = self.status | StatusFlags::B | StatusFlags::U;
        self.push(bus, status.bits());
        0
    }

    /// PLA - Pull Accumulator
    fn pla(&mut self, bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.a = self.pull(bus);
        self.update_nz_flags(self.a);
        0
    }

    /// PLP - Pull Processor Status
    fn plp(&mut self, bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        let value = self.pull(bus);
        // B flag is not restored from stack; U flag is always set
        self.status = StatusFlags::from_bits_truncate(value);
        self.status.remove(StatusFlags::B);
        self.status.insert(StatusFlags::U);
        0
    }

    // ========================================================================
    // Logical Instructions
    // ========================================================================

    /// AND - Logical AND
    fn and(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        self.a &= value;
        self.update_nz_flags(self.a);
        extra_cycles
    }

    /// EOR - Exclusive OR
    fn eor(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        self.a ^= value;
        self.update_nz_flags(self.a);
        extra_cycles
    }

    /// ORA - Logical OR
    fn ora(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        self.a |= value;
        self.update_nz_flags(self.a);
        extra_cycles
    }

    /// BIT - Bit Test
    fn bit(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        self.status.set(StatusFlags::N, value & 0x80 != 0);
        self.status.set(StatusFlags::V, value & 0x40 != 0);
        self.status.set(StatusFlags::Z, (self.a & value) == 0);
        0
    }

    // ========================================================================
    // Arithmetic Instructions
    // ========================================================================

    /// ADC - Add with Carry
    fn adc(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        let carry = if self.status.contains(StatusFlags::C) {
            1
        } else {
            0
        };
        let sum = self.a as u16 + value as u16 + carry;

        // Set carry if overflow beyond 8 bits
        self.status.set(StatusFlags::C, sum > 0xFF);

        // Overflow flag: set if sign changes incorrectly
        // V = (~(A ^ M) & (A ^ R)) & 0x80
        let result = sum as u8;
        let overflow = (!(self.a ^ value) & (self.a ^ result)) & 0x80;
        self.status.set(StatusFlags::V, overflow != 0);

        self.a = result;
        self.update_nz_flags(self.a);
        extra_cycles
    }

    /// SBC - Subtract with Carry
    fn sbc(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        let carry = if self.status.contains(StatusFlags::C) {
            1
        } else {
            0
        };
        // A - M - (1 - C) = A + ~M + C
        let subtrahend = (value as u16) ^ 0xFF;
        let diff = self.a as u16 + subtrahend + carry;

        // Set carry if no borrow (result >= 0)
        self.status.set(StatusFlags::C, diff > 0xFF);

        // Overflow flag
        let result = diff as u8;
        let overflow = ((self.a ^ result) & (self.a ^ value)) & 0x80;
        self.status.set(StatusFlags::V, overflow != 0);

        self.a = result;
        self.update_nz_flags(self.a);
        extra_cycles
    }

    /// CMP - Compare Accumulator
    fn cmp(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        let diff = self.a.wrapping_sub(value);
        self.status.set(StatusFlags::C, self.a >= value);
        self.update_nz_flags(diff);
        extra_cycles
    }

    /// CPX - Compare X Register
    fn cpx(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        let diff = self.x.wrapping_sub(value);
        self.status.set(StatusFlags::C, self.x >= value);
        self.update_nz_flags(diff);
        0
    }

    /// CPY - Compare Y Register
    fn cpy(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        let diff = self.y.wrapping_sub(value);
        self.status.set(StatusFlags::C, self.y >= value);
        self.update_nz_flags(diff);
        0
    }

    // ========================================================================
    // Increment/Decrement Instructions
    // ========================================================================

    /// INC - Increment Memory
    fn inc(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // RMW pattern: read, write original, write modified
        let value = bus.read(addr);
        bus.write(addr, value); // dummy write
        let result = value.wrapping_add(1);
        bus.write(addr, result);
        self.update_nz_flags(result);
        0
    }

    /// INX - Increment X
    fn inx(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.x = self.x.wrapping_add(1);
        self.update_nz_flags(self.x);
        0
    }

    /// INY - Increment Y
    fn iny(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.y = self.y.wrapping_add(1);
        self.update_nz_flags(self.y);
        0
    }

    /// DEC - Decrement Memory
    fn dec(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // RMW pattern: read, write original, write modified
        let value = bus.read(addr);
        bus.write(addr, value); // dummy write
        let result = value.wrapping_sub(1);
        bus.write(addr, result);
        self.update_nz_flags(result);
        0
    }

    /// DEX - Decrement X
    fn dex(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.x = self.x.wrapping_sub(1);
        self.update_nz_flags(self.x);
        0
    }

    /// DEY - Decrement Y
    fn dey(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.y = self.y.wrapping_sub(1);
        self.update_nz_flags(self.y);
        0
    }

    // ========================================================================
    // Shift Instructions
    // ========================================================================

    /// ASL - Arithmetic Shift Left
    fn asl(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if addr == 0xFFFF {
            // Accumulator mode
            self.status.set(StatusFlags::C, self.a & 0x80 != 0);
            self.a <<= 1;
            self.update_nz_flags(self.a);
        } else {
            // Memory mode - RMW pattern
            let value = bus.read(addr);
            bus.write(addr, value); // dummy write
            self.status.set(StatusFlags::C, value & 0x80 != 0);
            let result = value << 1;
            bus.write(addr, result);
            self.update_nz_flags(result);
        }
        0
    }

    /// LSR - Logical Shift Right
    fn lsr(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if addr == 0xFFFF {
            // Accumulator mode
            self.status.set(StatusFlags::C, self.a & 0x01 != 0);
            self.a >>= 1;
            self.update_nz_flags(self.a);
        } else {
            // Memory mode - RMW pattern
            let value = bus.read(addr);
            bus.write(addr, value); // dummy write
            self.status.set(StatusFlags::C, value & 0x01 != 0);
            let result = value >> 1;
            bus.write(addr, result);
            self.update_nz_flags(result);
        }
        0
    }

    /// ROL - Rotate Left
    fn rol(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let carry_in = if self.status.contains(StatusFlags::C) {
            1
        } else {
            0
        };
        if addr == 0xFFFF {
            // Accumulator mode
            self.status.set(StatusFlags::C, self.a & 0x80 != 0);
            self.a = (self.a << 1) | carry_in;
            self.update_nz_flags(self.a);
        } else {
            // Memory mode - RMW pattern
            let value = bus.read(addr);
            bus.write(addr, value); // dummy write
            self.status.set(StatusFlags::C, value & 0x80 != 0);
            let result = (value << 1) | carry_in;
            bus.write(addr, result);
            self.update_nz_flags(result);
        }
        0
    }

    /// ROR - Rotate Right
    fn ror(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let carry_in = if self.status.contains(StatusFlags::C) {
            0x80
        } else {
            0
        };
        if addr == 0xFFFF {
            // Accumulator mode
            self.status.set(StatusFlags::C, self.a & 0x01 != 0);
            self.a = (self.a >> 1) | carry_in;
            self.update_nz_flags(self.a);
        } else {
            // Memory mode - RMW pattern
            let value = bus.read(addr);
            bus.write(addr, value); // dummy write
            self.status.set(StatusFlags::C, value & 0x01 != 0);
            let result = (value >> 1) | carry_in;
            bus.write(addr, result);
            self.update_nz_flags(result);
        }
        0
    }

    // ========================================================================
    // Jump/Call Instructions
    // ========================================================================

    /// JMP - Jump
    fn jmp(&mut self, _bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // JMP sets PC to target address - 1 because PC will be incremented
        self.pc = addr;
        0
    }

    /// JSR - Jump to Subroutine
    fn jsr(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // Push return address (PC - 1) onto stack
        let return_addr = self.pc.wrapping_sub(1);
        self.push(bus, (return_addr >> 8) as u8);
        self.push(bus, (return_addr & 0xFF) as u8);
        // Read high byte of target (for open bus behavior)
        bus.read(self.pc.wrapping_add(1));
        self.pc = addr;
        0
    }

    /// RTS - Return from Subroutine
    fn rts(&mut self, bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        let lo = self.pull(bus) as u16;
        let hi = self.pull(bus) as u16;
        self.pc = ((hi << 8) | lo).wrapping_add(1);
        0
    }

    /// RTI - Return from Interrupt
    fn rti(&mut self, bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        // Pull status register
        let status = self.pull(bus);
        self.status = StatusFlags::from_bits_truncate(status);
        self.status.remove(StatusFlags::B);
        self.status.insert(StatusFlags::U);

        // Pull program counter
        let lo = self.pull(bus) as u16;
        let hi = self.pull(bus) as u16;
        self.pc = (hi << 8) | lo;
        0
    }

    /// Service a hardware interrupt (NMI or IRQ).
    ///
    /// Pushes the program counter and processor status, sets the interrupt
    /// disable flag, and jumps to the handler address stored in `vector`.
    /// Hardware interrupts push the status with the B flag clear.
    ///
    /// Returns the 7 cycles consumed, matching the real 6502.
    ///
    /// # Arguments
    /// * `bus` - Memory bus used for the stack pushes and vector fetch
    /// * `vector` - Address of the interrupt vector ($FFFA for NMI, $FFFE for IRQ)
    fn service_interrupt(&mut self, bus: &mut dyn Bus, vector: u16) -> u8 {
        self.push(bus, (self.pc >> 8) as u8);
        self.push(bus, (self.pc & 0xFF) as u8);

        // Hardware interrupts push the status with B clear and U set.
        let status = (self.status | StatusFlags::U) & !StatusFlags::B;
        self.push(bus, status.bits());

        self.status.insert(StatusFlags::I);

        let lo = bus.read(vector) as u16;
        let hi = bus.read(vector.wrapping_add(1)) as u16;
        self.pc = (hi << 8) | lo;

        7
    }

    // ========================================================================
    // Branch Instructions
    // ========================================================================

    /// Branch helper - handles page crossing for branches
    fn branch(&mut self, bus: &mut dyn Bus, target: u16) -> u8 {
        let old_pc = self.pc;
        self.pc = target;

        // Branch takes 1 extra cycle, plus 1 more if page crossed
        if (old_pc & 0xFF00) != (target & 0xFF00) {
            // Page crossed - dummy read at wrong address
            let dummy_addr = (old_pc & 0xFF00) | (target & 0x00FF);
            bus.read(dummy_addr);
            2
        } else {
            1
        }
    }

    /// BCC - Branch if Carry Clear
    fn bcc(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if !self.status.contains(StatusFlags::C) {
            self.branch(bus, addr)
        } else {
            0
        }
    }

    /// BCS - Branch if Carry Set
    fn bcs(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if self.status.contains(StatusFlags::C) {
            self.branch(bus, addr)
        } else {
            0
        }
    }

    /// BEQ - Branch if Equal (Zero Set)
    fn beq(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if self.status.contains(StatusFlags::Z) {
            self.branch(bus, addr)
        } else {
            0
        }
    }

    /// BNE - Branch if Not Equal (Zero Clear)
    fn bne(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if !self.status.contains(StatusFlags::Z) {
            self.branch(bus, addr)
        } else {
            0
        }
    }

    /// BMI - Branch if Minus (Negative Set)
    fn bmi(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if self.status.contains(StatusFlags::N) {
            self.branch(bus, addr)
        } else {
            0
        }
    }

    /// BPL - Branch if Plus (Negative Clear)
    fn bpl(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if !self.status.contains(StatusFlags::N) {
            self.branch(bus, addr)
        } else {
            0
        }
    }

    /// BVC - Branch if Overflow Clear
    fn bvc(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if !self.status.contains(StatusFlags::V) {
            self.branch(bus, addr)
        } else {
            0
        }
    }

    /// BVS - Branch if Overflow Set
    fn bvs(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        if self.status.contains(StatusFlags::V) {
            self.branch(bus, addr)
        } else {
            0
        }
    }

    // ========================================================================
    // Flag Instructions
    // ========================================================================

    /// CLC - Clear Carry
    fn clc(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.status.remove(StatusFlags::C);
        0
    }

    /// CLD - Clear Decimal
    fn cld(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.status.remove(StatusFlags::D);
        0
    }

    /// CLI - Clear Interrupt Disable
    fn cli(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.status.remove(StatusFlags::I);
        0
    }

    /// CLV - Clear Overflow
    fn clv(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.status.remove(StatusFlags::V);
        0
    }

    /// SEC - Set Carry
    fn sec(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.status.insert(StatusFlags::C);
        0
    }

    /// SED - Set Decimal
    fn sed(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.status.insert(StatusFlags::D);
        0
    }

    /// SEI - Set Interrupt Disable
    fn sei(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.status.insert(StatusFlags::I);
        0
    }

    // ========================================================================
    // System Instructions
    // ========================================================================

    /// BRK - Force Interrupt
    fn brk(&mut self, bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        // Push return address (PC + 2) onto stack
        let return_addr = self.pc.wrapping_add(1);
        self.push(bus, (return_addr >> 8) as u8);
        self.push(bus, (return_addr & 0xFF) as u8);

        // Push status with B flag set
        let status = self.status | StatusFlags::B | StatusFlags::U;
        self.push(bus, status.bits());

        // Set interrupt disable flag
        self.status.insert(StatusFlags::I);

        // Jump to IRQ vector at $FFFE
        let lo = bus.read(0xFFFE) as u16;
        let hi = bus.read(0xFFFF) as u16;
        self.pc = (hi << 8) | lo;

        0
    }

    /// NOP - No Operation
    fn nop(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        // Does nothing
        0
    }

    // ========================================================================
    // Unofficial Opcodes
    // ========================================================================

    /// ALR (ASR) - AND + LSR
    /// AND accumulator with memory, then LSR the result.
    /// Opcode: $4B (Immediate)
    fn alr(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        let result = self.a & value;
        self.status.set(StatusFlags::C, result & 0x01 != 0);
        self.a = result >> 1;
        self.update_nz_flags(self.a);
        0
    }

    /// ANC - AND + set C as bit 7
    /// AND accumulator with memory, setting carry to bit 7 of the result.
    /// Opcodes: $0B, $2B (Immediate)
    fn anc(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        self.a &= value;
        self.status.set(StatusFlags::C, self.a & 0x80 != 0);
        self.update_nz_flags(self.a);
        0
    }

    /// ARR - AND + ROR with special V/C handling
    /// AND accumulator with memory, then ROR with special flag handling.
    /// Opcode: $6B (Immediate)
    fn arr(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        let temp = self.a & value;
        let carry_in = if self.status.contains(StatusFlags::C) {
            0x80
        } else {
            0
        };
        self.a = (temp >> 1) | carry_in;
        self.update_nz_flags(self.a);
        // Special V and C handling for ARR
        self.status.set(StatusFlags::C, temp & 0x80 != 0);
        self.status
            .set(StatusFlags::V, ((temp >> 7) ^ (temp >> 6)) & 1 != 0);
        0
    }

    /// AXS (SBX) - A & X - imm, store in X
    /// Set X to (A AND X) minus immediate. Affects N, Z, C (not V).
    /// Opcode: $CB (Immediate)
    fn axs(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        let ax = self.a & self.x;
        let result = ax.wrapping_sub(value);
        self.status.set(StatusFlags::C, ax >= value);
        self.x = result;
        self.update_nz_flags(self.x);
        0
    }

    /// LAX - Load A and X
    /// Load both A and X from memory.
    /// Opcodes: $A3, $A7, $AB, $AF, $B3, $B7, $BF
    fn lax(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        self.a = value;
        self.x = value;
        self.update_nz_flags(value);
        extra_cycles
    }

    /// SAX - Store A AND X
    /// Store A AND X to memory.
    /// Opcodes: $83, $87, $8F, $97
    fn sax(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        bus.write(addr, self.a & self.x);
        0
    }

    /// DCP (DCM) - DEC + CMP
    /// Decrement memory, then compare with A.
    /// Opcodes: $C3, $C7, $CF, $D3, $D7, $DB, $DF
    fn dcp(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // RMW pattern: read, write original, write modified
        let value = bus.read(addr);
        bus.write(addr, value); // dummy write
        let result = value.wrapping_sub(1);
        bus.write(addr, result);
        // Compare with accumulator
        let diff = self.a.wrapping_sub(result);
        self.status.set(StatusFlags::C, self.a >= result);
        self.update_nz_flags(diff);
        0
    }

    /// ISC (ISB, INS) - INC + SBC
    /// Increment memory, then subtract from A.
    /// Opcodes: $E3, $E7, $EF, $F3, $F7, $FB, $FF
    fn isc(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // RMW pattern: read, write original, write modified
        let value = bus.read(addr);
        bus.write(addr, value); // dummy write
        let result = value.wrapping_add(1);
        bus.write(addr, result);
        // SBC the result from accumulator
        let carry = if self.status.contains(StatusFlags::C) {
            1
        } else {
            0
        };
        let subtrahend = (result as u16) ^ 0xFF;
        let diff = self.a as u16 + subtrahend + carry;
        self.status.set(StatusFlags::C, diff > 0xFF);
        let result_a = diff as u8;
        let overflow = ((self.a ^ result_a) & (self.a ^ result)) & 0x80;
        self.status.set(StatusFlags::V, overflow != 0);
        self.a = result_a;
        self.update_nz_flags(self.a);
        0
    }

    /// RLA - ROL + AND
    /// Rotate memory left, then AND with A.
    /// Opcodes: $23, $27, $2F, $33, $37, $3B, $3F
    fn rla(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // RMW pattern: read, write original, write modified
        let value = bus.read(addr);
        bus.write(addr, value); // dummy write
        let carry_in = if self.status.contains(StatusFlags::C) {
            1
        } else {
            0
        };
        self.status.set(StatusFlags::C, value & 0x80 != 0);
        let result = (value << 1) | carry_in;
        bus.write(addr, result);
        // AND with accumulator
        self.a &= result;
        self.update_nz_flags(self.a);
        0
    }

    /// RRA - ROR + ADC
    /// Rotate memory right, then ADC to A.
    /// Opcodes: $63, $67, $6F, $73, $77, $7B, $7F
    fn rra(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // RMW pattern: read, write original, write modified
        let value = bus.read(addr);
        bus.write(addr, value); // dummy write
        let carry_in = if self.status.contains(StatusFlags::C) {
            0x80
        } else {
            0
        };
        self.status.set(StatusFlags::C, value & 0x01 != 0);
        let result = (value >> 1) | carry_in;
        bus.write(addr, result);
        // ADC to accumulator
        let carry = if self.status.contains(StatusFlags::C) {
            1
        } else {
            0
        };
        let sum = self.a as u16 + result as u16 + carry;
        self.status.set(StatusFlags::C, sum > 0xFF);
        let result_a = sum as u8;
        let overflow = (!(self.a ^ result) & (self.a ^ result_a)) & 0x80;
        self.status.set(StatusFlags::V, overflow != 0);
        self.a = result_a;
        self.update_nz_flags(self.a);
        0
    }

    /// SLO (ASO) - ASL + ORA
    /// Shift memory left, then ORA with A.
    /// Opcodes: $03, $07, $0F, $13, $17, $1B, $1F
    fn slo(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // RMW pattern: read, write original, write modified
        let value = bus.read(addr);
        bus.write(addr, value); // dummy write
        self.status.set(StatusFlags::C, value & 0x80 != 0);
        let result = value << 1;
        bus.write(addr, result);
        // ORA with accumulator
        self.a |= result;
        self.update_nz_flags(self.a);
        0
    }

    /// SRE (LSE) - LSR + EOR
    /// Shift memory right, then EOR with A.
    /// Opcodes: $43, $47, $4F, $53, $57, $5B, $5F
    fn sre(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // RMW pattern: read, write original, write modified
        let value = bus.read(addr);
        bus.write(addr, value); // dummy write
        self.status.set(StatusFlags::C, value & 0x01 != 0);
        let result = value >> 1;
        bus.write(addr, result);
        // EOR with accumulator
        self.a ^= result;
        self.update_nz_flags(self.a);
        0
    }

    /// SKB (NOP immediate) - Skip byte
    /// Do nothing (immediate read is already handled by addressing mode).
    /// Opcodes: $80, $82, $89, $C2, $E2
    fn skb(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        // Immediate read already happened in addressing mode
        0
    }

    /// IGN (NOP with read) - Ignore byte
    /// Read from memory and ignore result.
    /// Opcodes: $04, $44, $64, $14, $34, $54, $74, $D4, $F4, $1C, $3C, $5C, $7C, $DC, $FC
    fn ign(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        // Perform the read (already done by addressing mode for some)
        bus.read(addr);
        extra_cycles
    }

    /// SHA (AHX, AXA) - Store A & X & (high byte + 1)
    /// Store A AND X AND (high byte of address + 1) to memory.
    /// Has special behavior on page crossing.
    /// Opcodes: $93 (Indirect,Y), $9F (Absolute,Y)
    fn sha(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        // The value to store is A & X & ((high byte of base address) + 1)
        // For indirect Y, we need to calculate this from the pointer
        // For absolute Y, base high byte is known from addressing
        // Simplified: use high byte of effective address
        let high_byte = ((addr >> 8) + 1) as u8;
        let value = self.a & self.x & high_byte;
        bus.write(addr, value);
        0
    }

    /// SHX (SXA, XAS) - Store X & (high byte + 1)
    /// Store X AND (high byte of address + 1) to memory.
    /// Opcode: $9E (Absolute,Y)
    fn shx(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let high_byte = ((addr >> 8) + 1) as u8;
        let value = self.x & high_byte;
        bus.write(addr, value);
        0
    }

    /// SHY (SYA, SAY) - Store Y & (high byte + 1)
    /// Store Y AND (high byte of address + 1) to memory.
    /// Opcode: $9C (Absolute,X)
    fn shy(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        let high_byte = ((addr >> 8) + 1) as u8;
        let value = self.y & high_byte;
        bus.write(addr, value);
        0
    }

    /// SHS (TAS, XAS) - Transfer A & X to SP, then SHA
    /// SP = A & X, then store SP & (high byte + 1).
    /// Opcode: $9B (Absolute,Y)
    fn shs(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        self.sp = self.a & self.x;
        let high_byte = ((addr >> 8) + 1) as u8;
        let value = self.sp & high_byte;
        bus.write(addr, value);
        0
    }

    /// LAE (LAS, LAR) - Load A, X, SP from memory & SP
    /// A = X = SP = memory AND SP
    /// Opcode: $BB (Absolute,Y)
    fn lae(&mut self, bus: &mut dyn Bus, addr: u16, extra_cycles: u8) -> u8 {
        let value = bus.read(addr);
        let result = value & self.sp;
        self.a = result;
        self.x = result;
        self.sp = result;
        self.update_nz_flags(result);
        extra_cycles
    }

    /// ANE (XAA) - A = (A | MAGIC) & X & imm
    /// Transfer X to A, then AND with immediate.
    /// The "magic" constant varies between CPU revisions ($00, $EE, $FF).
    /// Using $FF as the most common value.
    /// Opcode: $8B (Immediate)
    fn ane(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        const MAGIC: u8 = 0xFF;
        let value = bus.read(addr);
        self.a = (self.a | MAGIC) & self.x & value;
        self.update_nz_flags(self.a);
        0
    }

    /// LXA (LAX immediate, ATX) - Load A and X with (A | MAGIC) & imm
    /// A = X = (A | MAGIC) & immediate
    /// Same magic constant issue as ANE.
    /// Opcode: $AB (Immediate)
    fn lxa(&mut self, bus: &mut dyn Bus, addr: u16, _extra_cycles: u8) -> u8 {
        const MAGIC: u8 = 0xFF;
        let value = bus.read(addr);
        self.a = (self.a | MAGIC) & value;
        self.x = self.a;
        self.update_nz_flags(self.a);
        0
    }

    /// DUMMY (NOP) - Cycle halt / No operation
    /// Does nothing but consume cycles.
    /// Opcodes: $1A, $3A, $5A, $7A, $DA, $FA (Implied)
    fn dummy(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        // NOP - does nothing
        0
    }

    /// KIL (JAM, HLT) - Stop the CPU
    ///
    /// The twelve KIL opcodes leave the 6502 hung: it stops fetching, and only
    /// an interrupt (NMI, or IRQ with the I flag clear) or a reset brings it
    /// back. Cartridges do not use them deliberately — a corrupted ROM or a
    /// jump into data is how they show up — but hanging is what the hardware
    /// does, and it is far better than tearing down the host: `wasm32` aborts
    /// the whole instance when the core panics.
    ///
    /// Opcodes: $02, $12, $22, $32, $42, $52, $62, $72, $92, $B2, $D2, $F2
    fn jam(&mut self, _bus: &mut dyn Bus, _addr: u16, _extra_cycles: u8) -> u8 {
        self.halted = true;
        0
    }
}

/// The complete 6502 opcode table (256 entries)
///
/// Each entry maps an opcode byte to its metadata and handler. Every one of the
/// 256 byte values has a handler: the documented instructions, the undocumented
/// ones the 2A03 really executes (LAX/SAX/DCP/ISC/SLO/RLA/SRE/RRA …), the NOP
/// variants, and the twelve KILs that hang the CPU.
pub static OPCODE_TABLE: [Opcode; 256] = [
    // 0x00 - BRK (Implied)
    Opcode {
        mnemonic: "BRK",
        addr_mode: AddressingMode::Implied,
        base_cycles: 7,
        handler: Cpu::brk,
    },
    // 0x01 - ORA (Indirect,X)
    Opcode {
        mnemonic: "ORA",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::ora,
    },
    // 0x02 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0x03 - SLO (Indirect,X)
    Opcode {
        mnemonic: "SLO",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 8,
        handler: Cpu::slo,
    },
    // 0x04 - IGN (Zero Page)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::ign,
    },
    // 0x05 - ORA (Zero Page)
    Opcode {
        mnemonic: "ORA",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::ora,
    },
    // 0x06 - ASL (Zero Page)
    Opcode {
        mnemonic: "ASL",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::asl,
    },
    // 0x07 - SLO (Zero Page)
    Opcode {
        mnemonic: "SLO",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::slo,
    },
    // 0x08 - PHP (Implied)
    Opcode {
        mnemonic: "PHP",
        addr_mode: AddressingMode::Implied,
        base_cycles: 3,
        handler: Cpu::php,
    },
    // 0x09 - ORA (Immediate)
    Opcode {
        mnemonic: "ORA",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::ora,
    },
    // 0x0A - ASL (Accumulator)
    Opcode {
        mnemonic: "ASL",
        addr_mode: AddressingMode::Accumulator,
        base_cycles: 2,
        handler: Cpu::asl,
    },
    // 0x0B - ANC (Immediate)
    Opcode {
        mnemonic: "ANC",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::anc,
    },
    // 0x0C - NOP (Absolute) - undocumented, 3 bytes / 4 cycles
    Opcode {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::nop,
    },
    // 0x0D - ORA (Absolute)
    Opcode {
        mnemonic: "ORA",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::ora,
    },
    // 0x0E - ASL (Absolute)
    Opcode {
        mnemonic: "ASL",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::asl,
    },
    // 0x0F - SLO (Absolute)
    Opcode {
        mnemonic: "SLO",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::slo,
    },
    // 0x10 - BPL (Relative)
    Opcode {
        mnemonic: "BPL",
        addr_mode: AddressingMode::Relative,
        base_cycles: 2,
        handler: Cpu::bpl,
    },
    // 0x11 - ORA (Indirect,Y)
    Opcode {
        mnemonic: "ORA",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 5,
        handler: Cpu::ora,
    },
    // 0x12 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0x13 - SLO (Indirect,Y)
    Opcode {
        mnemonic: "SLO",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 8,
        handler: Cpu::slo,
    },
    // 0x14 - IGN (Zero Page,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0x15 - ORA (Zero Page,X)
    Opcode {
        mnemonic: "ORA",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::ora,
    },
    // 0x16 - ASL (Zero Page,X)
    Opcode {
        mnemonic: "ASL",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::asl,
    },
    // 0x17 - SLO (Zero Page,X)
    Opcode {
        mnemonic: "SLO",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::slo,
    },
    // 0x18 - CLC (Implied)
    Opcode {
        mnemonic: "CLC",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::clc,
    },
    // 0x19 - ORA (Absolute,Y)
    Opcode {
        mnemonic: "ORA",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::ora,
    },
    // 0x1A - NOP (Implied)
    Opcode {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::dummy,
    },
    // 0x1B - SLO (Absolute,Y)
    Opcode {
        mnemonic: "SLO",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 7,
        handler: Cpu::slo,
    },
    // 0x1C - IGN (Absolute,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0x1D - ORA (Absolute,X)
    Opcode {
        mnemonic: "ORA",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::ora,
    },
    // 0x1E - ASL (Absolute,X)
    Opcode {
        mnemonic: "ASL",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::asl,
    },
    // 0x1F - SLO (Absolute,X)
    Opcode {
        mnemonic: "SLO",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::slo,
    },
    // 0x20 - JSR (Absolute)
    Opcode {
        mnemonic: "JSR",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::jsr,
    },
    // 0x21 - AND (Indirect,X)
    Opcode {
        mnemonic: "AND",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::and,
    },
    // 0x22 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0x23 - RLA (Indirect,X)
    Opcode {
        mnemonic: "RLA",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 8,
        handler: Cpu::rla,
    },
    // 0x24 - BIT (Zero Page)
    Opcode {
        mnemonic: "BIT",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::bit,
    },
    // 0x25 - AND (Zero Page)
    Opcode {
        mnemonic: "AND",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::and,
    },
    // 0x26 - ROL (Zero Page)
    Opcode {
        mnemonic: "ROL",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::rol,
    },
    // 0x27 - RLA (Zero Page)
    Opcode {
        mnemonic: "RLA",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::rla,
    },
    // 0x28 - PLP (Implied)
    Opcode {
        mnemonic: "PLP",
        addr_mode: AddressingMode::Implied,
        base_cycles: 4,
        handler: Cpu::plp,
    },
    // 0x29 - AND (Immediate)
    Opcode {
        mnemonic: "AND",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::and,
    },
    // 0x2A - ROL (Accumulator)
    Opcode {
        mnemonic: "ROL",
        addr_mode: AddressingMode::Accumulator,
        base_cycles: 2,
        handler: Cpu::rol,
    },
    // 0x2B - ANC (Immediate)
    Opcode {
        mnemonic: "ANC",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::anc,
    },
    // 0x2C - BIT (Absolute)
    Opcode {
        mnemonic: "BIT",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::bit,
    },
    // 0x2D - AND (Absolute)
    Opcode {
        mnemonic: "AND",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::and,
    },
    // 0x2E - ROL (Absolute)
    Opcode {
        mnemonic: "ROL",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::rol,
    },
    // 0x2F - RLA (Absolute)
    Opcode {
        mnemonic: "RLA",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::rla,
    },
    // 0x30 - BMI (Relative)
    Opcode {
        mnemonic: "BMI",
        addr_mode: AddressingMode::Relative,
        base_cycles: 2,
        handler: Cpu::bmi,
    },
    // 0x31 - AND (Indirect,Y)
    Opcode {
        mnemonic: "AND",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 5,
        handler: Cpu::and,
    },
    // 0x32 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0x33 - RLA (Indirect,Y)
    Opcode {
        mnemonic: "RLA",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 8,
        handler: Cpu::rla,
    },
    // 0x34 - IGN (Zero Page,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0x35 - AND (Zero Page,X)
    Opcode {
        mnemonic: "AND",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::and,
    },
    // 0x36 - ROL (Zero Page,X)
    Opcode {
        mnemonic: "ROL",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::rol,
    },
    // 0x37 - RLA (Zero Page,X)
    Opcode {
        mnemonic: "RLA",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::rla,
    },
    // 0x38 - SEC (Implied)
    Opcode {
        mnemonic: "SEC",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::sec,
    },
    // 0x39 - AND (Absolute,Y)
    Opcode {
        mnemonic: "AND",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::and,
    },
    // 0x3A - NOP (Implied)
    Opcode {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::dummy,
    },
    // 0x3B - RLA (Absolute,Y)
    Opcode {
        mnemonic: "RLA",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 7,
        handler: Cpu::rla,
    },
    // 0x3C - IGN (Absolute,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0x3D - AND (Absolute,X)
    Opcode {
        mnemonic: "AND",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::and,
    },
    // 0x3E - ROL (Absolute,X)
    Opcode {
        mnemonic: "ROL",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::rol,
    },
    // 0x3F - RLA (Absolute,X)
    Opcode {
        mnemonic: "RLA",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::rla,
    },
    // 0x40 - RTI (Implied)
    Opcode {
        mnemonic: "RTI",
        addr_mode: AddressingMode::Implied,
        base_cycles: 6,
        handler: Cpu::rti,
    },
    // 0x41 - EOR (Indirect,X)
    Opcode {
        mnemonic: "EOR",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::eor,
    },
    // 0x42 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0x43 - SRE (Indirect,X)
    Opcode {
        mnemonic: "SRE",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 8,
        handler: Cpu::sre,
    },
    // 0x44 - IGN (Zero Page)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::ign,
    },
    // 0x45 - EOR (Zero Page)
    Opcode {
        mnemonic: "EOR",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::eor,
    },
    // 0x46 - LSR (Zero Page)
    Opcode {
        mnemonic: "LSR",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::lsr,
    },
    // 0x47 - SRE (Zero Page)
    Opcode {
        mnemonic: "SRE",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::sre,
    },
    // 0x48 - PHA (Implied)
    Opcode {
        mnemonic: "PHA",
        addr_mode: AddressingMode::Implied,
        base_cycles: 3,
        handler: Cpu::pha,
    },
    // 0x49 - EOR (Immediate)
    Opcode {
        mnemonic: "EOR",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::eor,
    },
    // 0x4A - LSR (Accumulator)
    Opcode {
        mnemonic: "LSR",
        addr_mode: AddressingMode::Accumulator,
        base_cycles: 2,
        handler: Cpu::lsr,
    },
    // 0x4B - ALR (Immediate)
    Opcode {
        mnemonic: "ALR",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::alr,
    },
    // 0x4C - JMP (Absolute)
    Opcode {
        mnemonic: "JMP",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 3,
        handler: Cpu::jmp,
    },
    // 0x4D - EOR (Absolute)
    Opcode {
        mnemonic: "EOR",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::eor,
    },
    // 0x4E - LSR (Absolute)
    Opcode {
        mnemonic: "LSR",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::lsr,
    },
    // 0x4F - SRE (Absolute)
    Opcode {
        mnemonic: "SRE",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::sre,
    },
    // 0x50 - BVC (Relative)
    Opcode {
        mnemonic: "BVC",
        addr_mode: AddressingMode::Relative,
        base_cycles: 2,
        handler: Cpu::bvc,
    },
    // 0x51 - EOR (Indirect,Y)
    Opcode {
        mnemonic: "EOR",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 5,
        handler: Cpu::eor,
    },
    // 0x52 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0x53 - SRE (Indirect,Y)
    Opcode {
        mnemonic: "SRE",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 8,
        handler: Cpu::sre,
    },
    // 0x54 - IGN (Zero Page,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0x55 - EOR (Zero Page,X)
    Opcode {
        mnemonic: "EOR",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::eor,
    },
    // 0x56 - LSR (Zero Page,X)
    Opcode {
        mnemonic: "LSR",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::lsr,
    },
    // 0x57 - SRE (Zero Page,X)
    Opcode {
        mnemonic: "SRE",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::sre,
    },
    // 0x58 - CLI (Implied)
    Opcode {
        mnemonic: "CLI",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::cli,
    },
    // 0x59 - EOR (Absolute,Y)
    Opcode {
        mnemonic: "EOR",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::eor,
    },
    // 0x5A - NOP (Implied)
    Opcode {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::dummy,
    },
    // 0x5B - SRE (Absolute,Y)
    Opcode {
        mnemonic: "SRE",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 7,
        handler: Cpu::sre,
    },
    // 0x5C - IGN (Absolute,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0x5D - EOR (Absolute,X)
    Opcode {
        mnemonic: "EOR",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::eor,
    },
    // 0x5E - LSR (Absolute,X)
    Opcode {
        mnemonic: "LSR",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::lsr,
    },
    // 0x5F - SRE (Absolute,X)
    Opcode {
        mnemonic: "SRE",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::sre,
    },
    // 0x60 - RTS (Implied)
    Opcode {
        mnemonic: "RTS",
        addr_mode: AddressingMode::Implied,
        base_cycles: 6,
        handler: Cpu::rts,
    },
    // 0x61 - ADC (Indirect,X)
    Opcode {
        mnemonic: "ADC",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::adc,
    },
    // 0x62 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0x63 - RRA (Indirect,X)
    Opcode {
        mnemonic: "RRA",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 8,
        handler: Cpu::rra,
    },
    // 0x64 - IGN (Zero Page)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::ign,
    },
    // 0x65 - ADC (Zero Page)
    Opcode {
        mnemonic: "ADC",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::adc,
    },
    // 0x66 - ROR (Zero Page)
    Opcode {
        mnemonic: "ROR",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::ror,
    },
    // 0x67 - RRA (Zero Page)
    Opcode {
        mnemonic: "RRA",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::rra,
    },
    // 0x68 - PLA (Implied)
    Opcode {
        mnemonic: "PLA",
        addr_mode: AddressingMode::Implied,
        base_cycles: 4,
        handler: Cpu::pla,
    },
    // 0x69 - ADC (Immediate)
    Opcode {
        mnemonic: "ADC",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::adc,
    },
    // 0x6A - ROR (Accumulator)
    Opcode {
        mnemonic: "ROR",
        addr_mode: AddressingMode::Accumulator,
        base_cycles: 2,
        handler: Cpu::ror,
    },
    // 0x6B - ARR (Immediate)
    Opcode {
        mnemonic: "ARR",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::arr,
    },
    // 0x6C - JMP (Indirect)
    Opcode {
        mnemonic: "JMP",
        addr_mode: AddressingMode::Indirect,
        base_cycles: 5,
        handler: Cpu::jmp,
    },
    // 0x6D - ADC (Absolute)
    Opcode {
        mnemonic: "ADC",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::adc,
    },
    // 0x6E - ROR (Absolute)
    Opcode {
        mnemonic: "ROR",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::ror,
    },
    // 0x6F - RRA (Absolute)
    Opcode {
        mnemonic: "RRA",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::rra,
    },
    // 0x70 - BVS (Relative)
    Opcode {
        mnemonic: "BVS",
        addr_mode: AddressingMode::Relative,
        base_cycles: 2,
        handler: Cpu::bvs,
    },
    // 0x71 - ADC (Indirect,Y)
    Opcode {
        mnemonic: "ADC",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 5,
        handler: Cpu::adc,
    },
    // 0x72 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0x73 - RRA (Indirect,Y)
    Opcode {
        mnemonic: "RRA",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 8,
        handler: Cpu::rra,
    },
    // 0x74 - IGN (Zero Page,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0x75 - ADC (Zero Page,X)
    Opcode {
        mnemonic: "ADC",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::adc,
    },
    // 0x76 - ROR (Zero Page,X)
    Opcode {
        mnemonic: "ROR",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::ror,
    },
    // 0x77 - RRA (Zero Page,X)
    Opcode {
        mnemonic: "RRA",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::rra,
    },
    // 0x78 - SEI (Implied)
    Opcode {
        mnemonic: "SEI",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::sei,
    },
    // 0x79 - ADC (Absolute,Y)
    Opcode {
        mnemonic: "ADC",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::adc,
    },
    // 0x7A - NOP (Implied)
    Opcode {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::dummy,
    },
    // 0x7B - RRA (Absolute,Y)
    Opcode {
        mnemonic: "RRA",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 7,
        handler: Cpu::rra,
    },
    // 0x7C - IGN (Absolute,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0x7D - ADC (Absolute,X)
    Opcode {
        mnemonic: "ADC",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::adc,
    },
    // 0x7E - ROR (Absolute,X)
    Opcode {
        mnemonic: "ROR",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::ror,
    },
    // 0x7F - RRA (Absolute,X)
    Opcode {
        mnemonic: "RRA",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::rra,
    },
    // 0x80 - SKB (Immediate)
    Opcode {
        mnemonic: "SKB",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::skb,
    },
    // 0x81 - STA (Indirect,X)
    Opcode {
        mnemonic: "STA",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::sta,
    },
    // 0x82 - SKB (Immediate)
    Opcode {
        mnemonic: "SKB",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::skb,
    },
    // 0x83 - SAX (Indirect,X)
    Opcode {
        mnemonic: "SAX",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::sax,
    },
    // 0x84 - STY (Zero Page)
    Opcode {
        mnemonic: "STY",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::sty,
    },
    // 0x85 - STA (Zero Page)
    Opcode {
        mnemonic: "STA",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::sta,
    },
    // 0x86 - STX (Zero Page)
    Opcode {
        mnemonic: "STX",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::stx,
    },
    // 0x87 - SAX (Zero Page)
    Opcode {
        mnemonic: "SAX",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::sax,
    },
    // 0x88 - DEY (Implied)
    Opcode {
        mnemonic: "DEY",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::dey,
    },
    // 0x89 - SKB (Immediate)
    Opcode {
        mnemonic: "SKB",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::skb,
    },
    // 0x8A - TXA (Implied)
    Opcode {
        mnemonic: "TXA",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::txa,
    },
    // 0x8B - ANE (Immediate)
    Opcode {
        mnemonic: "ANE",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::ane,
    },
    // 0x8C - STY (Absolute)
    Opcode {
        mnemonic: "STY",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::sty,
    },
    // 0x8D - STA (Absolute)
    Opcode {
        mnemonic: "STA",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::sta,
    },
    // 0x8E - STX (Absolute)
    Opcode {
        mnemonic: "STX",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::stx,
    },
    // 0x8F - SAX (Absolute)
    Opcode {
        mnemonic: "SAX",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::sax,
    },
    // 0x90 - BCC (Relative)
    Opcode {
        mnemonic: "BCC",
        addr_mode: AddressingMode::Relative,
        base_cycles: 2,
        handler: Cpu::bcc,
    },
    // 0x91 - STA (Indirect,Y)
    Opcode {
        mnemonic: "STA",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 6,
        handler: Cpu::sta,
    },
    // 0x92 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0x93 - SHA (Indirect,Y)
    Opcode {
        mnemonic: "SHA",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 6,
        handler: Cpu::sha,
    },
    // 0x94 - STY (Zero Page,X)
    Opcode {
        mnemonic: "STY",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::sty,
    },
    // 0x95 - STA (Zero Page,X)
    Opcode {
        mnemonic: "STA",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::sta,
    },
    // 0x96 - STX (Zero Page,Y)
    Opcode {
        mnemonic: "STX",
        addr_mode: AddressingMode::ZeroPageY,
        base_cycles: 4,
        handler: Cpu::stx,
    },
    // 0x97 - SAX (Zero Page,Y)
    Opcode {
        mnemonic: "SAX",
        addr_mode: AddressingMode::ZeroPageY,
        base_cycles: 4,
        handler: Cpu::sax,
    },
    // 0x98 - TYA (Implied)
    Opcode {
        mnemonic: "TYA",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::tya,
    },
    // 0x99 - STA (Absolute,Y)
    Opcode {
        mnemonic: "STA",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 5,
        handler: Cpu::sta,
    },
    // 0x9A - TXS (Implied)
    Opcode {
        mnemonic: "TXS",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::txs,
    },
    // 0x9B - SHS (Absolute,Y)
    Opcode {
        mnemonic: "SHS",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 5,
        handler: Cpu::shs,
    },
    // 0x9C - SHY (Absolute,X)
    Opcode {
        mnemonic: "SHY",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 5,
        handler: Cpu::shy,
    },
    // 0x9D - STA (Absolute,X)
    Opcode {
        mnemonic: "STA",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 5,
        handler: Cpu::sta,
    },
    // 0x9E - SHX (Absolute,Y)
    Opcode {
        mnemonic: "SHX",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 5,
        handler: Cpu::shx,
    },
    // 0x9F - SHA (Absolute,Y)
    Opcode {
        mnemonic: "SHA",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 5,
        handler: Cpu::sha,
    },
    // 0xA0 - LDY (Immediate)
    Opcode {
        mnemonic: "LDY",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::ldy,
    },
    // 0xA1 - LDA (Indirect,X)
    Opcode {
        mnemonic: "LDA",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::lda,
    },
    // 0xA2 - LDX (Immediate)
    Opcode {
        mnemonic: "LDX",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::ldx,
    },
    // 0xA3 - LAX (Indirect,X)
    Opcode {
        mnemonic: "LAX",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::lax,
    },
    // 0xA4 - LDY (Zero Page)
    Opcode {
        mnemonic: "LDY",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::ldy,
    },
    // 0xA5 - LDA (Zero Page)
    Opcode {
        mnemonic: "LDA",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::lda,
    },
    // 0xA6 - LDX (Zero Page)
    Opcode {
        mnemonic: "LDX",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::ldx,
    },
    // 0xA7 - LAX (Zero Page)
    Opcode {
        mnemonic: "LAX",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::lax,
    },
    // 0xA8 - TAY (Implied)
    Opcode {
        mnemonic: "TAY",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::tay,
    },
    // 0xA9 - LDA (Immediate)
    Opcode {
        mnemonic: "LDA",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::lda,
    },
    // 0xAA - TAX (Implied)
    Opcode {
        mnemonic: "TAX",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::tax,
    },
    // 0xAB - LXA (Immediate)
    Opcode {
        mnemonic: "LXA",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::lxa,
    },
    // 0xAC - LDY (Absolute)
    Opcode {
        mnemonic: "LDY",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::ldy,
    },
    // 0xAD - LDA (Absolute)
    Opcode {
        mnemonic: "LDA",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::lda,
    },
    // 0xAE - LDX (Absolute)
    Opcode {
        mnemonic: "LDX",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::ldx,
    },
    // 0xAF - LAX (Absolute)
    Opcode {
        mnemonic: "LAX",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::lax,
    },
    // 0xB0 - BCS (Relative)
    Opcode {
        mnemonic: "BCS",
        addr_mode: AddressingMode::Relative,
        base_cycles: 2,
        handler: Cpu::bcs,
    },
    // 0xB1 - LDA (Indirect,Y)
    Opcode {
        mnemonic: "LDA",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 5,
        handler: Cpu::lda,
    },
    // 0xB2 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0xB3 - LAX (Indirect,Y)
    Opcode {
        mnemonic: "LAX",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 5,
        handler: Cpu::lax,
    },
    // 0xB4 - LDY (Zero Page,X)
    Opcode {
        mnemonic: "LDY",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::ldy,
    },
    // 0xB5 - LDA (Zero Page,X)
    Opcode {
        mnemonic: "LDA",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::lda,
    },
    // 0xB6 - LDX (Zero Page,Y)
    Opcode {
        mnemonic: "LDX",
        addr_mode: AddressingMode::ZeroPageY,
        base_cycles: 4,
        handler: Cpu::ldx,
    },
    // 0xB7 - LAX (Zero Page,Y)
    Opcode {
        mnemonic: "LAX",
        addr_mode: AddressingMode::ZeroPageY,
        base_cycles: 4,
        handler: Cpu::lax,
    },
    // 0xB8 - CLV (Implied)
    Opcode {
        mnemonic: "CLV",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::clv,
    },
    // 0xB9 - LDA (Absolute,Y)
    Opcode {
        mnemonic: "LDA",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::lda,
    },
    // 0xBA - TSX (Implied)
    Opcode {
        mnemonic: "TSX",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::tsx,
    },
    // 0xBB - LAE (Absolute,Y)
    Opcode {
        mnemonic: "LAE",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::lae,
    },
    // 0xBC - LDY (Absolute,X)
    Opcode {
        mnemonic: "LDY",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::ldy,
    },
    // 0xBD - LDA (Absolute,X)
    Opcode {
        mnemonic: "LDA",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::lda,
    },
    // 0xBE - LDX (Absolute,Y)
    Opcode {
        mnemonic: "LDX",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::ldx,
    },
    // 0xBF - LAX (Absolute,Y)
    Opcode {
        mnemonic: "LAX",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::lax,
    },
    // 0xC0 - CPY (Immediate)
    Opcode {
        mnemonic: "CPY",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::cpy,
    },
    // 0xC1 - CMP (Indirect,X)
    Opcode {
        mnemonic: "CMP",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::cmp,
    },
    // 0xC2 - SKB (Immediate)
    Opcode {
        mnemonic: "SKB",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::skb,
    },
    // 0xC3 - DCP (Indirect,X)
    Opcode {
        mnemonic: "DCP",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 8,
        handler: Cpu::dcp,
    },
    // 0xC4 - CPY (Zero Page)
    Opcode {
        mnemonic: "CPY",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::cpy,
    },
    // 0xC5 - CMP (Zero Page)
    Opcode {
        mnemonic: "CMP",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::cmp,
    },
    // 0xC6 - DEC (Zero Page)
    Opcode {
        mnemonic: "DEC",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::dec,
    },
    // 0xC7 - DCP (Zero Page)
    Opcode {
        mnemonic: "DCP",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::dcp,
    },
    // 0xC8 - INY (Implied)
    Opcode {
        mnemonic: "INY",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::iny,
    },
    // 0xC9 - CMP (Immediate)
    Opcode {
        mnemonic: "CMP",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::cmp,
    },
    // 0xCA - DEX (Implied)
    Opcode {
        mnemonic: "DEX",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::dex,
    },
    // 0xCB - AXS (Immediate)
    Opcode {
        mnemonic: "AXS",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::axs,
    },
    // 0xCC - CPY (Absolute)
    Opcode {
        mnemonic: "CPY",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::cpy,
    },
    // 0xCD - CMP (Absolute)
    Opcode {
        mnemonic: "CMP",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::cmp,
    },
    // 0xCE - DEC (Absolute)
    Opcode {
        mnemonic: "DEC",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::dec,
    },
    // 0xCF - DCP (Absolute)
    Opcode {
        mnemonic: "DCP",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::dcp,
    },
    // 0xD0 - BNE (Relative)
    Opcode {
        mnemonic: "BNE",
        addr_mode: AddressingMode::Relative,
        base_cycles: 2,
        handler: Cpu::bne,
    },
    // 0xD1 - CMP (Indirect,Y)
    Opcode {
        mnemonic: "CMP",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 5,
        handler: Cpu::cmp,
    },
    // 0xD2 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0xD3 - DCP (Indirect,Y)
    Opcode {
        mnemonic: "DCP",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 8,
        handler: Cpu::dcp,
    },
    // 0xD4 - IGN (Zero Page,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0xD5 - CMP (Zero Page,X)
    Opcode {
        mnemonic: "CMP",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::cmp,
    },
    // 0xD6 - DEC (Zero Page,X)
    Opcode {
        mnemonic: "DEC",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::dec,
    },
    // 0xD7 - DCP (Zero Page,X)
    Opcode {
        mnemonic: "DCP",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::dcp,
    },
    // 0xD8 - CLD (Implied)
    Opcode {
        mnemonic: "CLD",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::cld,
    },
    // 0xD9 - CMP (Absolute,Y)
    Opcode {
        mnemonic: "CMP",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::cmp,
    },
    // 0xDA - NOP (Implied)
    Opcode {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::dummy,
    },
    // 0xDB - DCP (Absolute,Y)
    Opcode {
        mnemonic: "DCP",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 7,
        handler: Cpu::dcp,
    },
    // 0xDC - IGN (Absolute,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0xDD - CMP (Absolute,X)
    Opcode {
        mnemonic: "CMP",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::cmp,
    },
    // 0xDE - DEC (Absolute,X)
    Opcode {
        mnemonic: "DEC",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::dec,
    },
    // 0xDF - DCP (Absolute,X)
    Opcode {
        mnemonic: "DCP",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::dcp,
    },
    // 0xE0 - CPX (Immediate)
    Opcode {
        mnemonic: "CPX",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::cpx,
    },
    // 0xE1 - SBC (Indirect,X)
    Opcode {
        mnemonic: "SBC",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 6,
        handler: Cpu::sbc,
    },
    // 0xE2 - SKB (Immediate)
    Opcode {
        mnemonic: "SKB",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::skb,
    },
    // 0xE3 - ISC (Indirect,X)
    Opcode {
        mnemonic: "ISC",
        addr_mode: AddressingMode::IndirectX,
        base_cycles: 8,
        handler: Cpu::isc,
    },
    // 0xE4 - CPX (Zero Page)
    Opcode {
        mnemonic: "CPX",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::cpx,
    },
    // 0xE5 - SBC (Zero Page)
    Opcode {
        mnemonic: "SBC",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 3,
        handler: Cpu::sbc,
    },
    // 0xE6 - INC (Zero Page)
    Opcode {
        mnemonic: "INC",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::inc,
    },
    // 0xE7 - ISC (Zero Page)
    Opcode {
        mnemonic: "ISC",
        addr_mode: AddressingMode::ZeroPage,
        base_cycles: 5,
        handler: Cpu::isc,
    },
    // 0xE8 - INX (Implied)
    Opcode {
        mnemonic: "INX",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::inx,
    },
    // 0xE9 - SBC (Immediate)
    Opcode {
        mnemonic: "SBC",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::sbc,
    },
    // 0xEA - NOP (Implied)
    Opcode {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::nop,
    },
    // 0xEB - SBC (Immediate)
    Opcode {
        mnemonic: "SBC",
        addr_mode: AddressingMode::Immediate,
        base_cycles: 2,
        handler: Cpu::sbc,
    },
    // 0xEC - CPX (Absolute)
    Opcode {
        mnemonic: "CPX",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::cpx,
    },
    // 0xED - SBC (Absolute)
    Opcode {
        mnemonic: "SBC",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 4,
        handler: Cpu::sbc,
    },
    // 0xEE - INC (Absolute)
    Opcode {
        mnemonic: "INC",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::inc,
    },
    // 0xEF - ISC (Absolute)
    Opcode {
        mnemonic: "ISC",
        addr_mode: AddressingMode::Absolute,
        base_cycles: 6,
        handler: Cpu::isc,
    },
    // 0xF0 - BEQ (Relative)
    Opcode {
        mnemonic: "BEQ",
        addr_mode: AddressingMode::Relative,
        base_cycles: 2,
        handler: Cpu::beq,
    },
    // 0xF1 - SBC (Indirect,Y)
    Opcode {
        mnemonic: "SBC",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 5,
        handler: Cpu::sbc,
    },
    // 0xF2 - KIL (Implied)
    Opcode {
        mnemonic: "KIL",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::jam,
    },
    // 0xF3 - ISC (Indirect,Y)
    Opcode {
        mnemonic: "ISC",
        addr_mode: AddressingMode::IndirectY,
        base_cycles: 8,
        handler: Cpu::isc,
    },
    // 0xF4 - IGN (Zero Page,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0xF5 - SBC (Zero Page,X)
    Opcode {
        mnemonic: "SBC",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 4,
        handler: Cpu::sbc,
    },
    // 0xF6 - INC (Zero Page,X)
    Opcode {
        mnemonic: "INC",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::inc,
    },
    // 0xF7 - ISC (Zero Page,X)
    Opcode {
        mnemonic: "ISC",
        addr_mode: AddressingMode::ZeroPageX,
        base_cycles: 6,
        handler: Cpu::isc,
    },
    // 0xF8 - SED (Implied)
    Opcode {
        mnemonic: "SED",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::sed,
    },
    // 0xF9 - SBC (Absolute,Y)
    Opcode {
        mnemonic: "SBC",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 4,
        handler: Cpu::sbc,
    },
    // 0xFA - NOP (Implied)
    Opcode {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        base_cycles: 2,
        handler: Cpu::dummy,
    },
    // 0xFB - ISC (Absolute,Y)
    Opcode {
        mnemonic: "ISC",
        addr_mode: AddressingMode::AbsoluteY,
        base_cycles: 7,
        handler: Cpu::isc,
    },
    // 0xFC - IGN (Absolute,X)
    Opcode {
        mnemonic: "IGN",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::ign,
    },
    // 0xFD - SBC (Absolute,X)
    Opcode {
        mnemonic: "SBC",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 4,
        handler: Cpu::sbc,
    },
    // 0xFE - INC (Absolute,X)
    Opcode {
        mnemonic: "INC",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::inc,
    },
    // 0xFF - ISC (Absolute,X)
    Opcode {
        mnemonic: "ISC",
        addr_mode: AddressingMode::AbsoluteX,
        base_cycles: 7,
        handler: Cpu::isc,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock bus for testing
    struct MockBus {
        memory: [u8; 65536],
        nmi: bool,
    }

    impl MockBus {
        fn new() -> Self {
            Self {
                memory: [0; 65536],
                nmi: false,
            }
        }

        /// Raise the NMI line, the interrupt that restarts a halted CPU.
        fn set_nmi(&mut self, pending: bool) {
            self.nmi = pending;
        }

        fn write_mem(&mut self, addr: u16, value: u8) {
            self.memory[addr as usize] = value;
        }

        fn load_program(&mut self, start: u16, program: &[u8]) {
            for (i, &byte) in program.iter().enumerate() {
                self.memory[(start as usize) + i] = byte;
            }
        }
    }

    impl Bus for MockBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.memory[addr as usize]
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.memory[addr as usize] = value;
        }

        fn on_cpu_cycle(&mut self, _cycles: u8) {}

        fn nmi_pending(&self) -> bool {
            self.nmi
        }

        fn clear_nmi(&mut self) {
            self.nmi = false;
        }

        fn irq_pending(&self) -> bool {
            false
        }
    }

    #[test]
    fn test_lda_immediate() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        // LDA #$42
        bus.load_program(0x0600, &[0xA9, 0x42]);
        cpu.pc = 0x0600;

        let cycles = cpu.execute_instruction(&mut bus, 0xA9);

        assert_eq!(cpu.a, 0x42);
        assert!(!cpu.status.contains(StatusFlags::Z));
        assert!(!cpu.status.contains(StatusFlags::N));
        assert_eq!(cycles, 2);
    }

    #[test]
    fn test_lda_zero_flag() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        // LDA #$00
        bus.load_program(0x0600, &[0xA9, 0x00]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0xA9);

        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(StatusFlags::Z));
        assert!(!cpu.status.contains(StatusFlags::N));
    }

    #[test]
    fn test_lda_negative_flag() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        // LDA #$80
        bus.load_program(0x0600, &[0xA9, 0x80]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0xA9);

        assert_eq!(cpu.a, 0x80);
        assert!(!cpu.status.contains(StatusFlags::Z));
        assert!(cpu.status.contains(StatusFlags::N));
    }

    #[test]
    fn test_sta_absolute() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x42;
        // STA $0200
        bus.load_program(0x0600, &[0x8D, 0x00, 0x02]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x8D);

        assert_eq!(bus.memory[0x0200], 0x42);
    }

    #[test]
    fn test_tax() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x42;
        // TAX
        bus.load_program(0x0600, &[0xAA]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0xAA);

        assert_eq!(cpu.x, 0x42);
        assert!(!cpu.status.contains(StatusFlags::Z));
        assert!(!cpu.status.contains(StatusFlags::N));
    }

    #[test]
    fn test_adc() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x10;
        // ADC #$20
        bus.load_program(0x0600, &[0x69, 0x20]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x69);

        assert_eq!(cpu.a, 0x30);
        assert!(!cpu.status.contains(StatusFlags::C));
    }

    #[test]
    fn test_adc_with_carry() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x10;
        cpu.status.insert(StatusFlags::C);
        // ADC #$20
        bus.load_program(0x0600, &[0x69, 0x20]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x69);

        assert_eq!(cpu.a, 0x31);
    }

    #[test]
    fn test_adc_overflow() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        // Adding two positive numbers gives negative result
        cpu.a = 0x50; // +80
                      // ADC #$50
        bus.load_program(0x0600, &[0x69, 0x50]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x69);

        assert_eq!(cpu.a, 0xA0); // -96 (overflow!)
        assert!(cpu.status.contains(StatusFlags::V));
    }

    #[test]
    fn test_sbc() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x50;
        cpu.status.insert(StatusFlags::C); // No borrow
                                           // SBC #$20
        bus.load_program(0x0600, &[0xE9, 0x20]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0xE9);

        assert_eq!(cpu.a, 0x30);
        assert!(cpu.status.contains(StatusFlags::C)); // No borrow needed
    }

    #[test]
    fn test_and() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0xF0;
        // AND #$0F
        bus.load_program(0x0600, &[0x29, 0x0F]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x29);

        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(StatusFlags::Z));
    }

    #[test]
    fn test_ora() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0xF0;
        // ORA #$0F
        bus.load_program(0x0600, &[0x09, 0x0F]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x09);

        assert_eq!(cpu.a, 0xFF);
        assert!(cpu.status.contains(StatusFlags::N));
    }

    #[test]
    fn test_eor() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0xFF;
        // EOR #$0F
        bus.load_program(0x0600, &[0x49, 0x0F]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x49);

        assert_eq!(cpu.a, 0xF0);
    }

    #[test]
    fn test_asl_accumulator() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x81;
        // ASL A
        bus.load_program(0x0600, &[0x0A]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x0A);

        assert_eq!(cpu.a, 0x02);
        assert!(cpu.status.contains(StatusFlags::C));
    }

    #[test]
    fn test_lsr_accumulator() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x81;
        // LSR A
        bus.load_program(0x0600, &[0x4A]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x4A);

        assert_eq!(cpu.a, 0x40);
        assert!(cpu.status.contains(StatusFlags::C));
    }

    #[test]
    fn test_rol_accumulator() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x80;
        cpu.status.insert(StatusFlags::C);
        // ROL A
        bus.load_program(0x0600, &[0x2A]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x2A);

        assert_eq!(cpu.a, 0x01);
        assert!(cpu.status.contains(StatusFlags::C));
    }

    #[test]
    fn test_ror_accumulator() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x01;
        cpu.status.insert(StatusFlags::C);
        // ROR A
        bus.load_program(0x0600, &[0x6A]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x6A);

        assert_eq!(cpu.a, 0x80);
        assert!(cpu.status.contains(StatusFlags::C));
    }

    #[test]
    fn test_cmp() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x50;
        // CMP #$30
        bus.load_program(0x0600, &[0xC9, 0x30]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0xC9);

        assert!(cpu.status.contains(StatusFlags::C)); // A >= M
        assert!(!cpu.status.contains(StatusFlags::Z)); // A != M
    }

    #[test]
    fn test_inx() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.x = 0x7F;
        // INX
        bus.load_program(0x0600, &[0xE8]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0xE8);

        assert_eq!(cpu.x, 0x80);
        assert!(cpu.status.contains(StatusFlags::N));
    }

    #[test]
    fn test_dex() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.x = 0x00;
        // DEX
        bus.load_program(0x0600, &[0xCA]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0xCA);

        assert_eq!(cpu.x, 0xFF);
        assert!(cpu.status.contains(StatusFlags::N));
    }

    #[test]
    fn test_pha_pla() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0x42;
        cpu.sp = 0xFF;
        // PHA
        bus.load_program(0x0600, &[0x48]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x48);

        assert_eq!(cpu.sp, 0xFE);
        assert_eq!(bus.memory[0x01FF], 0x42);

        cpu.a = 0x00;
        // PLA
        bus.load_program(0x0601, &[0x68]);
        cpu.pc = 0x0601;

        cpu.execute_instruction(&mut bus, 0x68);

        assert_eq!(cpu.a, 0x42);
        assert_eq!(cpu.sp, 0xFF);
    }

    #[test]
    fn test_php_plp() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.status = StatusFlags::N | StatusFlags::C;
        cpu.sp = 0xFF;
        // PHP
        bus.load_program(0x0600, &[0x08]);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x08);

        // PHP pushes with B and U flags set
        assert_eq!(bus.memory[0x01FF], 0xB1); // N=1, U=1, B=1, C=1

        cpu.status = StatusFlags::empty();
        // PLP
        bus.load_program(0x0601, &[0x28]);
        cpu.pc = 0x0601;

        cpu.execute_instruction(&mut bus, 0x28);

        // PLP restores flags but B is cleared and U is set
        assert!(cpu.status.contains(StatusFlags::N));
        assert!(cpu.status.contains(StatusFlags::C));
        assert!(!cpu.status.contains(StatusFlags::B));
        assert!(cpu.status.contains(StatusFlags::U));
    }

    #[test]
    fn test_beq_branch_taken() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.status.insert(StatusFlags::Z);
        cpu.pc = 0x0600;
        // BEQ +5
        bus.load_program(0x0600, &[0xF0, 0x05]);

        cpu.execute_instruction(&mut bus, 0xF0);

        // PC should be 0x0602 + 5 = 0x0607
        assert_eq!(cpu.pc, 0x0607);
    }

    #[test]
    fn test_beq_branch_not_taken() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.status.remove(StatusFlags::Z);
        cpu.pc = 0x0600;
        // BEQ +5
        bus.load_program(0x0600, &[0xF0, 0x05]);

        cpu.execute_instruction(&mut bus, 0xF0);

        // PC should be 0x0602 (next instruction)
        assert_eq!(cpu.pc, 0x0602);
    }

    #[test]
    fn test_jmp_absolute() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.pc = 0x0600;
        // JMP $1234
        bus.load_program(0x0600, &[0x4C, 0x34, 0x12]);

        cpu.execute_instruction(&mut bus, 0x4C);

        assert_eq!(cpu.pc, 0x1234);
    }

    #[test]
    fn test_jsr_rts() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.sp = 0xFF;
        cpu.pc = 0x0600;
        // JSR $1234
        bus.load_program(0x0600, &[0x20, 0x34, 0x12]);

        cpu.execute_instruction(&mut bus, 0x20);

        // PC should be 0x1234
        assert_eq!(cpu.pc, 0x1234);
        // Return address (PC - 1 = 0x0602) should be on stack
        assert_eq!(bus.memory[0x01FF], 0x06);
        assert_eq!(bus.memory[0x01FE], 0x02);
        assert_eq!(cpu.sp, 0xFD);

        // RTS
        bus.load_program(0x1234, &[0x60]);
        cpu.pc = 0x1234;

        cpu.execute_instruction(&mut bus, 0x60);

        // PC should be 0x0603 (return address + 1)
        assert_eq!(cpu.pc, 0x0603);
        assert_eq!(cpu.sp, 0xFF);
    }

    #[test]
    fn test_nop() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.pc = 0x0600;
        // NOP
        bus.load_program(0x0600, &[0xEA]);

        let cycles = cpu.execute_instruction(&mut bus, 0xEA);

        assert_eq!(cycles, 2);
        assert_eq!(cpu.pc, 0x0601);
    }

    #[test]
    fn test_bit() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.a = 0xFF;
        bus.write_mem(0x0200, 0xC0); // Bits 7 and 6 set
        cpu.pc = 0x0600;
        // BIT $0200
        bus.load_program(0x0600, &[0x2C, 0x00, 0x02]);

        cpu.execute_instruction(&mut bus, 0x2C);

        assert!(cpu.status.contains(StatusFlags::N));
        assert!(cpu.status.contains(StatusFlags::V));
        assert!(!cpu.status.contains(StatusFlags::Z));
    }

    #[test]
    fn test_inc_memory() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        bus.write_mem(0x0200, 0x7F);
        cpu.pc = 0x0600;
        // INC $0200
        bus.load_program(0x0600, &[0xEE, 0x00, 0x02]);

        cpu.execute_instruction(&mut bus, 0xEE);

        assert_eq!(bus.memory[0x0200], 0x80);
        assert!(cpu.status.contains(StatusFlags::N));
    }

    #[test]
    fn test_dec_memory() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        bus.write_mem(0x0200, 0x80);
        cpu.pc = 0x0600;
        // DEC $0200
        bus.load_program(0x0600, &[0xCE, 0x00, 0x02]);

        cpu.execute_instruction(&mut bus, 0xCE);

        assert_eq!(bus.memory[0x0200], 0x7F);
        assert!(!cpu.status.contains(StatusFlags::N));
    }

    #[test]
    fn test_clc_sec() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.pc = 0x0600;
        // SEC
        bus.load_program(0x0600, &[0x38]);

        cpu.execute_instruction(&mut bus, 0x38);
        assert!(cpu.status.contains(StatusFlags::C));

        // CLC
        bus.load_program(0x0601, &[0x18]);
        cpu.pc = 0x0601;

        cpu.execute_instruction(&mut bus, 0x18);
        assert!(!cpu.status.contains(StatusFlags::C));
    }

    #[test]
    fn test_cli_sei() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.pc = 0x0600;
        // SEI
        bus.load_program(0x0600, &[0x78]);

        cpu.execute_instruction(&mut bus, 0x78);
        assert!(cpu.status.contains(StatusFlags::I));

        // CLI
        bus.load_program(0x0601, &[0x58]);
        cpu.pc = 0x0601;

        cpu.execute_instruction(&mut bus, 0x58);
        assert!(!cpu.status.contains(StatusFlags::I));
    }

    #[test]
    fn test_clv() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.status.insert(StatusFlags::V);
        cpu.pc = 0x0600;
        // CLV
        bus.load_program(0x0600, &[0xB8]);

        cpu.execute_instruction(&mut bus, 0xB8);

        assert!(!cpu.status.contains(StatusFlags::V));
    }

    #[test]
    fn test_brk() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();

        cpu.pc = 0x0600;
        cpu.sp = 0xFF;
        cpu.status = StatusFlags::C;

        // Set up IRQ vector
        bus.write_mem(0xFFFE, 0x34);
        bus.write_mem(0xFFFF, 0x12);

        // BRK
        bus.load_program(0x0600, &[0x00]);

        cpu.execute_instruction(&mut bus, 0x00);

        // PC should be at IRQ vector
        assert_eq!(cpu.pc, 0x1234);
        // I flag should be set
        assert!(cpu.status.contains(StatusFlags::I));
        // Return address (PC + 2 = 0x0602) should be on stack
        assert_eq!(bus.memory[0x01FF], 0x06);
        assert_eq!(bus.memory[0x01FE], 0x02);
        // Status with B flag should be on stack
        assert_eq!(bus.memory[0x01FD], 0x31); // C=1, I=0, U=1, B=1
    }

    /// The twelve KIL opcodes hang the CPU. They must not take the host down
    /// with them: on `wasm32` a panic aborts the whole instance.
    #[test]
    fn test_kil_halts_the_cpu_without_panicking() {
        const KIL: [u8; 12] = [
            0x02, 0x12, 0x22, 0x32, 0x42, 0x52, 0x62, 0x72, 0x92, 0xB2, 0xD2, 0xF2,
        ];

        for opcode in KIL {
            let mut cpu = Cpu::new();
            let mut bus = MockBus::new();
            bus.load_program(0x0600, &[opcode, opcode, opcode]);
            cpu.pc = 0x0600;

            assert_eq!(cpu.execute_instruction(&mut bus, opcode), 2);
            assert!(cpu.is_halted(), "{opcode:#04X} should halt the CPU");

            // A halted CPU burns cycles but fetches nothing: the PC stays put
            // and the machine keeps ticking around it.
            let pc = cpu.pc();
            assert_eq!(cpu.execute_instruction(&mut bus, 0xEA), 2);
            assert_eq!(
                cpu.pc(),
                pc,
                "{opcode:#04X} must not advance the PC while halted"
            );
            assert!(cpu.is_halted());
        }
    }

    #[test]
    fn test_nmi_resumes_a_halted_cpu() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x02]);
        bus.write_mem(0xFFFA, 0x00);
        bus.write_mem(0xFFFB, 0x90); // NMI vector -> $9000
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x02);
        assert!(cpu.is_halted());

        // The interrupt is what brings it back, exactly as on hardware.
        bus.set_nmi(true);
        assert_eq!(cpu.execute_instruction(&mut bus, 0xEA), 7);
        assert!(!cpu.is_halted());
        assert_eq!(cpu.pc(), 0x9000);
        assert!(!bus.nmi_pending(), "taking the NMI acknowledges it");
    }

    #[test]
    fn test_reset_clears_the_halt() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.write_mem(0xFFFC, 0x00);
        bus.write_mem(0xFFFD, 0x80);
        cpu.pc = 0x0600;

        cpu.execute_instruction(&mut bus, 0x02);
        assert!(cpu.is_halted());

        cpu.reset(&mut bus);
        assert!(!cpu.is_halted());
        assert_eq!(cpu.pc(), 0x8000);
    }

    /// A page-crossed indexed read pays its extra cycle exactly once.
    ///
    /// The addressing mode reports the crossing and the handler charges for it;
    /// adding the reported penalty again in `execute_instruction` made every
    /// cross-page `LDA abs,X` cost six cycles instead of five (blargg's
    /// `instr_timing` catches this).
    #[test]
    fn test_page_cross_penalty_is_charged_once() {
        /// Runs one instruction from $0600 with the given X.
        fn cycles_of(program: &[u8], x: u8) -> u8 {
            let mut cpu = Cpu::new();
            let mut bus = MockBus::new();
            bus.load_program(0x0600, program);
            cpu.pc = 0x0600;
            cpu.x = x;
            cpu.execute_instruction(&mut bus, program[0])
        }

        // LDA $12F0,X with X = $20 lands in $1310: 4 cycles + the crossing.
        assert_eq!(cycles_of(&[0xBD, 0xF0, 0x12], 0x20), 5);
        // The same read inside one page takes four.
        assert_eq!(cycles_of(&[0xBD, 0xF0, 0x12], 0x00), 4);
        // A read-modify-write never pays it: ASL abs,X is 7 either way.
        assert_eq!(cycles_of(&[0x1E, 0xF0, 0x12], 0x20), 7);
        assert_eq!(cycles_of(&[0x1E, 0xF0, 0x12], 0x00), 7);
        // Nor does a store: STA abs,X is 5 either way.
        assert_eq!(cycles_of(&[0x9D, 0xF0, 0x12], 0x20), 5);

        // (ind),Y reads are the other place it applies: the pointer at $10
        // holds $12F0, so Y = $20 lands in $1310.
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0xB1, 0x10]);
        bus.write_mem(0x0010, 0xF0);
        bus.write_mem(0x0011, 0x12);
        bus.write_mem(0x1310, 0x42);
        cpu.pc = 0x0600;
        cpu.y = 0x20;
        assert_eq!(cpu.execute_instruction(&mut bus, 0xB1), 6);
        assert_eq!(cpu.a, 0x42);
    }

    /// $0C is the undocumented 3-byte NOP; it used to be "invalid".
    #[test]
    fn test_undocumented_nop_absolute() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x0C, 0x34, 0x12]);
        cpu.pc = 0x0600;

        assert_eq!(cpu.execute_instruction(&mut bus, 0x0C), 4);
        assert_eq!(cpu.pc, 0x0603);
        assert!(!cpu.is_halted());
    }

    /// Every byte value must have a real handler: the documented set, the
    /// undocumented instructions the 2A03 executes, and the KILs. A sweep over
    /// all 256 opcodes catches a table entry that was left "invalid" and makes
    /// the "no opcode panics" guarantee explicit.
    #[test]
    fn test_every_opcode_executes() {
        const KIL: [u8; 12] = [
            0x02, 0x12, 0x22, 0x32, 0x42, 0x52, 0x62, 0x72, 0x92, 0xB2, 0xD2, 0xF2,
        ];

        for opcode in 0u8..=0xFF {
            for flags in [
                StatusFlags::from_bits_truncate(0x24),
                StatusFlags::C | StatusFlags::V,
            ] {
                let mut cpu = Cpu::new();
                let mut bus = MockBus::new();
                // A program of interest: the opcode followed by operands, with
                // the rest of memory holding a pattern that makes a stray fetch
                // or store visible rather than silent.
                for (addr, slot) in bus.memory.iter_mut().enumerate() {
                    *slot = (addr as u8).wrapping_mul(0x2B);
                }
                bus.load_program(0x0600, &[opcode, 0x00, 0x02, 0x04, 0x06]);
                cpu.pc = 0x0600;
                cpu.a = 0x55;
                cpu.x = 0x1F;
                cpu.y = 0xF0;
                cpu.sp = 0xFD;
                cpu.status = flags;

                let cycles = cpu.execute_instruction(&mut bus, opcode);

                assert!(
                    (2..=8).contains(&cycles),
                    "{opcode:#04X} returned {cycles} cycles"
                );
                assert_eq!(
                    cpu.is_halted(),
                    KIL.contains(&opcode),
                    "{opcode:#04X} halted when it should not (or the reverse)"
                );
            }
        }
    }

    /// The undocumented instructions are combinations of documented ones; these
    /// pin a representative sample so a refactor cannot quietly change them.
    #[test]
    fn test_undocumented_instruction_semantics() {
        /// Runs `opcode` against a zero page operand and returns the machine.
        fn run(
            opcode: u8,
            operand: u8,
            setup: impl FnOnce(&mut Cpu),
            value: u8,
        ) -> (Cpu, MockBus, u8) {
            let mut cpu = Cpu::new();
            let mut bus = MockBus::new();
            bus.load_program(0x0600, &[opcode, operand]);
            bus.write_mem(operand as u16, value);
            cpu.pc = 0x0600;
            setup(&mut cpu);
            let cycles = cpu.execute_instruction(&mut bus, opcode);
            (cpu, bus, cycles)
        }

        /// Runs an immediate-mode opcode with `A` primed.
        fn run_immediate(opcode: u8, immediate: u8, a: u8) -> (Cpu, u8) {
            let mut cpu = Cpu::new();
            let mut bus = MockBus::new();
            bus.load_program(0x0600, &[opcode, immediate]);
            cpu.pc = 0x0600;
            cpu.a = a;
            let cycles = cpu.execute_instruction(&mut bus, opcode);
            (cpu, cycles)
        }

        // LAX $10 - load A and X together
        let (cpu, _, _) = run(0xA7, 0x10, |_| {}, 0x80);
        assert_eq!((cpu.a, cpu.x), (0x80, 0x80));
        assert!(cpu.status.contains(StatusFlags::N));

        // SAX $10 - store A AND X
        let (_, bus, _) = run(
            0x87,
            0x10,
            |cpu| {
                cpu.a = 0xF0;
                cpu.x = 0xFF;
            },
            0x00,
        );
        assert_eq!(bus.memory[0x10], 0xF0);

        // DCP $10 - decrement memory then compare with A
        let (cpu, bus, _) = run(0xC7, 0x10, |cpu| cpu.a = 0x40, 0x41);
        assert_eq!(bus.memory[0x10], 0x40);
        assert!(
            cpu.status.contains(StatusFlags::Z),
            "A == memory after the decrement"
        );

        // ISC $10 - increment memory then subtract it from A
        let (cpu, bus, _) = run(
            0xE7,
            0x10,
            |cpu| {
                cpu.a = 0x10;
                cpu.status.insert(StatusFlags::C);
            },
            0x04,
        );
        assert_eq!(bus.memory[0x10], 0x05);
        assert_eq!(cpu.a, 0x0B);

        // SLO $10 - shift memory left, then ORA into A
        let (cpu, bus, _) = run(0x07, 0x10, |cpu| cpu.a = 0x01, 0x81);
        assert_eq!(bus.memory[0x10], 0x02);
        assert_eq!(cpu.a, 0x03);
        assert!(cpu.status.contains(StatusFlags::C));

        // RLA $10 - rotate memory left, then AND into A
        let (cpu, bus, _) = run(
            0x27,
            0x10,
            |cpu| {
                cpu.a = 0xFF;
                cpu.status.remove(StatusFlags::C);
            },
            0x81,
        );
        assert_eq!(bus.memory[0x10], 0x02);
        assert_eq!(cpu.a, 0x02);
        assert!(cpu.status.contains(StatusFlags::C));

        // SRE $10 - shift memory right, then EOR into A
        let (cpu, bus, _) = run(0x47, 0x10, |cpu| cpu.a = 0xFF, 0x03);
        assert_eq!(bus.memory[0x10], 0x01);
        assert_eq!(cpu.a, 0xFE);
        assert!(cpu.status.contains(StatusFlags::C));

        // RRA $10 - rotate memory right, then ADC into A with the carry the
        // rotate just produced
        let (cpu, bus, _) = run(
            0x67,
            0x10,
            |cpu| {
                cpu.a = 0x00;
                cpu.status.insert(StatusFlags::C);
            },
            0x02,
        );
        assert_eq!(bus.memory[0x10], 0x81);
        assert_eq!(cpu.a, 0x81);

        // ANC #$80 - AND immediate, carry from bit 7
        let (cpu, _) = run_immediate(0x0B, 0x80, 0xFF);
        assert_eq!(cpu.a, 0x80);
        assert!(cpu.status.contains(StatusFlags::C));

        // ALR #$01 - AND immediate, then LSR
        let (cpu, _) = run_immediate(0x4B, 0x01, 0x03);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(StatusFlags::C));
        assert!(cpu.status.contains(StatusFlags::Z));

        // AXS #$10 - X = (A AND X) - immediate
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0xCB, 0x10]);
        cpu.pc = 0x0600;
        cpu.a = 0xFF;
        cpu.x = 0x20;
        cpu.execute_instruction(&mut bus, 0xCB);
        assert_eq!(cpu.x, 0x10);

        // LAE $1234,Y - A = X = SP = memory AND SP
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0xBB, 0x34, 0x12]);
        bus.write_mem(0x1234, 0xFF);
        cpu.pc = 0x0600;
        cpu.sp = 0x0F;
        cpu.y = 0x00;
        cpu.execute_instruction(&mut bus, 0xBB);
        assert_eq!((cpu.a, cpu.x, cpu.sp), (0x0F, 0x0F, 0x0F));

        // SHX $1234,Y - store X AND (high byte + 1)
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x9E, 0x34, 0x12]);
        cpu.pc = 0x0600;
        cpu.x = 0xFF;
        cpu.y = 0x00;
        cpu.execute_instruction(&mut bus, 0x9E);
        assert_eq!(bus.memory[0x1234], 0x13);

        // IGN $10 - reads the operand and ignores it
        let (cpu, _, cycles) = run(0x04, 0x10, |cpu| cpu.a = 0x7E, 0x00);
        assert_eq!(cycles, 3);
        assert_eq!(cpu.a, 0x7E);

        // SKB #$10 - an immediate NOP
        let (cpu, _, cycles) = run(0x80, 0x10, |cpu| cpu.a = 0x7E, 0x00);
        assert_eq!(cycles, 2);
        assert_eq!((cpu.a, cpu.pc), (0x7E, 0x0602));
    }
}
