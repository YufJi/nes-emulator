//! Mapper 32 - Irem G-101
//!
//! Used by Image Fight and a handful of other Irem titles.
//!
//! Registers ($8000-$FFFF):
//! - $8000-$8007: 8KB PRG banks for $8000, $A000 and $C000
//! - $9000-$9007: 1KB CHR banks for $0000-$1FFF
//! - $A000: bit 0 mirroring (0 = vertical, 1 = horizontal),
//!   bit 1 swaps the $8000 and $C000 banks
//!
//! $E000-$FFFF is hard-wired to the last 8KB PRG bank.
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_032

use super::Mapper;
use crate::ppu::rendering::Mirroring;
use crate::prelude::*;

/// Irem G-101 mapper (Mapper 32)
#[derive(Debug)]
pub struct Mapper32 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_is_ram: bool,
    /// 8KB PRG banks for $8000, $A000 and $C000
    prg_banks: [u8; 3],
    /// Eight 1KB CHR banks covering $0000-$1FFF
    chr_banks: [u8; 8],
    /// Nametable mirroring
    mirroring: Mirroring,
    /// Whether $8000 and $C000 swap banks
    swap_mode: bool,
    prg_bank_count: usize,
    chr_bank_count: usize,
    irq_pending: bool,
}

impl Mapper32 {
    /// Create a new Mapper32 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };

        Self {
            prg_bank_count: (prg_rom.len() / 0x2000).max(1),
            chr_bank_count: (chr_rom.len() / 0x400).max(1),
            prg_rom,
            chr_rom,
            chr_is_ram,
            prg_banks: [0, 1, 2],
            chr_banks: [0; 8],
            mirroring: Mirroring::Vertical,
            swap_mode: false,
            irq_pending: false,
        }
    }

    fn prg_offset(&self, bank: u8) -> usize {
        (bank as usize % self.prg_bank_count) * 0x2000
    }

    fn chr_offset(&self, bank: u8) -> usize {
        (bank as usize % self.chr_bank_count) * 0x400
    }

    /// Bank mapped at $8000, honouring the swap mode.
    fn lower_bank(&self) -> usize {
        if self.swap_mode {
            self.prg_banks[2] as usize
        } else {
            self.prg_banks[0] as usize
        }
    }

    /// Bank mapped at $C000, honouring the swap mode.
    fn upper_bank(&self) -> usize {
        if self.swap_mode {
            self.prg_banks[0] as usize
        } else {
            self.prg_banks[2] as usize
        }
    }
}

impl Mapper for Mapper32 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_banks, chr_banks,
        mirroring, swap_mode, prg_bank_count, chr_bank_count, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_banks[slot]);
                self.chr_rom
                    .get(offset + (addr as usize & 0x3FF))
                    .copied()
                    .unwrap_or(0)
            }
            0x6000..=0x7FFF => 0,
            0x8000..=0x9FFF => {
                let offset = self.prg_offset(self.lower_bank() as u8);
                self.prg_rom
                    .get(offset + (addr as usize & 0x1FFF))
                    .copied()
                    .unwrap_or(0)
            }
            0xA000..=0xBFFF => {
                let offset = self.prg_offset(self.prg_banks[1]);
                self.prg_rom
                    .get(offset + (addr as usize & 0x1FFF))
                    .copied()
                    .unwrap_or(0)
            }
            0xC000..=0xFFFF if addr < 0xE000 => {
                let offset = self.prg_offset(self.upper_bank() as u8);
                self.prg_rom
                    .get(offset + (addr as usize & 0x1FFF))
                    .copied()
                    .unwrap_or(0)
            }
            0xE000..=0xFFFF => {
                let offset = self.prg_offset((self.prg_bank_count - 1) as u8);
                self.prg_rom
                    .get(offset + (addr as usize & 0x1FFF))
                    .copied()
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_banks[slot]);
                let index = offset + (addr as usize & 0x3FF);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            0x8000..=0x8007 => {
                let slot = (addr as usize) & 0x03;
                if slot < 3 {
                    self.prg_banks[slot] = value & 0x1F;
                }
            }
            0x9000..=0x9007 => {
                let slot = (addr as usize) & 0x07;
                self.chr_banks[slot] = value;
            }
            0xA000..=0xAFFF => {
                self.mirroring = if value & 0x01 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
                self.swap_mode = value & 0x02 != 0;
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
        Some(self.mirroring)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mapper() -> Mapper32 {
        let mut prg = vec![0u8; 8 * 0x2000];
        for bank in 0..8 {
            prg[bank * 0x2000] = bank as u8;
        }
        let mut chr = vec![0u8; 8 * 0x400];
        for bank in 0..8 {
            chr[bank * 0x400] = 0x20 + bank as u8;
        }
        Mapper32::new(prg, chr)
    }

    #[test]
    fn test_prg_banking_and_swap() {
        let mut mapper = make_mapper();

        mapper.write(0x8000, 3);
        mapper.write(0x8001, 4);
        mapper.write(0x8002, 5);
        assert_eq!(mapper.read(0x8000), 3);
        assert_eq!(mapper.read(0xA000), 4);
        assert_eq!(mapper.read(0xC000), 5);
        assert_eq!(mapper.read(0xE000), 7, "$E000 is fixed to the last bank");

        // Swap mode exchanges $8000 and $C000.
        mapper.write(0xA000, 0x02);
        assert_eq!(mapper.read(0x8000), 5);
        assert_eq!(mapper.read(0xC000), 3);
    }

    #[test]
    fn test_chr_and_mirroring() {
        let mut mapper = make_mapper();

        mapper.write(0x9000, 2);
        mapper.write(0x9007, 6);
        assert_eq!(mapper.read(0x0000), 0x22);
        assert_eq!(mapper.read(0x1C00), 0x26);

        mapper.write(0xA000, 0x01);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Horizontal));
    }
}
