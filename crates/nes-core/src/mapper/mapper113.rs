//! Mapper 113 - HES / NTDEC "6-in-1" boards
//!
//! A discrete board whose single register lives in the expansion area, and
//! which is mirrored at a handful of aliases. Everything the register holds is
//! derived from one byte: a 32 KB PRG bank plus two CHR bank fields.
//!
//! Registers ($4100, $4111, $4120, $4194, $4195, $4900, $8008, $8009):
//! - Bits 3-7:      32 KB PRG bank
//! - Bit 3 of that, plus bits 0-2: 8 KB CHR bank
//!
//! $8E66/$8E67 pick between CHR banks 0 and 1 by "is the low nibble zero";
//! $E00A switches to one-screen (lower) mirroring.
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_113>

use crate::prelude::*;
use crate::ppu::rendering::Mirroring;
use super::Mapper;

/// HES mapper (Mapper 113)
#[derive(Debug)]
pub struct Mapper113 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_bank: u8,
    chr_bank: u8,
    one_screen_nametable: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper113 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank: 0,
            one_screen_nametable: false,
            chr_is_ram,
            irq_pending: false,
        }
    }

    /// The register's one byte split into its two bank fields.
    fn select_banks(&mut self, value: u8) {
        self.prg_bank = value >> 3;
        self.chr_bank = ((value >> 3) & 0x08) + (value & 0x07);
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

impl Mapper for Mapper113 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_bank, chr_bank,
        one_screen_nametable, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset() + (addr as usize);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
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
            0x4100 | 0x4111 | 0x4120 | 0x4194 | 0x4195 | 0x4900 => self.select_banks(value),
            0x8008 | 0x8009 => self.select_banks(value),
            0x8E66 | 0x8E67 => self.chr_bank = if value & 0x07 != 0 { 0 } else { 1 },
            0xE00A => self.one_screen_nametable = true,
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
        self.one_screen_nametable.then_some(Mirroring::SingleScreen)
    }
}
