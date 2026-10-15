//! CPU Instructions - 6502 processor instruction implementations
//!
//! This module implements all 56 official 6502 opcodes with proper
//! flag handling, cycle counting, and addressing mode integration.
//!
//! Reference: https://www.nesdev.org/wiki/CPU_instructions

use super::{Cpu, InterruptSource, StatusFlags};
use crate::Bus;

/// Where an instruction's handler touches the effective address the addressing
/// mode resolved.
///
/// The machine runs the handler on the cycle the chip performs the access on,
/// and it has to know that cycle *before* the handler runs: the plan is
/// expressed as "the access is on the instruction's last cycle" or "two cycles
/// before it", which is what the handler's own cycle count cannot tell until it
/// is too late.
/// Opcode table entry - defines instruction metadata
#[derive(Debug, Clone, Copy)]
pub struct Opcode {
    /// Instruction mnemonic
    pub mnemonic: &'static str,
    /// Addressing mode function
    pub addr_mode: AddressingMode,
    /// Cycle count the instruction should take, for the tests to check the
    /// accesses against.
    ///
    /// Mesen has no such table - its handlers spell out every cycle as an
    /// access or a dummy read, and the total is whatever that comes to. This
    /// one is a cross-check kept for `test_every_opcode_costs_what_the_table_
    /// says`; it is not what drives the machine.
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
    /// Absolute X for a *write* (Mesen's `AbsXW`): the discarded read the
    /// addressing does is unconditional, because a store has no page-crossing
    /// penalty to hide it in.
    AbsoluteXWrite,
    /// Absolute Y - 16-bit address + Y
    AbsoluteY,
    /// Absolute Y for a write (Mesen's `AbsYW`).
    AbsoluteYWrite,
    /// Indirect - pointer at 16-bit address (JMP only)
    Indirect,
    /// Indirect X - pointer at ZP + X
    IndirectX,
    /// Indirect Y - pointer at ZP, then + Y
    IndirectY,
    /// Indirect Y for a write (Mesen's `IndYW`).
    IndirectYWrite,
    /// Relative - branch offset (signed)
    Relative,
    /// Accumulator - operation on A register
    Accumulator,
    /// Implied - no operand
    Implied,
}

/// Instruction handler function type
///
/// The addressing mode has already fetched the operands and left the effective
/// address in `addr`; the handler performs the instruction's own accesses, each
/// of which is a CPU cycle.
pub type InstructionHandler = fn(&mut Cpu, &mut dyn Bus, u16);

impl Cpu {
    /// Execute one instruction whose opcode the caller has already fetched.
    ///
    /// This is Mesen's `Exec()` after `GetOPCode()`. There is no cycle table in
    /// the path: the addressing fetches its operands on their own cycles, the
    /// handler performs the instruction's own accesses (each of them a cycle),
    /// and the discarded reads the chip makes in between are written out where
    /// they happen. An instruction takes as long as its accesses say it does,
    /// which is what makes the timing something you can read off the code.
    ///
    /// The returned count is the instruction's whole duration — the caller's
    /// opcode fetch included — and is for the caller's bookkeeping and the
    /// tests; the machine has already been driven by the accesses themselves.
    pub fn execute_instruction(&mut self, bus: &mut dyn Bus, opcode: u8) -> u8 {
        let start = bus.cycle_count();

        // The caller's opcode fetch was a cycle, and its end-of-cycle
        // bookkeeping (the interrupt-line samples) belongs to this instruction:
        // Mesen's `GetOPCode` is a `MemoryRead`, which brackets the same pair.
        self.end_cycle(bus);

        // The program counter moves past the opcode byte
        // here, so the addressing's operand fetches start where Mesen's do
        // (`GetOPCode` is `MemoryRead(PC); PC++`).
        self.pc = self.pc.wrapping_add(1);

        // A KIL/JAM re-runs itself (see `Cpu::jam`), so it is an ordinary
        // instruction here: a fetch and a discarded read, over and over.
        let op = &OPCODE_TABLE[opcode as usize];

        self.addr_mode = op.addr_mode;
        let addr = self.fetch_operand(bus, op.addr_mode);
        (op.handler)(self, bus, addr);

        // "It's really the status of the interrupt lines at the end of the
        // second-to-last cycle that matters": the decision below reads samples
        // that are one cycle behind. That single rule is what gives CLI/SEI/PLP
        // their one-instruction delay, lets a taken branch shrug off an IRQ
        // raised during its last cycle, and keeps an NMI that arrives on an
        // instruction's last cycle waiting for the next one.
        if self.prev_run_irq || self.prev_need_nmi {
            self.take_interrupt(bus);
        }

        (bus.cycle_count() - start) as u8 + 1
    }

    /// Fetch the operands the addressing mode calls for — one CPU cycle per
    /// access — and return the effective address.
    ///
    /// Mesen's `FetchOperand`, discarded reads included. The zero-page indexed
    /// modes read the un-indexed address first; the indexed modes read the
    /// address the index has *not* carried into (always for a write, and only
    /// on a carry for a read); the accumulator and implied modes read the byte
    /// after the opcode and throw it away. Those reads are the instruction's
    /// cycles, so what is left over is nothing at all.
    fn fetch_operand(&mut self, bus: &mut dyn Bus, mode: AddressingMode) -> u16 {
        match mode {
            AddressingMode::Immediate => {
                // The handler's read of this address is the operand fetch, on
                // the same cycle it would take inside the addressing.
                let addr = self.pc;
                self.pc = self.pc.wrapping_add(1);
                addr
            }
            AddressingMode::ZeroPage => self.read_byte(bus) as u16,
            AddressingMode::ZeroPageX => {
                let base = self.read_byte(bus);
                self.read(bus, base as u16);
                base.wrapping_add(self.x) as u16
            }
            AddressingMode::ZeroPageY => {
                let base = self.read_byte(bus);
                self.read(bus, base as u16);
                base.wrapping_add(self.y) as u16
            }
            AddressingMode::Absolute => self.read_word(bus),
            AddressingMode::AbsoluteX | AddressingMode::AbsoluteXWrite => {
                let base = self.read_word(bus);
                self.addr_base = base;
                let addr = base.wrapping_add(self.x as u16);
                self.indexed_dummy(bus, base, addr, mode == AddressingMode::AbsoluteXWrite);
                addr
            }
            AddressingMode::AbsoluteY | AddressingMode::AbsoluteYWrite => {
                let base = self.read_word(bus);
                self.addr_base = base;
                let addr = base.wrapping_add(self.y as u16);
                self.indexed_dummy(bus, base, addr, mode == AddressingMode::AbsoluteYWrite);
                addr
            }
            AddressingMode::Indirect => {
                // JMP ($xxxx): the high byte of the target comes from the same
                // page as the low byte of the pointer, which is the chip's
                // page-wrap bug.
                let ptr = self.read_word(bus);
                let low = self.read(bus, ptr) as u16;
                let high_addr = (ptr & 0xFF00) | (ptr.wrapping_add(1) & 0x00FF);
                let high = self.read(bus, high_addr) as u16;
                (high << 8) | low
            }
            AddressingMode::IndirectX => {
                let base = self.read_byte(bus);
                self.read(bus, base as u16);
                let ptr = base.wrapping_add(self.x);
                let low = self.read(bus, ptr as u16) as u16;
                let high = self.read(bus, ptr.wrapping_add(1) as u16) as u16;
                (high << 8) | low
            }
            AddressingMode::IndirectY | AddressingMode::IndirectYWrite => {
                let ptr = self.read_byte(bus);
                let low = self.read(bus, ptr as u16) as u16;
                let high = self.read(bus, ptr.wrapping_add(1) as u16) as u16;
                let base = (high << 8) | low;
                self.addr_base = base;
                let addr = base.wrapping_add(self.y as u16);
                self.indexed_dummy(bus, base, addr, mode == AddressingMode::IndirectYWrite);
                addr
            }
            AddressingMode::Relative => {
                let offset = self.read_byte(bus) as i8;
                self.pc.wrapping_add(offset as u16)
            }
            AddressingMode::Accumulator => {
                // The accumulator modes still read: the byte after the opcode,
                // discarded. `0xFFFF` is the sentinel the handlers know.
                self.dummy_read(bus);
                0xFFFF
            }
            AddressingMode::Implied => {
                self.dummy_read(bus);
                0
            }
        }
    }

