//! Mapper 206 - Namcot 118 / Tengen MIMIC-1 (DxROM)
//!
//! The simpler predecessor of the MMC3. The bank select and bank data
//! registers work exactly like MMC3's, but there is no CHR banking mode to
//! choose, no PRG mode to choose, no WRAM, and no IRQ: the last two 8 KB PRG
//! banks are always fixed and the CHR layout is always two 2 KB banks at
//! $0000-$0FFF followed by four 1 KB banks at $1000-$1FFF.
//!
//! Register mask is $E001:
//! - $8000-$9FFE (even): bank select, low three bits
//! - $8001-$9FFF (odd):  bank data
//! - $A000-$FFFF:        nothing
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_206>

use super::Mapper;
use crate::prelude::*;

/// Namcot 118 mapper (Mapper 206)
#[derive(Debug)]
pub struct Mapper206 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Last value written to the bank select register
    bank_select: u8,
    /// The eight bank registers, indexed as in the bank select register
    banks: [u8; 8],
    prg_ram: [u8; 0x2000],
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper206 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            bank_select: 0,
            banks: [0; 8],
            prg_ram: [0; 0x2000],
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_bank(&self, addr: u16) -> usize {
        let banks = (self.prg_rom.len() / 0x2000).max(1);
        match addr {
            0x8000..=0x9FFF => self.banks[6] as usize % banks,
            0xA000..=0xBFFF => self.banks[7] as usize % banks,
            // The top two banks never move.
            0xC000..=0xDFFF => banks.saturating_sub(2),
            _ => banks.saturating_sub(1),
        }
    }

    /// CHR bank for a pattern-table address, in 1 KB units.
    fn chr_bank(&self, addr: u16) -> usize {
        let banks = (self.chr_rom.len() / 0x400).max(1);
        let bank = match addr {
            0x0000..=0x07FF => (self.banks[0] as usize) * 2 + ((addr >> 10) & 1) as usize,
            0x0800..=0x0FFF => (self.banks[1] as usize) * 2 + ((addr >> 10) & 1) as usize,
            0x1000..=0x13FF => self.banks[2] as usize,
            0x1400..=0x17FF => self.banks[3] as usize,
            0x1800..=0x1BFF => self.banks[4] as usize,
            _ => self.banks[5] as usize,
        };
        bank % banks
    }
}

impl Mapper for Mapper206 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, bank_select, banks, prg_ram,
        irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_bank(addr) * 0x400 + (addr as usize & 0x3FF);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize & 0x1FFF],
            0x8000..=0xFFFF => {
                let index = self.prg_bank(addr) * 0x2000 + (addr as usize & 0x1FFF);
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
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize & 0x1FFF] = value,
            0x8000..=0x9FFE if addr & 1 == 0 => self.bank_select = value & 0x07,
            0x8001..=0x9FFF if addr & 1 == 1 => {
                self.banks[(self.bank_select & 0x07) as usize] = value;
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

    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    fn load_save_ram(&mut self, data: &[u8]) {
        let len = data.len().min(self.prg_ram.len());
        self.prg_ram[..len].copy_from_slice(&data[..len]);
    }
}
