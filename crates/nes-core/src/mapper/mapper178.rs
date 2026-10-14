//! Mapper 178 - Waixing FS004 / "Education" boards
//!
//! A 32 KB PRG switch assembled from two registers plus a nametable select,
//! all in the expansion area, with 32 KB of work RAM behind them.
//!
//! Registers:
//! - $4800: bit 0 selects the nametable (0 = vertical, 1 = horizontal)
//! - $4801: bits 1-4 become PRG bits 1-4 (bit 0 is always clear)
//! - $4802: bits 0-3 become PRG bits 5-8
//! - $4803: saved, not used for banking
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_178>

use crate::prelude::*;
use crate::ppu::rendering::Mirroring;
use super::Mapper;

/// Waixing FS004 mapper (Mapper 178)
#[derive(Debug)]
pub struct Mapper178 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    reg: [u8; 3],
    horizontal_mirroring: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper178 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg_ram: vec![0u8; 0x8000],
            reg: [0; 3],
            horizontal_mirroring: false,
            chr_is_ram,
            irq_pending: false,
        }
    }

    /// The two PRG registers are joined to form a 32 KB bank number.
    fn prg_offset(&self) -> usize {
        let bank = (self.reg[0] | self.reg[1]) as usize;
        let banks = (self.prg_rom.len() / 0x8000).max(1);
        (bank % banks) * 0x8000
    }
}

impl Mapper for Mapper178 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_ram, reg, horizontal_mirroring,
        irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.chr_rom.get(addr as usize).copied().unwrap_or(0),
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let index = self.prg_offset() + (addr - 0x8000) as usize;
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                if let Some(byte) = self.chr_rom.get_mut(addr as usize) {
                    *byte = value;
                }
            }
            0x4800 => self.horizontal_mirroring = (value & 0x01) != 0,
            0x4801 => self.reg[0] = (value >> 1) & 0x0F,
            0x4802 => self.reg[1] = value << 2,
            0x4803 => self.reg[2] = value,
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = value,
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

    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    fn load_save_ram(&mut self, data: &[u8]) {
        let len = data.len().min(self.prg_ram.len());
        self.prg_ram[..len].copy_from_slice(&data[..len]);
    }
}
