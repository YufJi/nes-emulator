//! Mapper 65 - Irem H3001
//!
//! Used by games such as Daiku no Gen-san and Kaiketsu Yanchamaru 3.
//!
//! Registers ($8000-$FFFF):
//! - $8000: 8KB PRG bank at $8000-$9FFF
//! - $9000: mirroring, latched when bits 0-2 are 1, 3 or 4 (bit 7: 0 = vertical)
//! - $A000: 8KB PRG bank at $A000-$BFFF
//! - $B000-$B007: 1KB CHR banks for $0000-$1FFF
//! - $C000-$FFFF: IRQ control (not used by the games on this board)
//!
//! $C000-$FFFF is hard-wired to the last 8KB PRG bank.
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_065

use super::Mapper;
use crate::ppu::rendering::Mirroring;

/// Irem H3001 mapper (Mapper 65)
#[derive(Debug)]
pub struct Mapper65 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    /// 8KB PRG bank mapped at $8000
    prg_bank_8000: u8,
    /// 8KB PRG bank mapped at $A000
    prg_bank_a000: u8,
    /// Eight 1KB CHR banks covering $0000-$1FFF
    chr_banks: [u8; 8],
    /// Nametable mirroring selected through $9000
    mirroring: Mirroring,
    /// Number of 8KB PRG-ROM banks
    prg_banks: usize,
    /// Number of 1KB CHR-ROM banks
    chr_banks_count: usize,
    irq_pending: bool,
}

impl Mapper65 {
    /// Create a new Mapper65 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram {
            vec![0u8; 8192]
        } else {
            chr_rom
        };

        Self {
            prg_banks: (prg_rom.len() / 0x2000).max(1),
            chr_banks_count: (chr_rom.len() / 0x400).max(1),
            prg_rom,
            chr_rom,
            chr_is_ram,
            prg_bank_8000: 0,
            prg_bank_a000: 0,
            chr_banks: [0; 8],
            mirroring: Mirroring::Vertical,
            irq_pending: false,
        }
    }

    /// Byte offset of an 8KB PRG bank, clamped to the available ROM.
    fn prg_offset(&self, bank: u8) -> usize {
        (bank as usize % self.prg_banks) * 0x2000
    }

    /// Byte offset of a 1KB CHR bank, clamped to the available ROM.
    fn chr_offset(&self, bank: u8) -> usize {
        (bank as usize % self.chr_banks_count) * 0x400
    }
}

impl Mapper for Mapper65 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_bank_8000, prg_bank_a000,
        chr_banks, mirroring, prg_banks, chr_banks_count, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_banks[slot]);
                self.chr_rom.get(offset + (addr as usize & 0x3FF)).copied().unwrap_or(0)
            }
            0x6000..=0x7FFF => 0,
            0x8000..=0x9FFF => {
                let offset = self.prg_offset(self.prg_bank_8000);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            0xA000..=0xBFFF => {
                let offset = self.prg_offset(self.prg_bank_a000);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            0xC000..=0xFFFF => {
                let offset = self.prg_offset((self.prg_banks - 1) as u8);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // CHR-RAM writes
            0x0000..=0x1FFF if self.chr_is_ram => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_banks[slot]);
                let index = offset + (addr as usize & 0x3FF);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            0x8000..=0x8FFF => self.prg_bank_8000 = value,
            0x9000..=0x9FFF => {
                // Only the specific sub-addresses $9001/$9003/$9004 latch the
                // mirroring; other writes are ignored.
                if matches!(value & 0x07, 1 | 3 | 4) {
                    self.mirroring = if value & 0x80 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            0xA000..=0xAFFF => self.prg_bank_a000 = value,
            0xB000..=0xBFFF => {
                let slot = (addr as usize) & 0x07;
                self.chr_banks[slot] = value;
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

    /// Mirroring is set by the board rather than the header.
    fn mirroring(&self) -> Option<Mirroring> {
        Some(self.mirroring)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mapper() -> Mapper65 {
        let mut prg = vec![0u8; 8 * 0x2000];
        for bank in 0..8 {
            prg[bank * 0x2000] = bank as u8;
        }
        let mut chr = vec![0u8; 8 * 0x400];
        for bank in 0..8 {
            chr[bank * 0x400] = 0x10 + bank as u8;
        }
        Mapper65::new(prg, chr)
    }

    #[test]
    fn test_prg_banks() {
        let mut mapper = make_mapper();

        // $C000 is fixed to the last bank.
        assert_eq!(mapper.read(0xC000), 7);

        mapper.write(0x8000, 2);
        mapper.write(0xA000, 5);
        assert_eq!(mapper.read(0x8000), 2);
        assert_eq!(mapper.read(0xA000), 5);
    }

    #[test]
    fn test_chr_banks() {
        let mut mapper = make_mapper();

        mapper.write(0xB000, 3); // slot 0
        mapper.write(0xB001, 6); // slot 1
        assert_eq!(mapper.read(0x0000), 0x13);
        assert_eq!(mapper.read(0x0400), 0x16);
    }

    #[test]
    fn test_mirroring_latch_requires_specific_values() {
        let mut mapper = make_mapper();
        assert_eq!(mapper.mirroring(), Some(Mirroring::Vertical));

        // Values other than 1/3/4 do not latch.
        mapper.write(0x9000, 0x02);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Vertical));

        mapper.write(0x9000, 0x81);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Horizontal));

        mapper.write(0x9000, 0x04);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Vertical));
    }
}
