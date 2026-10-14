//! Mapper 87 - Jaleco J87 (JF-0x, JF-10)
//!
//! The first board Jaleco sold as a "mapper": 32 KB of PRG fixed in place and
//! 8 KB CHR bank switching, written through a register anywhere in
//! $6000-$7FFF. `iNES Mapper 101` is the same board with the CHR bank bits
//! reversed; mapper 140 is its later upgrade.
//!
//! Register ($6000-$7FFF):
//! - Bit 0: bit 1 of the CHR bank (only meaningful on the 32 KB CHR boards)
//! - Bit 1: bit 0 of the CHR bank
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_087>

use super::Mapper;
use crate::prelude::*;

/// J87 mapper (Mapper 87)
#[derive(Debug)]
pub struct Mapper87 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 8 KB CHR bank selected by the register
    chr_bank: u8,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper87 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            chr_bank: 0,
            chr_is_ram,
            irq_pending: false,
        }
    }

    /// Offset of the selected 8 KB CHR bank.
    fn chr_offset(&self) -> usize {
        let banks = (self.chr_rom.len() / 0x2000).max(1);
        (self.chr_bank as usize % banks) * 0x2000
    }
}

impl Mapper for Mapper87 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, chr_bank, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset() + (addr as usize);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            // Fixed PRG. A 16 KB cartridge mirrors the one bank across both
            // halves, which the modulo does on its own.
            0x8000..=0xFFFF => {
                let offset = (addr - 0x8000) as usize;
                self.prg_rom
                    .get(offset % self.prg_rom.len().max(1))
                    .copied()
                    .unwrap_or(0)
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
            0x6000..=0x7FFF => {
                // Bit 0 is the high bit of the bank, bit 1 the low one.
                self.chr_bank = ((value & 0x01) << 1) | ((value >> 1) & 0x01);
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

    fn mapper(prg: usize, chr: usize) -> Mapper87 {
        let mut prg_rom = vec![0u8; prg];
        for (i, byte) in prg_rom.iter_mut().enumerate() {
            *byte = (i / 0x4000) as u8;
        }
        let mut chr_rom = vec![0u8; chr];
        for (i, byte) in chr_rom.iter_mut().enumerate() {
            *byte = (i / 0x2000) as u8;
        }
        Mapper87::new(prg_rom, chr_rom)
    }

    #[test]
    fn test_chr_bank_bits_are_swapped() {
        let mut m = mapper(0x8000, 0x8000); // 4 CHR banks
        m.write(0x6000, 0x02); // bit 1 -> bank 1
        assert_eq!(m.read(0x0000), 1);
        m.write(0x6000, 0x01); // bit 0 -> bank 2
        assert_eq!(m.read(0x0000), 2);
        m.write(0x6000, 0x03); // both -> bank 3
        assert_eq!(m.read(0x0000), 3);
    }

    #[test]
    fn test_16k_prg_mirrors() {
        let mut m = mapper(0x4000, 0x2000);
        assert_eq!(m.read(0x8000), 0);
        assert_eq!(m.read(0xC000), 0, "a 16 KB cartridge mirrors the same bank");
    }

    #[test]
    fn test_32k_prg_is_fixed() {
        let mut m = mapper(0x8000, 0x2000);
        assert_eq!(m.read(0x8000), 0);
        assert_eq!(m.read(0xC000), 1);
    }
}
