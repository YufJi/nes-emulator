//! Mapper 112 - Asder / NTDEC boards
//!
//! An MMC3-shaped board with the bank select and bank data registers split
//! apart: $8000 picks which of the eight bank registers the next write to
//! $A000 will load, and the two PRG slots are always the low two of the four
//! 8 KB windows ($C000-$FFFF stays fixed).
//!
//! Registers:
//! - $8000: bank select (bits 0-2)
//! - $A000: bank data
//!   - 0/1: 8 KB PRG banks at $8000 and $A000
//!   - 2/3: 2 KB CHR banks at $0000 and $0800
//!   - 4-7: 1 KB CHR banks at $1000, $1400, $1800, $1C00
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_112>

use super::Mapper;
use crate::prelude::*;

/// Asder mapper (Mapper 112)
#[derive(Debug)]
pub struct Mapper112 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Last value written to the bank select register
    bank_select: u8,
    /// 8 KB PRG banks for $8000 and $A000
    prg: [u8; 2],
    /// 2 KB CHR banks for $0000 and $0800
    chr_2k: [u8; 2],
    /// 1 KB CHR banks for $1000-$1FFF
    chr_1k: [u8; 4],
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper112 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            bank_select: 0,
            prg: [0, 1],
            chr_2k: [0, 1],
            chr_1k: [0, 1, 2, 3],
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let banks = (self.prg_rom.len() / 0x2000).max(1);
        let bank = match addr {
            0x8000..=0x9FFF => self.prg[0] as usize % banks,
            0xA000..=0xBFFF => self.prg[1] as usize % banks,
            0xC000..=0xDFFF => banks.saturating_sub(2),
            _ => banks.saturating_sub(1),
        };
        bank * 0x2000 + (addr as usize & 0x1FFF)
    }

    /// CHR bank for a pattern-table address, in 1 KB units.
    fn chr_bank(&self, addr: u16) -> usize {
        let banks = (self.chr_rom.len() / 0x400).max(1);
        let bank = match addr {
            0x0000..=0x07FF => (self.chr_2k[0] as usize) * 2 + ((addr >> 10) & 1) as usize,
            0x0800..=0x0FFF => (self.chr_2k[1] as usize) * 2 + ((addr >> 10) & 1) as usize,
            0x1000..=0x13FF => self.chr_1k[0] as usize,
            0x1400..=0x17FF => self.chr_1k[1] as usize,
            0x1800..=0x1BFF => self.chr_1k[2] as usize,
            _ => self.chr_1k[3] as usize,
        };
        bank % banks
    }
}

impl Mapper for Mapper112 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, bank_select, prg, chr_2k, chr_1k,
        irq_pending);

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
            0x8000 => self.bank_select = value,
            0xA000 => match self.bank_select & 0x07 {
                0 => self.prg[0] = value,
                1 => self.prg[1] = value,
                2 => self.chr_2k[0] = value,
                3 => self.chr_2k[1] = value,
                4 => self.chr_1k[0] = value,
                5 => self.chr_1k[1] = value,
                6 => self.chr_1k[2] = value,
                _ => self.chr_1k[3] = value,
            },
            _ => {}
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }
}
