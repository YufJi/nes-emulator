//! Mapper 114 - Asder / Waixing MMC3 clone with 128 KB CHR RAM
//!
//! An MMC3-shaped board whose bank registers are loaded through a two-byte
//! handshake ($A000 names the register, $C000 loads it) rather than through the
//! MMC3's own select/data pair.
//!
//! Registers:
//! - $6000-$7FFF: outer PRG register. Bit 7 switches $8000-$BFFF to a single
//!   16 KB bank; otherwise the two 8 KB registers below fill those windows.
//! - $8000-$9FFF: mirroring (bit 0: 0 = vertical, 1 = horizontal)
//! - $A000: register number for the next $C000 write
//! - $C000: bank data
//!   - 0, 2: 2 KB CHR banks at $0000 and $0800
//!   - 6, 1, 7, 3: 1 KB CHR banks at $1000, $1400, $1800, $1C00
//!   - 4, 5: 8 KB PRG banks at $8000 and $A000
//! - $E002: acknowledge the IRQ
//! - $E003: IRQ counter; it counts down one per rendered scanline
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_114>

use super::Mapper;
use crate::ppu::rendering::Mirroring;
use crate::prelude::*;

/// Asder mapper (Mapper 114)
#[derive(Debug)]
pub struct Mapper114 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Last value written to $6000-$7FFF
    outer: u8,
    /// Register number latched by $A000, and whether one is latched at all
    reg_select: u8,
    reg_latched: bool,
    /// The eight bank registers
    banks: [u8; 8],
    horizontal_mirroring: bool,
    irq_counter: u8,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper114 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            outer: 0,
            reg_select: 0,
            reg_latched: false,
            banks: [0, 1, 2, 3, 4, 5, 6, 7],
            horizontal_mirroring: false,
            irq_counter: 0,
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let banks = (self.prg_rom.len() / 0x2000).max(1);
        let bank = match addr {
            0x8000..=0x9FFF if self.outer & 0x80 != 0 => {
                // 16 KB mode: one bank across both low windows.
                ((self.outer as usize & 0x1F) * 2) % banks
            }
            0x8000..=0x9FFF => self.banks[4] as usize % banks,
            0xA000..=0xBFFF if self.outer & 0x80 != 0 => {
                (((self.outer as usize & 0x1F) * 2) + 1) % banks
            }
            0xA000..=0xBFFF => self.banks[5] as usize % banks,
            0xC000..=0xDFFF => banks.saturating_sub(2),
            _ => banks.saturating_sub(1),
        };
        bank * 0x2000 + (addr as usize & 0x1FFF)
    }

    /// CHR bank for a pattern-table address, in 1 KB units.
    ///
    /// The two 2 KB registers sit in slots 0 and 2, and the four 1 KB ones are
    /// interleaved through slots 1, 3, 6 and 7 - the board wires them that way.
    fn chr_bank(&self, addr: u16) -> usize {
        let banks = (self.chr_rom.len() / 0x400).max(1);
        let bank = match addr {
            0x0000..=0x07FF => (self.banks[0] as usize & 0xFE) + ((addr >> 10) & 1) as usize,
            0x0800..=0x0FFF => (self.banks[2] as usize & 0xFE) + ((addr >> 10) & 1) as usize,
            0x1000..=0x13FF => self.banks[6] as usize,
            0x1400..=0x17FF => self.banks[1] as usize,
            0x1800..=0x1BFF => self.banks[7] as usize,
            _ => self.banks[3] as usize,
        };
        bank % banks
    }
}

impl Mapper for Mapper114 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, outer, reg_select, reg_latched,
        banks, horizontal_mirroring, irq_counter, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_bank(addr) * 0x400 + (addr as usize & 0x3FF);
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
                let index = self.chr_bank(addr) * 0x400 + (addr as usize & 0x3FF);
                if let Some(byte) = self.chr_rom.get_mut(index) {
                    *byte = value;
                }
            }
            0x6000..=0x7FFF => self.outer = value,
            0x8000..=0x9FFF => self.horizontal_mirroring = value & 0x01 != 0,
            0xA000 => {
                self.reg_latched = true;
                self.reg_select = value;
            }
            0xC000 => {
                if self.reg_latched {
                    self.banks[(self.reg_select & 0x07) as usize] = value;
                }
            }
            0xE002 => self.irq_pending = false,
            0xE003 => self.irq_counter = value,
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
        Some(if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        })
    }

    fn on_scanline(&mut self, scanline: u16, rendering: bool) {
        if !rendering || scanline >= 240 || self.irq_counter == 0 {
            return;
        }
        self.irq_counter -= 1;
        if self.irq_counter == 0 {
            self.irq_pending = true;
        }
    }
}
