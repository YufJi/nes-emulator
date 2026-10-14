//! Mapper 95 - Namcot 3425 (Dragon Buster)
//!
//! Namco's 108-family banking (the same registers as mapper 206) with CHR A15
//! wired to CIRAM A10, so the CHR bank a tile comes from also picks which
//! nametable it lands in. That is what lets Dragon Buster show a status bar
//! that does not scroll with the playfield.
//!
//! The register interface is mapper 206's, with bit 5 of the bank data acting
//! as the nametable bit on the first six registers (`..ND DDDD`).
//!
//! Only the nametable bit is approximated here: the renderer still picks one
//! mirroring mode per scanline rather than one nametable per pattern fetch, so
//! the selection is global instead of per-nametable.
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_095>

use super::Mapper;
use crate::ppu::rendering::Mirroring;
use crate::prelude::*;

/// Namcot 3425 mapper (Mapper 95)
#[derive(Debug)]
pub struct Mapper95 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    bank_select: u8,
    banks: [u8; 8],
    /// Nametable bit from the last CHR bank write
    upper_nametable: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper95 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            bank_select: 0,
            // Power-on state: every window starts on bank 0, as on the board
            // before the game programmes its own banks.
            banks: [0; 8],
            upper_nametable: false,
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_bank(&self, addr: u16) -> usize {
        let banks = (self.prg_rom.len() / 0x2000).max(1);
        match addr {
            0x8000..=0x9FFF => self.banks[6] as usize % banks,
            0xA000..=0xBFFF => self.banks[7] as usize % banks,
            0xC000..=0xDFFF => banks.saturating_sub(2),
            _ => banks.saturating_sub(1),
        }
    }

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

impl Mapper for Mapper95 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, bank_select, banks,
        upper_nametable, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_bank(addr) * 0x400 + (addr as usize & 0x3FF);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
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
            0x8000..=0x9FFE if addr & 1 == 0 => self.bank_select = value & 0x07,
            0x8001..=0x9FFF if addr & 1 == 1 => {
                let register = (self.bank_select & 0x07) as usize;
                let mut data = value;
                if register <= 5 {
                    // Bit 5 goes to CIRAM A10 instead of the CHR bank.
                    self.upper_nametable = value & 0x20 != 0;
                    data &= 0x1F;
                }
                self.banks[register] = data;
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
