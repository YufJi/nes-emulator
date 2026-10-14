//! Mapper 18 - Jaleco SS88006
//!
//! Three 8 KB PRG windows (the last stays fixed), eight 1 KB CHR windows, a
//! nametable select and a cycle-counting IRQ with eight clock modes that
//! decide which bit of the counter the IRQ watches.
//!
//! Registers (each bank takes two writes, low nibble then high nibble):
//! - $8000/$8001: 8 KB PRG bank at $8000
//! - $8002/$8003: 8 KB PRG bank at $A000
//! - $9000/$9001: 8 KB PRG bank at $C000
//! - $A000-$D003: the eight 1 KB CHR banks, two registers each
//! - $E000-$E003: IRQ latch, four nibbles
//! - $F000:       reload the counter from the latch
//! - $F001:       IRQ mode (bits 1-3) and enable (bit 0)
//! - $F002:       mirroring (0 = horizontal, 1 = vertical, 2 = one screen)
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_018>

use crate::prelude::*;
use crate::ppu::rendering::Mirroring;
use super::Mapper;

/// Jaleco SS88006 mapper (Mapper 18)
#[derive(Debug)]
pub struct Mapper18 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 8 KB PRG banks for $8000, $A000 and $C000, as nibble pairs
    prg: [u16; 3],
    /// 1 KB CHR banks, as nibble pairs
    chr: [u16; 8],
    mirroring: Mirroring,
    irq_latch: u16,
    irq_counter: u16,
    irq_mode: u8,
    irq_enabled: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper18 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg: [0, 1, 2],
            chr: [0, 1, 2, 3, 4, 5, 6, 7],
            mirroring: Mirroring::Horizontal,
            irq_latch: 0xFFFF,
            irq_counter: 0xFFFF,
            irq_mode: 0,
            irq_enabled: false,
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let banks = (self.prg_rom.len() / 0x2000).max(1);
        let bank = match addr {
            0x8000..=0x9FFF => self.prg[0] as usize % banks,
            0xA000..=0xBFFF => self.prg[1] as usize % banks,
            0xC000..=0xDFFF => self.prg[2] as usize % banks,
            _ => banks.saturating_sub(1),
        };
        bank * 0x2000 + (addr as usize & 0x1FFF)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let banks = (self.chr_rom.len() / 0x400).max(1);
        let window = ((addr >> 10) & 0x07) as usize;
        (self.chr[window] as usize % banks) * 0x400 + (addr as usize & 0x3FF)
    }

    /// Joins a nibble pair into one byte: low nibble first, then high.
    ///
    /// The result stays an 8-bit bank number, which is what the board's
    /// registers are; only the IRQ latch uses four nibbles.
    fn set_nibble(target: &mut u16, value: u8, high: bool) {
        let nibble = u16::from(value & 0x0F);
        *target = if high {
            (*target & 0x000F) | (nibble << 4)
        } else {
            (*target & 0xFFF0) | nibble
        };
    }
}

impl Mapper for Mapper18 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg, chr, mirroring, irq_latch,
        irq_counter, irq_mode, irq_enabled, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset(addr);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            0x8000..=0xFFFF => {
                let index = self.prg_offset(addr);
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let index = self.chr_offset(addr);
                if let Some(byte) = self.chr_rom.get_mut(index) {
                    *byte = value;
                }
            }
            0x8000 | 0x8001 => Self::set_nibble(&mut self.prg[0], value, addr & 1 == 1),
            0x8002 | 0x8003 => Self::set_nibble(&mut self.prg[1], value, addr & 1 == 1),
            0x9000 | 0x9001 => Self::set_nibble(&mut self.prg[2], value, addr & 1 == 1),
            0xA000..=0xD003 => {
                // Four registers per 2 KB CHR window, two nibbles each.
                let window = ((addr - 0xA000) / 0x800) as usize;
                let high = addr & 0x03 >= 0x02;
                Self::set_nibble(&mut self.chr[window], value, high);
            }
            0xE000..=0xE003 => {
                let nibble = u16::from(value & 0x0F);
                let shift = ((addr - 0xE000) as u32) * 4;
                self.irq_latch = (self.irq_latch & !(0x0F << shift)) | (nibble << shift);
            }
            0xF000 => self.irq_counter = self.irq_latch,
            0xF001 => {
                self.irq_mode = (value >> 1) & 0x07;
                self.irq_enabled = value & 0x01 != 0;
                self.irq_pending = false;
            }
            0xF002 => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Horizontal,
                    1 => Mirroring::Vertical,
                    _ => Mirroring::SingleScreen,
                }
            }
            _ => {}
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    fn mirroring(&self) -> Option<Mirroring> {
        Some(self.mirroring)
    }

    /// Eight modes, each watching a different bit of the counter as it walks
    /// backwards from the latch.
    fn on_cpu_cycles(&mut self, cycles: u8) {
        if !self.irq_enabled || self.irq_counter == 0 {
            return;
        }
        let before = self.irq_counter;
        self.irq_counter = self.irq_counter.saturating_sub(u16::from(cycles));

        let mask = match self.irq_mode {
            0 => 0xFFFF,
            1 => 0xF000,
            2 | 3 => 0xFF00,
            4..=7 => 0xFFF0,
            _ => 0xFFFF,
        };
        let fired = match self.irq_mode {
            0 => self.irq_counter == 0,
            _ => (before & mask) != (self.irq_counter & mask),
        };

        if fired {
            self.irq_counter = 0;
            self.irq_enabled = false;
            self.irq_pending = true;
        }
    }
}