    /// The discarded read an indexed address makes.
    ///
    /// A read only makes it when the index carried into the high byte: it reads
    /// the low byte of the sum in the base's page, which is where the chip's
    /// address adder had got to. A write always makes it, at the same address,
    /// because a store has no other cycle to spend.
    fn indexed_dummy(&mut self, bus: &mut dyn Bus, base: u16, addr: u16, always: bool) {
        let carried = (base & 0xFF00) != (addr & 0xFF00);
        if carried || always {
            let wrong = (base & 0xFF00) | (addr & 0x00FF);
            self.read(bus, wrong);
        }
    }

    /// Take the interrupt the lines called for once an instruction is over.
    ///
    /// Mesen's `IRQ()`: two discarded reads, the return address, then the
    /// status byte — and at that point an NMI that has arrived in the meantime
    /// takes the vector over, with the frame already pushed staying the IRQ's
    /// (B clear). Seven cycles, every one of them an access.
    fn take_interrupt(&mut self, bus: &mut dyn Bus) {
        self.halted = false;

        self.dummy_read(bus);
        self.dummy_read(bus);
        self.push_word(bus, self.pc);

        // The NMI check happens where the chip makes it: the return address is
        // already on the stack, the status byte is not.
        let source = if self.need_nmi {
            self.need_nmi = false;
            InterruptSource::Nmi
        } else {
            InterruptSource::Irq
        };

        self.push(bus, self.status.bits() | StatusFlags::U.bits());
        self.status.insert(StatusFlags::I);
        self.pc = Self::read_vector(bus, source);
    }

    /// Push two bytes, high first.
    fn push_word(&mut self, bus: &mut dyn Bus, value: u16) {
        self.push(bus, (value >> 8) as u8);
        self.push(bus, (value & 0xFF) as u8);
    }

