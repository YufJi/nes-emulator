//! Mapper 133 - Sachen 8259 / "Jovial Race" boards
//!
//! A single register in the expansion area picks both banks; everything else
//! in $4100-$7FFF is plain work RAM.
//!
//! Register ($4120):
//! - Bit 2:    32 KB PRG bank
//! - Bits 0-1: 8 KB CHR bank
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_133>

use super::Mapper;
use crate::prelude::*;

/// Sachen 8259 mapper (Mapper 133)
#[derive(Debug)]
pub struct Mapper133 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: [u8; 0x2000],
    prg_bank: u8,
    chr_bank: u8,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper133 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg_ram: [0; 0x2000],
            prg_bank: 0,
            chr_bank: 0,
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_offset(&self) -> usize {
        let banks = (self.prg_rom.len() / 0x8000).max(1);
        (self.prg_bank as usize % banks) * 0x8000
    }

    fn chr_offset(&self) -> usize {
        let banks = (self.chr_rom.len() / 0x2000).max(1);
        (self.chr_bank as usize % banks) * 0x2000
    }
}

impl Mapper for Mapper133 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_ram, prg_bank, chr_bank,
        irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset() + (addr as usize);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize & 0x1FFF],
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
                let index = self.chr_offset() + (addr as usize);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            // The register lives inside the work RAM window, so it is checked
            // first.
            0x4120 => {
                self.prg_bank = (value >> 2) & 0x01;
                self.chr_bank = value & 0x03;
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize & 0x1FFF] = value,
            _ => {}
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    fn load_save_ram(&mut self, data: &[u8]) {
        let len = data.len().min(self.prg_ram.len());
        self.prg_ram[..len].copy_from_slice(&data[..len]);
    }
}
