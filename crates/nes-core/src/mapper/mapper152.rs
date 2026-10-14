//! Mapper 152 - Bandai 74161/32 (Arkanoid II, Gegege no Kitarou 2)
//!
//! One register holds everything: the 16 KB PRG window, the 8 KB CHR window
//! and the one-screen nametable select.
//!
//! Register ($8000-$FFFF): `MPPP CCCC`
//! - Bits 0-3: 8 KB CHR bank
//! - Bits 4-6: 16 KB PRG bank at $8000-$BFFF ($C000-$FFFF stays fixed)
//! - Bit 7:    nametable select (0 = lower, 1 = upper)
//!
//! The board has bus conflicts on writes; this core does not model them yet
//! (see `docs/architecture.md` §15.1).
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_152>

use crate::prelude::*;
use crate::ppu::rendering::Mirroring;
use super::Mapper;

/// Bandai 74161/32 mapper (Mapper 152)
#[derive(Debug)]
pub struct Mapper152 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_bank: u8,
    chr_bank: u8,
    upper_nametable: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper152 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank: 0,
            upper_nametable: false,
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_offset(&self) -> usize {
        let banks = (self.prg_rom.len() / 0x4000).max(1);
        (self.prg_bank as usize % banks) * 0x4000
    }

    fn prg_last_offset(&self) -> usize {
        self.prg_rom.len().saturating_sub(0x4000)
    }

    fn chr_offset(&self) -> usize {
        let banks = (self.chr_rom.len() / 0x2000).max(1);
        (self.chr_bank as usize % banks) * 0x2000
    }
}

impl Mapper for Mapper152 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_bank, chr_bank,
        upper_nametable, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset() + (addr as usize);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            0x8000..=0xBFFF => {
                let index = self.prg_offset() + (addr - 0x8000) as usize;
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            0xC000..=0xFFFF => {
                let index = self.prg_last_offset() + (addr - 0xC000) as usize;
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let index = self.chr_offset() + (addr as usize);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            0x8000..=0xFFFF => {
                self.chr_bank = value & 0x0F;
                self.prg_bank = (value >> 4) & 0x07;
                self.upper_nametable = (value & 0x80) != 0;
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
        Some(if self.upper_nametable {
            Mirroring::SingleScreenUpper
        } else {
            Mirroring::SingleScreen
        })
    }
}