    /// Read an interrupt vector's two bytes.
    fn read_vector(bus: &mut dyn Bus, source: InterruptSource) -> u16 {
        let vector = Self::interrupt_vector(source);
        (bus.read_cycle(vector) as u16) | ((bus.read_cycle(vector.wrapping_add(1)) as u16) << 8)
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
    fn lda(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = if addr == 0xFFFF {
            // Accumulator mode - shouldn't happen for LDA
            self.a
        } else {
            self.read(bus, addr)
        };
        self.a = value;
        self.update_nz_flags(value);
    }

    /// LDX - Load X Register
    fn ldx(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = if addr == 0xFFFF {
            self.x
        } else {
            self.read(bus, addr)
        };
        self.x = value;
        self.update_nz_flags(value);
    }

    /// LDY - Load Y Register
    fn ldy(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = if addr == 0xFFFF {
            self.y
        } else {
            self.read(bus, addr)
        };
        self.y = value;
        self.update_nz_flags(value);
    }

    /// STA - Store Accumulator
    fn sta(&mut self, bus: &mut dyn Bus, addr: u16) {
        // Stores always do dummy read on indexed modes even without page cross
        self.write(bus, addr, self.a);
    }

    /// STX - Store X Register
    fn stx(&mut self, bus: &mut dyn Bus, addr: u16) {
        self.write(bus, addr, self.x);
    }

    /// STY - Store Y Register
    fn sty(&mut self, bus: &mut dyn Bus, addr: u16) {
        self.write(bus, addr, self.y);
    }

    // ========================================================================
    // Register Transfer Instructions
    // ========================================================================

    /// TAX - Transfer Accumulator to X
    fn tax(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.x = self.a;
        self.update_nz_flags(self.x);
    }

    /// TAY - Transfer Accumulator to Y
    fn tay(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.y = self.a;
        self.update_nz_flags(self.y);
    }

    /// TXA - Transfer X to Accumulator
    fn txa(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.a = self.x;
        self.update_nz_flags(self.a);
    }

    /// TYA - Transfer Y to Accumulator
    fn tya(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.a = self.y;
        self.update_nz_flags(self.a);
    }

    /// TSX - Transfer Stack Pointer to X
    fn tsx(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.x = self.sp;
        self.update_nz_flags(self.x);
    }

    /// TXS - Transfer X to Stack Pointer
    fn txs(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.sp = self.x;
        // TXS does not affect flags
    }

    // ========================================================================
    // Stack Operations
    // ========================================================================

    /// Push a byte onto the stack
    fn push(&mut self, bus: &mut dyn Bus, value: u8) {
        let addr = 0x0100 | (self.sp as u16);
        self.write(bus, addr, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pull a byte from the stack
    fn pull(&mut self, bus: &mut dyn Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        let addr = 0x0100 | (self.sp as u16);
        self.read(bus, addr)
    }

    /// PHA - Push Accumulator
    fn pha(&mut self, bus: &mut dyn Bus, _addr: u16) {
        self.push(bus, self.a);
    }

    /// PHP - Push Processor Status
    fn php(&mut self, bus: &mut dyn Bus, _addr: u16) {
        // PHP always sets B flag when pushing
        let status = self.status | StatusFlags::B | StatusFlags::U;
        self.push(bus, status.bits());
    }

    /// PLA - Pull Accumulator
    fn pla(&mut self, bus: &mut dyn Bus, _addr: u16) {
        self.dummy_read(bus);
        self.a = self.pull(bus);
        self.update_nz_flags(self.a);
    }

    /// PLP - Pull Processor Status
    fn plp(&mut self, bus: &mut dyn Bus, _addr: u16) {
        self.dummy_read(bus);
        let value = self.pull(bus);
        // B flag is not restored from stack; U flag is always set
        self.status = StatusFlags::from_bits_truncate(value);
        self.status.remove(StatusFlags::B);
        self.status.insert(StatusFlags::U);
    }

    // ========================================================================
    // Logical Instructions
    // ========================================================================

    /// AND - Logical AND
    fn and(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        self.a &= value;
        self.update_nz_flags(self.a);
    }

    /// EOR - Exclusive OR
    fn eor(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        self.a ^= value;
        self.update_nz_flags(self.a);
    }

    /// ORA - Logical OR
    fn ora(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        self.a |= value;
        self.update_nz_flags(self.a);
    }

    /// BIT - Bit Test
    fn bit(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        self.status.set(StatusFlags::N, value & 0x80 != 0);
        self.status.set(StatusFlags::V, value & 0x40 != 0);
        self.status.set(StatusFlags::Z, (self.a & value) == 0);
    }

    // ========================================================================
    // Arithmetic Instructions
    // ========================================================================

    /// ADC - Add with Carry
    fn adc(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
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
    }

    /// SBC - Subtract with Carry
    fn sbc(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
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
    }

    /// CMP - Compare Accumulator
    fn cmp(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        let diff = self.a.wrapping_sub(value);
        self.status.set(StatusFlags::C, self.a >= value);
        self.update_nz_flags(diff);
    }

    /// CPX - Compare X Register
    fn cpx(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        let diff = self.x.wrapping_sub(value);
        self.status.set(StatusFlags::C, self.x >= value);
        self.update_nz_flags(diff);
    }

    /// CPY - Compare Y Register
    fn cpy(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        let diff = self.y.wrapping_sub(value);
        self.status.set(StatusFlags::C, self.y >= value);
        self.update_nz_flags(diff);
    }

    // ========================================================================
    // Increment/Decrement Instructions
    // ========================================================================

    /// INC - Increment Memory
    fn inc(&mut self, bus: &mut dyn Bus, addr: u16) {
        // RMW pattern: read, write original, write modified
        let value = self.read(bus, addr);
        self.write(bus, addr, value); // dummy write
        let result = value.wrapping_add(1);
        self.write(bus, addr, result);
        self.update_nz_flags(result);
    }

    /// INX - Increment X
    fn inx(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.x = self.x.wrapping_add(1);
        self.update_nz_flags(self.x);
    }

    /// INY - Increment Y
    fn iny(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.y = self.y.wrapping_add(1);
        self.update_nz_flags(self.y);
    }

    /// DEC - Decrement Memory
    fn dec(&mut self, bus: &mut dyn Bus, addr: u16) {
        // RMW pattern: read, write original, write modified
        let value = self.read(bus, addr);
        self.write(bus, addr, value); // dummy write
        let result = value.wrapping_sub(1);
        self.write(bus, addr, result);
        self.update_nz_flags(result);
    }

    /// DEX - Decrement X
    fn dex(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.x = self.x.wrapping_sub(1);
        self.update_nz_flags(self.x);
    }

    /// DEY - Decrement Y
    fn dey(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.y = self.y.wrapping_sub(1);
        self.update_nz_flags(self.y);
    }

    // ========================================================================
    // Shift Instructions
    // ========================================================================

    /// ASL - Arithmetic Shift Left
    fn asl(&mut self, bus: &mut dyn Bus, addr: u16) {
        if addr == 0xFFFF {
            // Accumulator mode
            self.status.set(StatusFlags::C, self.a & 0x80 != 0);
            self.a <<= 1;
            self.update_nz_flags(self.a);
        } else {
            // Memory mode - RMW pattern
            let value = self.read(bus, addr);
            self.write(bus, addr, value); // dummy write
            self.status.set(StatusFlags::C, value & 0x80 != 0);
            let result = value << 1;
            self.write(bus, addr, result);
            self.update_nz_flags(result);
        }
    }

    /// LSR - Logical Shift Right
    fn lsr(&mut self, bus: &mut dyn Bus, addr: u16) {
        if addr == 0xFFFF {
            // Accumulator mode
            self.status.set(StatusFlags::C, self.a & 0x01 != 0);
            self.a >>= 1;
            self.update_nz_flags(self.a);
        } else {
            // Memory mode - RMW pattern
            let value = self.read(bus, addr);
            self.write(bus, addr, value); // dummy write
            self.status.set(StatusFlags::C, value & 0x01 != 0);
            let result = value >> 1;
            self.write(bus, addr, result);
            self.update_nz_flags(result);
        }
    }

    /// ROL - Rotate Left
    fn rol(&mut self, bus: &mut dyn Bus, addr: u16) {
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
            let value = self.read(bus, addr);
            self.write(bus, addr, value); // dummy write
            self.status.set(StatusFlags::C, value & 0x80 != 0);
            let result = (value << 1) | carry_in;
            self.write(bus, addr, result);
            self.update_nz_flags(result);
        }
    }

    /// ROR - Rotate Right
    fn ror(&mut self, bus: &mut dyn Bus, addr: u16) {
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
            let value = self.read(bus, addr);
            self.write(bus, addr, value); // dummy write
            self.status.set(StatusFlags::C, value & 0x01 != 0);
            let result = (value >> 1) | carry_in;
            self.write(bus, addr, result);
            self.update_nz_flags(result);
        }
    }

    // ========================================================================
    // Jump/Call Instructions
    // ========================================================================

    /// JMP - Jump
    fn jmp(&mut self, _bus: &mut dyn Bus, addr: u16) {
        // JMP sets PC to target address - 1 because PC will be incremented
        self.pc = addr;
    }

    /// JSR - Jump to Subroutine
    fn jsr(&mut self, bus: &mut dyn Bus, addr: u16) {
        // Push return address (PC - 1) onto stack
        let return_addr = self.pc.wrapping_sub(1);
        self.push(bus, (return_addr >> 8) as u8);
        self.push(bus, (return_addr & 0xFF) as u8);
        // Read high byte of target (for open bus behavior)
        self.read(bus, self.pc.wrapping_add(1));
        self.pc = addr;
    }

    /// RTS - Return from Subroutine
    fn rts(&mut self, bus: &mut dyn Bus, _addr: u16) {
        // Two discarded reads, one on each side of the two pulls (Mesen's
        // `RTS()`).
        self.dummy_read(bus);
        let lo = self.pull(bus) as u16;
        let hi = self.pull(bus) as u16;
        self.dummy_read(bus);
        self.pc = ((hi << 8) | lo).wrapping_add(1);
    }

    /// RTI - Return from Interrupt
    fn rti(&mut self, bus: &mut dyn Bus, _addr: u16) {
        self.dummy_read(bus);
        // Pull status register
        let status = self.pull(bus);
        self.status = StatusFlags::from_bits_truncate(status);
        self.status.remove(StatusFlags::B);
        self.status.insert(StatusFlags::U);

        // Pull program counter
        let lo = self.pull(bus) as u16;
        let hi = self.pull(bus) as u16;
        self.pc = (hi << 8) | lo;
    }

    /// Sample the interrupt lines, as the chip does on the penultimate cycle of
    /// every instruction.
    ///
    /// NMI is edge-triggered and cannot be masked; IRQ is level-triggered and is
    /// Vector an interrupt of `source` reaches its handler through.
    fn interrupt_vector(source: InterruptSource) -> u16 {
        match source {
            InterruptSource::Irq => 0xFFFE,
            InterruptSource::Nmi => 0xFFFA,
        }
    }

    // ========================================================================
    // Branch Instructions
    // ========================================================================

    /// Branch helper - handles page crossing for branches
    fn branch(&mut self, bus: &mut dyn Bus, target: u16) {
        // "A taken non-page-crossing branch ignores IRQ/NMI during its last
        // clock, so that next instruction executes before the IRQ" (Mesen).
        // blargg's `5-branch_delays_irq` is what measures this.
        if self.run_irq && !self.prev_run_irq {
            self.run_irq = false;
        }

        self.dummy_read(bus);
        if (self.pc & 0xFF00) != (target & 0xFF00) {
            self.dummy_read(bus);
        }
        self.pc = target;
    }

    /// BCC - Branch if Carry Clear
    fn bcc(&mut self, bus: &mut dyn Bus, addr: u16) {
        if !self.status.contains(StatusFlags::C) {
            self.branch(bus, addr);
        }
    }

    /// BCS - Branch if Carry Set
    fn bcs(&mut self, bus: &mut dyn Bus, addr: u16) {
        if self.status.contains(StatusFlags::C) {
            self.branch(bus, addr);
        }
    }

    /// BEQ - Branch if Equal (Zero Set)
    fn beq(&mut self, bus: &mut dyn Bus, addr: u16) {
        if self.status.contains(StatusFlags::Z) {
            self.branch(bus, addr);
        }
    }

    /// BNE - Branch if Not Equal (Zero Clear)
    fn bne(&mut self, bus: &mut dyn Bus, addr: u16) {
        if !self.status.contains(StatusFlags::Z) {
            self.branch(bus, addr);
        }
    }

    /// BMI - Branch if Minus (Negative Set)
    fn bmi(&mut self, bus: &mut dyn Bus, addr: u16) {
        if self.status.contains(StatusFlags::N) {
            self.branch(bus, addr);
        }
    }

    /// BPL - Branch if Plus (Negative Clear)
    fn bpl(&mut self, bus: &mut dyn Bus, addr: u16) {
        if !self.status.contains(StatusFlags::N) {
            self.branch(bus, addr);
        }
    }

    /// BVC - Branch if Overflow Clear
    fn bvc(&mut self, bus: &mut dyn Bus, addr: u16) {
        if !self.status.contains(StatusFlags::V) {
            self.branch(bus, addr);
        }
    }

    /// BVS - Branch if Overflow Set
    fn bvs(&mut self, bus: &mut dyn Bus, addr: u16) {
        if self.status.contains(StatusFlags::V) {
            self.branch(bus, addr);
        }
    }

    // ========================================================================
    // Flag Instructions
    // ========================================================================

    /// CLC - Clear Carry
    fn clc(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.status.remove(StatusFlags::C);
    }

    /// CLD - Clear Decimal
    fn cld(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.status.remove(StatusFlags::D);
    }

    /// CLI - Clear Interrupt Disable
    fn cli(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.status.remove(StatusFlags::I);
    }

    /// CLV - Clear Overflow
    fn clv(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.status.remove(StatusFlags::V);
    }

    /// SEC - Set Carry
    fn sec(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.status.insert(StatusFlags::C);
    }

    /// SED - Set Decimal
    fn sed(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.status.insert(StatusFlags::D);
    }

    /// SEI - Set Interrupt Disable
    fn sei(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        self.status.insert(StatusFlags::I);
    }

    // ========================================================================
    // System Instructions
    // ========================================================================

    /// BRK - Force Interrupt
    fn brk(&mut self, bus: &mut dyn Bus, _addr: u16) {
        // Mesen's `BRK()`: the return address is the byte after the padding
        // byte, the pushed status has B set, and the vector is chosen at the
        // status push — an NMI asserted by then takes it over instead of the IRQ
        // vector. The samples are then cleared so the instruction after BRK
        // runs before any pending NMI is taken.
        self.push_word(bus, self.pc.wrapping_add(1));

        let source = if self.need_nmi {
            self.need_nmi = false;
            InterruptSource::Nmi
        } else {
            InterruptSource::Irq
        };

        self.push(bus, (self.status | StatusFlags::B | StatusFlags::U).bits());
        self.status.insert(StatusFlags::I);
        self.pc = Self::read_vector(bus, source);

        self.prev_need_nmi = false;
    }

    /// NOP - No Operation
    fn nop(&mut self, bus: &mut dyn Bus, addr: u16) {
        // The acknowledged NOP - the implied one - does nothing at all. The 2-
        // and 3-byte NOPs read their operand and throw it away, which is where
        // their extra cycles come from (Mesen's `NOP()` is `GetOperandValue()`).
        if self.operand_is_memory() {
            self.read(bus, addr);
        }
    }

    // ========================================================================
    // Unofficial Opcodes
    // ========================================================================

    /// ALR (ASR) - AND + LSR
    /// AND accumulator with memory, then LSR the result.
    /// Opcode: $4B (Immediate)
    fn alr(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        let result = self.a & value;
        self.status.set(StatusFlags::C, result & 0x01 != 0);
        self.a = result >> 1;
        self.update_nz_flags(self.a);
    }

    /// ANC - AND + set C as bit 7
    /// AND accumulator with memory, setting carry to bit 7 of the result.
    /// Opcodes: $0B, $2B (Immediate)
    fn anc(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        self.a &= value;
        self.status.set(StatusFlags::C, self.a & 0x80 != 0);
        self.update_nz_flags(self.a);
    }

    /// ARR - AND + ROR with special V/C handling
    /// AND accumulator with memory, then ROR with special flag handling.
    /// Opcode: $6B (Immediate)
    fn arr(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
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
    }

    /// AXS (SBX) - A & X - imm, store in X
    /// Set X to (A AND X) minus immediate. Affects N, Z, C (not V).
    /// Opcode: $CB (Immediate)
    fn axs(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        let ax = self.a & self.x;
        let result = ax.wrapping_sub(value);
        self.status.set(StatusFlags::C, ax >= value);
        self.x = result;
        self.update_nz_flags(self.x);
    }

    /// LAX - Load A and X
    /// Load both A and X from memory.
    /// Opcodes: $A3, $A7, $AB, $AF, $B3, $B7, $BF
    fn lax(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        self.a = value;
        self.x = value;
        self.update_nz_flags(value);
    }

    /// SAX - Store A AND X
    /// Store A AND X to memory.
    /// Opcodes: $83, $87, $8F, $97
    fn sax(&mut self, bus: &mut dyn Bus, addr: u16) {
        self.write(bus, addr, self.a & self.x);
    }

    /// DCP (DCM) - DEC + CMP
    /// Decrement memory, then compare with A.
    /// Opcodes: $C3, $C7, $CF, $D3, $D7, $DB, $DF
    fn dcp(&mut self, bus: &mut dyn Bus, addr: u16) {
        // RMW pattern: read, write original, write modified
        let value = self.read(bus, addr);
        self.write(bus, addr, value); // dummy write
        let result = value.wrapping_sub(1);
        self.write(bus, addr, result);
        // Compare with accumulator
        let diff = self.a.wrapping_sub(result);
        self.status.set(StatusFlags::C, self.a >= result);
        self.update_nz_flags(diff);
    }

    /// ISC (ISB, INS) - INC + SBC
    /// Increment memory, then subtract from A.
    /// Opcodes: $E3, $E7, $EF, $F3, $F7, $FB, $FF
    fn isc(&mut self, bus: &mut dyn Bus, addr: u16) {
        // RMW pattern: read, write original, write modified
        let value = self.read(bus, addr);
        self.write(bus, addr, value); // dummy write
        let result = value.wrapping_add(1);
        self.write(bus, addr, result);
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
    }

    /// RLA - ROL + AND
    /// Rotate memory left, then AND with A.
    /// Opcodes: $23, $27, $2F, $33, $37, $3B, $3F
    fn rla(&mut self, bus: &mut dyn Bus, addr: u16) {
        // RMW pattern: read, write original, write modified
        let value = self.read(bus, addr);
        self.write(bus, addr, value); // dummy write
        let carry_in = if self.status.contains(StatusFlags::C) {
            1
        } else {
            0
        };
        self.status.set(StatusFlags::C, value & 0x80 != 0);
        let result = (value << 1) | carry_in;
        self.write(bus, addr, result);
        // AND with accumulator
        self.a &= result;
        self.update_nz_flags(self.a);
    }

    /// RRA - ROR + ADC
    /// Rotate memory right, then ADC to A.
    /// Opcodes: $63, $67, $6F, $73, $77, $7B, $7F
    fn rra(&mut self, bus: &mut dyn Bus, addr: u16) {
        // RMW pattern: read, write original, write modified
        let value = self.read(bus, addr);
        self.write(bus, addr, value); // dummy write
        let carry_in = if self.status.contains(StatusFlags::C) {
            0x80
        } else {
            0
        };
        self.status.set(StatusFlags::C, value & 0x01 != 0);
        let result = (value >> 1) | carry_in;
        self.write(bus, addr, result);
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
    }

    /// SLO (ASO) - ASL + ORA
    /// Shift memory left, then ORA with A.
    /// Opcodes: $03, $07, $0F, $13, $17, $1B, $1F
    fn slo(&mut self, bus: &mut dyn Bus, addr: u16) {
        // RMW pattern: read, write original, write modified
        let value = self.read(bus, addr);
        self.write(bus, addr, value); // dummy write
        self.status.set(StatusFlags::C, value & 0x80 != 0);
        let result = value << 1;
        self.write(bus, addr, result);
        // ORA with accumulator
        self.a |= result;
        self.update_nz_flags(self.a);
    }

    /// SRE (LSE) - LSR + EOR
    /// Shift memory right, then EOR with A.
    /// Opcodes: $43, $47, $4F, $53, $57, $5B, $5F
    fn sre(&mut self, bus: &mut dyn Bus, addr: u16) {
        // RMW pattern: read, write original, write modified
        let value = self.read(bus, addr);
        self.write(bus, addr, value); // dummy write
        self.status.set(StatusFlags::C, value & 0x01 != 0);
        let result = value >> 1;
        self.write(bus, addr, result);
        // EOR with accumulator
        self.a ^= result;
        self.update_nz_flags(self.a);
    }

    /// SKB (NOP immediate) - Skip byte
    /// Do nothing (immediate read is already handled by addressing mode).
    /// Opcodes: $80, $82, $89, $C2, $E2
    fn skb(&mut self, bus: &mut dyn Bus, addr: u16) {
        // The immediate operand is fetched and discarded - that fetch is one of
        // the instruction's two cycles.
        self.read(bus, addr);
    }

    /// IGN (NOP with read) - Ignore byte
    /// Read from memory and ignore result.
    /// Opcodes: $04, $44, $64, $14, $34, $54, $74, $D4, $F4, $1C, $3C, $5C, $7C, $DC, $FC
    fn ign(&mut self, bus: &mut dyn Bus, addr: u16) {
        // Perform the read (already done by addressing mode for some)
        self.read(bus, addr);
    }

    /// SHA (AHX, AXA) - Store A & X & (high byte + 1)
    /// Store A AND X AND (high byte of address + 1) to memory.
    /// Has special behavior on page crossing.
    /// Opcodes: $93 (Indirect,Y), $9F (Absolute,Y)
    fn sha(&mut self, bus: &mut dyn Bus, addr: u16) {
        let (target, mask) = self.shx_target(addr);
        let value = self.a & self.x & mask;
        self.write(bus, target, value);
    }

    /// SHX (SXA, XAS) - Store X & (high byte + 1)
    /// Store X AND (high byte of address + 1) to memory.
    /// Opcode: $9E (Absolute,Y)
    fn shx(&mut self, bus: &mut dyn Bus, addr: u16) {
        let (target, mask) = self.shx_target(addr);
        let value = self.x & mask;
        self.write(bus, target, value);
    }

    /// SHY (SYA, SAY) - Store Y & (high byte + 1)
    /// Store Y AND (high byte of address + 1) to memory.
    /// Opcode: $9C (Absolute,X)
    fn shy(&mut self, bus: &mut dyn Bus, addr: u16) {
        let (target, mask) = self.shx_target(addr);
        let value = self.y & mask;
        self.write(bus, target, value);
    }

    /// SHS (TAS, XAS) - Transfer A & X to SP, then SHA
    /// SP = A & X, then store SP & (high byte + 1).
    /// Opcode: $9B (Absolute,Y)
    fn shs(&mut self, bus: &mut dyn Bus, addr: u16) {
        self.sp = self.a & self.x;
        let (target, mask) = self.shx_target(addr);
        let value = self.sp & mask;
        self.write(bus, target, value);
    }

    /// Where an SHx store really lands, and the mask its value gets.
    ///
    /// The index addition keeps the base address's page: a carry out of the low
    /// byte is dropped instead of incrementing the high byte, so an indexed
    /// store that "should" cross a page writes into the page it started in.
    /// The stored byte is `register & (high byte + 1)` of wherever that is,
    /// which is why SHX and SHY are the instructions that depend on their
    /// address instead of their data. blargg's `07-abs_xy` is the test that
    /// pins both halves of the behaviour down.
    fn shx_target(&self, addr: u16) -> (u16, u8) {
        let base = self.addr_base;
        let target = if (base & 0xFF00) != (addr & 0xFF00) {
            (base & 0xFF00) | (addr & 0x00FF)
        } else {
            addr
        };
        (target, ((target >> 8) as u8).wrapping_add(1))
    }

    /// LAE (LAS, LAR) - Load A, X, SP from memory & SP
    /// A = X = SP = memory AND SP
    /// Opcode: $BB (Absolute,Y)
    fn lae(&mut self, bus: &mut dyn Bus, addr: u16) {
        let value = self.read(bus, addr);
        let result = value & self.sp;
        self.a = result;
        self.x = result;
        self.sp = result;
        self.update_nz_flags(result);
    }

    /// ANE (XAA) - A = (A | MAGIC) & X & imm
    /// Transfer X to A, then AND with immediate.
    /// The "magic" constant varies between CPU revisions ($00, $EE, $FF).
    /// Using $FF as the most common value.
    /// Opcode: $8B (Immediate)
    fn ane(&mut self, bus: &mut dyn Bus, addr: u16) {
        const MAGIC: u8 = 0xFF;
        let value = self.read(bus, addr);
        self.a = (self.a | MAGIC) & self.x & value;
        self.update_nz_flags(self.a);
    }

    /// LXA (LAX immediate, ATX) - Load A and X with (A | MAGIC) & imm
    /// A = X = (A | MAGIC) & immediate
    /// Same magic constant issue as ANE.
    /// Opcode: $AB (Immediate)
    fn lxa(&mut self, bus: &mut dyn Bus, addr: u16) {
        const MAGIC: u8 = 0xFF;
        let value = self.read(bus, addr);
        self.a = (self.a | MAGIC) & value;
        self.x = self.a;
        self.update_nz_flags(self.a);
    }

    /// DUMMY (NOP) - Cycle halt / No operation
    /// Does nothing but consume cycles.
    /// Opcodes: $1A, $3A, $5A, $7A, $DA, $FA (Implied)
    fn dummy(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        // NOP - does nothing
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
    fn jam(&mut self, _bus: &mut dyn Bus, _addr: u16) {
        // The opcode has been fetched and the PC moved past it: put it back, so
        // the machine keeps running the same byte — a fetch and a discarded
        // read, two cycles at a time — until an interrupt or a reset arrives.
        self.pc = self.pc.wrapping_sub(1);
        self.halted = true;
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
        addr_mode: AddressingMode::IndirectYWrite,
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
        addr_mode: AddressingMode::AbsoluteYWrite,
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
        addr_mode: AddressingMode::AbsoluteXWrite,
        base_cycles: 7,
        handler: Cpu::asl,
    },
    // 0x1F - SLO (Absolute,X)
    Opcode {
        mnemonic: "SLO",
        addr_mode: AddressingMode::AbsoluteXWrite,
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
        addr_mode: AddressingMode::IndirectYWrite,
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
        addr_mode: AddressingMode::AbsoluteYWrite,
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
        addr_mode: AddressingMode::AbsoluteXWrite,
        base_cycles: 7,
        handler: Cpu::rol,
    },
    // 0x3F - RLA (Absolute,X)
    Opcode {
        mnemonic: "RLA",
        addr_mode: AddressingMode::AbsoluteXWrite,
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
        addr_mode: AddressingMode::IndirectYWrite,
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
        addr_mode: AddressingMode::AbsoluteYWrite,
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
        addr_mode: AddressingMode::AbsoluteXWrite,
        base_cycles: 7,
        handler: Cpu::lsr,
    },
    // 0x5F - SRE (Absolute,X)
    Opcode {
        mnemonic: "SRE",
        addr_mode: AddressingMode::AbsoluteXWrite,
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
        addr_mode: AddressingMode::IndirectYWrite,
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
        addr_mode: AddressingMode::AbsoluteYWrite,
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
        addr_mode: AddressingMode::AbsoluteXWrite,
        base_cycles: 7,
        handler: Cpu::ror,
    },
    // 0x7F - RRA (Absolute,X)
    Opcode {
        mnemonic: "RRA",
        addr_mode: AddressingMode::AbsoluteXWrite,
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
        addr_mode: AddressingMode::IndirectYWrite,
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
        addr_mode: AddressingMode::IndirectYWrite,
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
        addr_mode: AddressingMode::AbsoluteYWrite,
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
        addr_mode: AddressingMode::AbsoluteYWrite,
        base_cycles: 5,
        handler: Cpu::shs,
    },
    // 0x9C - SHY (Absolute,X)
    Opcode {
        mnemonic: "SHY",
        addr_mode: AddressingMode::AbsoluteXWrite,
        base_cycles: 5,
        handler: Cpu::shy,
    },
    // 0x9D - STA (Absolute,X)
    Opcode {
        mnemonic: "STA",
        addr_mode: AddressingMode::AbsoluteXWrite,
        base_cycles: 5,
        handler: Cpu::sta,
    },
    // 0x9E - SHX (Absolute,Y)
    Opcode {
        mnemonic: "SHX",
        addr_mode: AddressingMode::AbsoluteYWrite,
        base_cycles: 5,
        handler: Cpu::shx,
    },
    // 0x9F - SHA (Absolute,Y)
    Opcode {
        mnemonic: "SHA",
        addr_mode: AddressingMode::AbsoluteYWrite,
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
        addr_mode: AddressingMode::IndirectYWrite,
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
        addr_mode: AddressingMode::AbsoluteYWrite,
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
        addr_mode: AddressingMode::AbsoluteXWrite,
        base_cycles: 7,
        handler: Cpu::dec,
    },
    // 0xDF - DCP (Absolute,X)
    Opcode {
        mnemonic: "DCP",
        addr_mode: AddressingMode::AbsoluteXWrite,
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
        addr_mode: AddressingMode::IndirectYWrite,
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
        addr_mode: AddressingMode::AbsoluteYWrite,
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
        addr_mode: AddressingMode::AbsoluteXWrite,
        base_cycles: 7,
        handler: Cpu::inc,
    },
    // 0xFF - ISC (Absolute,X)
    Opcode {
        mnemonic: "ISC",
        addr_mode: AddressingMode::AbsoluteXWrite,
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
        irq: bool,
        cycles: u64,
        /// Raise the NMI line once the CPU has spent this many cycles, so a
        /// test can put an edge inside a sequence that is already running.
        nmi_at_cycle: Option<u64>,
    }

    impl MockBus {
        fn new() -> Self {
            Self {
                memory: [0; 65536],
                nmi: false,
                irq: false,
                cycles: 0,
                nmi_at_cycle: None,
            }
        }

        /// Raise the NMI line, the interrupt that restarts a halted CPU.
        fn set_nmi(&mut self, pending: bool) {
            self.nmi = pending;
        }

        /// Raise the NMI line once this many cycles have been spent, so the
        /// edge can land between the cycles of a sequence.
        fn set_nmi_at_cycle(&mut self, cycle: Option<u64>) {
            self.nmi_at_cycle = cycle;
        }

        /// Raise the IRQ line, which the CPU takes only while I is clear - or,
        /// thanks to the delayed-inhibition rule, one instruction after it goes
        /// clear.
        fn set_irq(&mut self, pending: bool) {
            self.irq = pending;
        }

        fn write_mem(&mut self, addr: u16, value: u8) {
            self.memory[addr as usize] = value;
        }

        /// One cycle's worth of bookkeeping for the mock: the counter, and the
        /// scripted NMI edge.
        fn end_of_cycle(&mut self) {
            self.cycles += 1;
            if self.nmi_at_cycle == Some(self.cycles) {
                self.nmi = true;
            }
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

        fn read_cycle(&mut self, addr: u16) -> u8 {
            let value = self.read(addr);
            self.end_of_cycle();
            value
        }

        fn write_cycle(&mut self, addr: u16, value: u8) {
            self.write(addr, value);
            self.end_of_cycle();
        }

        fn cycle_count(&self) -> u64 {
            self.cycles
        }

        fn on_cpu_cycle(&mut self, _cycles: u8) {}

        fn nmi_line(&self) -> bool {
            self.nmi
        }

        fn irq_pending(&self) -> bool {
            self.irq
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

            // The halt is the PC being wound back onto the opcode, so the
            // machine keeps running the same byte - a fetch and a discarded
            // read - and the PC never moves.
            let pc = cpu.pc();
            assert_eq!(cpu.execute_instruction(&mut bus, 0x02), 2);
            assert_eq!(
                cpu.pc(),
                pc,
                "{opcode:#04X} must not advance the PC while halted"
            );
            assert!(cpu.is_halted());
        }
    }
    /// An NMI asserted while an IRQ sequence is still before its status push
    /// takes the vector over: the frame the sequence pushes stays the IRQ's -
    /// status with B clear - and the handler is the NMI's.
    #[test]
    fn test_nmi_hijacks_an_irq_sequence() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.write_mem(0xFFFE, 0x00); // IRQ -> $9000
        bus.write_mem(0xFFFF, 0x90);
        bus.write_mem(0xFFFA, 0x00); // NMI -> $8000
        bus.write_mem(0xFFFB, 0x80);
        bus.load_program(0x0600, &[0xEA]); // the NOP the IRQ interrupts
        cpu.status.remove(StatusFlags::I);
        cpu.set_pc(0x0600);
        bus.set_irq(true);
        // The mock counts accesses, and the opcode fetch is the caller's, so
        // its numbers run one behind the CPU's: the NOP's discarded read is its
        // cycle 1, the sequence's two discarded reads and the two halves of the
        // return address are 2-5, and its cycle 6 is the CPU's cycle 7 - the
        // status push. An edge after 5 is in time to take the vector.
        bus.set_nmi_at_cycle(Some(5));

        cpu.execute_instruction(&mut bus, 0xEA);

        assert_eq!(cpu.pc(), 0x8000, "the NMI vector took the sequence over");
        assert_eq!(
            bus.read(0x01FB) & StatusFlags::B.bits(),
            0,
            "the frame on the stack is the IRQ's, so its B flag is clear"
        );
    }

    /// An NMI that arrives after the status push misses the sequence: the
    /// handler is the IRQ's, and the edge is left for the next instruction.
    #[test]
    fn test_nmi_after_the_status_push_waits_for_the_next_instruction() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.write_mem(0xFFFE, 0x00); // IRQ -> $9000
        bus.write_mem(0xFFFF, 0x90);
        bus.write_mem(0xFFFA, 0x00); // NMI -> $8000
        bus.write_mem(0xFFFB, 0x80);
        bus.load_program(0x0600, &[0xEA]);
        cpu.status.remove(StatusFlags::I);
        cpu.set_pc(0x0600);
        bus.set_irq(true);
        // After the status push, which is the mock's cycle 6.
        bus.set_nmi_at_cycle(Some(6));

        cpu.execute_instruction(&mut bus, 0xEA);

        assert_eq!(cpu.pc(), 0x9000, "the IRQ vector is the one that runs");
        assert!(cpu.need_nmi, "the NMI is latched, not lost");
    }

    /// An NMI asserted while BRK is still running takes BRK's vector over: one
    /// frame goes on the stack - with B set, as BRK pushes it - and the handler
    /// is the NMI's.
    #[test]
    fn test_nmi_takes_brks_vector_over() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x00]); // BRK
        bus.write_mem(0xFFFE, 0x00); // IRQ -> $9000
        bus.write_mem(0xFFFF, 0x90);
        bus.write_mem(0xFFFA, 0x00); // NMI -> $8000
        bus.write_mem(0xFFFB, 0x80);
        cpu.set_pc(0x0600);
        // BRK's discarded read is the mock's cycle 1, the two halves of the
        // return address are 2-3, and its cycle 4 is the CPU's status push: an
        // edge after 3 lands in time to take the vector.
        bus.set_nmi_at_cycle(Some(3));

        cpu.execute_instruction(&mut bus, 0x00);

        assert_eq!(cpu.pc(), 0x8000, "the NMI took BRK's vector over");
        assert_eq!(cpu.sp(), 0xFA, "three bytes pushed, not six");
        assert_eq!(
            bus.read(0x01FB) & StatusFlags::B.bits(),
            StatusFlags::B.bits(),
            "BRK's frame keeps B set"
        );
    }

    /// Run one instruction. Any interrupt the lines call for is taken inside
    /// it, the way the machine's does it.
    ///
    /// Returns the cycles the CPU spent, instruction and sequence together.
    fn run_instruction(cpu: &mut Cpu, bus: &mut MockBus, opcode: u8) -> u32 {
        u32::from(cpu.execute_instruction(bus, opcode))
    }

    #[test]
    fn test_nmi_resumes_a_halted_cpu() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x02]);
        bus.write_mem(0xFFFA, 0x00); // NMI -> $8000
        bus.write_mem(0xFFFB, 0x80);
        cpu.set_pc(0x0600);

        run_instruction(&mut cpu, &mut bus, 0x02);
        assert_eq!(cpu.pc(), 0x0600, "a KIL re-runs itself");
        assert!(cpu.is_halted());

        bus.set_nmi(true);
        run_instruction(&mut cpu, &mut bus, 0x02);

        assert_eq!(cpu.pc(), 0x8000, "the NMI handler runs");
        assert!(!cpu.is_halted(), "and the halt is over");
    }

    #[test]
    fn test_reset_clears_the_halt() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x02]);
        cpu.set_pc(0x0600);
        run_instruction(&mut cpu, &mut bus, 0x02);
        assert!(cpu.is_halted());

        bus.write_mem(0xFFFC, 0x00);
        bus.write_mem(0xFFFD, 0x90);
        cpu.reset(&mut bus);
        assert!(!cpu.is_halted());
        assert_eq!(cpu.pc(), 0x9000);
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

    /// CLI, SEI and PLP only change what interrupts are allowed *after* the
    /// next instruction has run; the flag itself changes at once.
    #[test]
    fn test_cli_delays_irq_inhibition_by_one_instruction() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x58, 0xEA, 0xEA, 0xEA]); // CLI, NOP…
        bus.write_mem(0xFFFE, 0x00);
        bus.write_mem(0xFFFF, 0x90); // IRQ vector
        cpu.pc = 0x0600;
        cpu.status.insert(StatusFlags::I);
        bus.set_irq(true);

        // CLI runs; its own poll still saw the interrupt inhibited.
        assert_eq!(run_instruction(&mut cpu, &mut bus, 0x58), 2);
        assert!(
            !cpu.status.contains(StatusFlags::I),
            "the flag itself clears immediately"
        );

        // Only the instruction after CLI sees the cleared flag, so it runs and
        // the handler's seven cycles follow it.
        assert_eq!(run_instruction(&mut cpu, &mut bus, 0xEA), 2 + 7);
        assert_eq!(cpu.pc(), 0x9000);
        // The pushed return address is the NOP's successor ($0601 + 1), so the
        // NOP really did run before the handler.
        assert_eq!(bus.memory[0x01FD], 0x06, "PCH");
        assert_eq!(bus.memory[0x01FC], 0x02, "PCL");
    }

    /// RTI is the exception: it changes inhibition at once, so an IRQ that was
    /// held back while I was set is taken immediately after the RTI.
    #[test]
    fn test_rti_lets_an_irq_through_immediately() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.write_mem(0xFFFE, 0x00);
        bus.write_mem(0xFFFF, 0x90);
        bus.set_irq(true);

        // The stack RTI pops: status with I clear, then the return address.
        cpu.sp = 0xFD;
        bus.write_mem(0x01FE, 0x20); // status: U set, I clear
        bus.write_mem(0x01FF, 0x00); // PC low
        bus.write_mem(0x0100, 0x80); // PC high (the stack wraps to $0100)
        cpu.pc = 0x0600;
        cpu.status.insert(StatusFlags::I);

        // The handler follows RTI without another instruction in between.
        assert_eq!(run_instruction(&mut cpu, &mut bus, 0x40), 6 + 7);
        assert_eq!(cpu.pc(), 0x9000);
        // The return address is the one RTI restored, and the status byte the
        // sequence pushed is the one RTI restored (I clear, U set): RTI had
        // already taken effect.
        assert_eq!(bus.memory[0x0100], 0x80, "PCH");
        assert_eq!(bus.memory[0x01FF], 0x00, "PCL");
        assert_eq!(bus.memory[0x01FE], 0x20, "pushed status");
    }

    /// A taken branch polls on its own penultimate cycle like any other
    /// instruction, so an interrupt that is already waiting is taken as soon as
    /// the branch ends - at the branch's target, not after one more instruction.
    #[test]
    fn test_taken_branch_polls_on_its_own_penultimate_cycle() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x90, 0x05, 0xEA, 0xEA]); // BCC +5
        bus.write_mem(0xFFFE, 0x00);
        bus.write_mem(0xFFFF, 0x90);
        cpu.pc = 0x0600;
        cpu.status.remove(StatusFlags::C); // BCC is taken
        cpu.status.remove(StatusFlags::I);

        bus.set_irq(true);
        assert_eq!(run_instruction(&mut cpu, &mut bus, 0x90), 3 + 7);
        assert_eq!(cpu.pc(), 0x9000);
        assert_eq!(bus.memory[0x01FD], 0x06, "PCH");
        assert_eq!(bus.memory[0x01FC], 0x07, "PCL: the branch's target");

        // A branch that is not taken behaves the same way, one cycle shorter.
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x90, 0x05, 0xEA]);
        bus.write_mem(0xFFFE, 0x00);
        bus.write_mem(0xFFFF, 0x90);
        cpu.pc = 0x0600;
        cpu.status.insert(StatusFlags::C); // BCC falls through
        cpu.status.remove(StatusFlags::I);
        bus.set_irq(true);

        assert_eq!(run_instruction(&mut cpu, &mut bus, 0x90), 2 + 7);
        assert_eq!(cpu.pc(), 0x9000);
        assert_eq!(bus.memory[0x01FC], 0x02, "PCL: the branch's successor");
    }

    /// A loop made of nothing but taken branches must keep polling.
    ///
    /// `BNE self` is how a game waits for VBlank, and the poll cannot be handed
    /// to "the instruction after the branch": in a one-instruction loop that is
    /// the same branch again, so the interrupt would never be taken and the
    /// screen would never change again. Regression test for 热血格斗, which
    /// parks in exactly this loop with NMI enabled.
    #[test]
    fn test_branch_only_loop_does_not_starve_nmi() {
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0xD0, 0xFE]); // BNE $0600
        bus.write_mem(0xFFFA, 0x00);
        bus.write_mem(0xFFFB, 0x90); // NMI vector -> $9000
        cpu.pc = 0x0600;
        cpu.status.remove(StatusFlags::Z); // BNE is taken, back to itself
        bus.set_nmi(true);

        // The first pass through the loop polls and takes it.
        assert_eq!(run_instruction(&mut cpu, &mut bus, 0xD0), 3 + 7);
        assert_eq!(cpu.pc(), 0x9000);
        assert!(!cpu.need_nmi, "taking the NMI acknowledges it");
        assert_eq!(bus.memory[0x01FD], 0x06, "PCH");
        assert_eq!(bus.memory[0x01FC], 0x00, "PCL: the loop's own address");
    }

    /// The SHx stores keep the page the address started in when the index
    /// carries out of the low byte, and mask the stored byte with that page's
    /// high byte + 1.
    #[test]
    fn test_shx_page_cross_keeps_the_base_page() {
        // SHY $12F0,X with X = $20 would land in $1310: it lands in $1210
        // instead, with the value masked by $12 + 1.
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x9C, 0xF0, 0x12]);
        cpu.pc = 0x0600;
        cpu.x = 0x20;
        cpu.y = 0xFF;
        assert_eq!(cpu.execute_instruction(&mut bus, 0x9C), 5);
        // Y & ($12 + 1): the page it stayed in is what the mask comes from.
        assert_eq!(bus.memory[0x1210], 0x13);
        assert_eq!(
            bus.memory[0x1310], 0x00,
            "the page it would have crossed is untouched"
        );

        // No carry: the store lands exactly where the arithmetic says.
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x9E, 0xF0, 0x12]);
        cpu.pc = 0x0600;
        cpu.x = 0xFF;
        cpu.y = 0x0F;
        cpu.execute_instruction(&mut bus, 0x9E);
        assert_eq!(bus.memory[0x12FF], 0x13);

        // The same rule applies to the A & X store and to SHS, which also
        // writes the AND into the stack pointer first.
        let mut cpu = Cpu::new();
        let mut bus = MockBus::new();
        bus.load_program(0x0600, &[0x9B, 0xF0, 0x12]);
        cpu.pc = 0x0600;
        cpu.y = 0x20;
        cpu.a = 0x0F;
        cpu.x = 0xF3;
        cpu.execute_instruction(&mut bus, 0x9B);
        assert_eq!(cpu.sp, 0x03);
        assert_eq!(bus.memory[0x1210], 0x03 & 0x13);
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
