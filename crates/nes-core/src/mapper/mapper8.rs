//! Mapper 8 - FFE F3
//!
//! Front Fareast's F3 board: 16 KB of switchable PRG at $8000-$BFFF with the
//! last 16 KB fixed at $C000-$FFFF, plus an 8 KB CHR bank. The CHR is usually
//! RAM on this board, in which case the bank bits have nothing to select.
//!
//! Register ($8000-$FFFF):
//! - Bits 0-2: 8 KB CHR bank
//! - Bits 3-7: 16 KB PRG bank at $8000-$BFFF
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_008>

use crate::prelude::*;
use super::Mapper;

/// FFE F3 mapper (Mapper 8)
#[derive(Debug)]
pub struct Mapper8 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_bank: u8,
    chr_bank: u8,
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper8 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank: 0,
            chr_is_ram,
            irq_pending: false,
        }
    }

    /// Offset of the 16 KB PRG bank mapped at $8000.
    fn prg_offset(&self) -> usize {
        let banks = (self.prg_rom.len() / 0x4000).max(1);
        (self.prg_bank as usize % banks) * 0x4000
    }

    /// Offset of the fixed last 16 KB PRG bank.
    fn prg_last_offset(&self) -> usize {
        self.prg_rom.len().saturating_sub(0x4000)
    }

    fn chr_offset(&self) -> usize {
        let banks = (self.chr_rom.len() / 0x2000).max(1);
        (self.chr_bank as usize % banks) * 0x2000
    }
}

impl Mapper for Mapper8 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_bank, chr_bank, irq_pending);

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
            // The top half never switches on this board.
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
                self.chr_bank = value & 0x07;
                self.prg_bank = (value >> 3) & 0x1F;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapper(prg: usize, chr: usize) -> Mapper8 {
        let mut prg_rom = vec![0u8; prg];
        for (i, byte) in prg_rom.iter_mut().enumerate() {
            *byte = (i / 0x4000) as u8;
        }
        let mut chr_rom = vec![0u8; chr];
        for (i, byte) in chr_rom.iter_mut().enumerate() {
            *byte = (i / 0x2000) as u8;
        }
        Mapper8::new(prg_rom, chr_rom)
    }

    #[test]
    fn test_prg_window_and_fixed_top() {
        let mut m = mapper(0x20000, 0x2000); // 8 banks of 16 KB
        assert_eq!(m.read(0x8000), 0);
        assert_eq!(m.read(0xC000), 7, "the top half is fixed to the last bank");

        m.write(0x8000, 0x28); // bits 3-7 = 5
        assert_eq!(m.read(0x8000), 5);
        assert_eq!(m.read(0x9FFF), 5);
        assert_eq!(m.read(0xC000), 7, "switching the window must not move the top");
    }

    #[test]
    fn test_chr_banks() {
        let mut m = mapper(0x8000, 0x8000);
        m.write(0x8000, 0x03);
        assert_eq!(m.read(0x0000), 3);
        assert_eq!(m.chr_bank, 3);
    }

    #[test]
    fn test_chr_ram_is_writable() {
        let mut m = Mapper8::new(vec![0u8; 0x8000], Vec::new());
        assert!(m.chr_is_ram);
        m.write(0x0100, 0x5A);
        assert_eq!(m.read(0x0100), 0x5A);
    }
}
