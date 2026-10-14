//! Mapper 34 - BNROM (NES-BNROM) / NINA-001
//!
//! Used by games like Deadly Towers (Mashou), Darkseed.
//! Simple 32KB PRG-ROM bank switching via writes to $8000-$FFFF.
//! Uses CHR-RAM (no CHR-ROM bank switching).
//!
//! Register at $8000-$FFFF:
//! - Bits 0-1: Select 32KB PRG-ROM bank at $8000-$FFFF
//!
//! Note: iNES mapper 34 also covers NINA-001; this implementation handles BNROM only.
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_034

use super::Mapper;
use crate::prelude::*;

/// BNROM mapper (Mapper 34)
#[derive(Debug)]
pub struct Mapper34 {
    prg_rom: Vec<u8>,
    /// 8KB CHR-RAM at $0000-$1FFF
    chr_ram: [u8; 0x2000],
    /// Current 32KB PRG-ROM bank (0-3)
    prg_bank: u8,
    /// Number of 32KB PRG-ROM banks
    prg_rom_banks: usize,
    irq_pending: bool,
}

impl Mapper34 {
    /// Create a new Mapper34 instance
    pub fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>) -> Self {
        let prg_rom_banks = prg_rom.len() / 0x8000;

        Self {
            prg_rom,
            chr_ram: [0u8; 0x2000],
            prg_bank: 0,
            prg_rom_banks: prg_rom_banks.max(1),
            irq_pending: false,
        }
    }

    /// Get the current PRG bank offset
    fn prg_bank_offset(&self) -> usize {
        ((self.prg_bank as usize) % self.prg_rom_banks) * 0x8000 // 32KB bank
    }
}

impl Mapper for Mapper34 {
    crate::state::mapper_state!(chr_ram, prg_bank, prg_rom_banks, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-RAM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let index = (addr as usize) % self.chr_ram.len();
                self.chr_ram[index]
            }
            // No PRG-RAM on BNROM
            0x6000..=0x7FFF => 0,
            // PRG-ROM: $8000-$FFFF (32KB switchable bank)
            0x8000..=0xFFFF => {
                let offset = self.prg_bank_offset();
                let index = offset + ((addr - 0x8000) as usize);
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // CHR-RAM writes
            0x0000..=0x1FFF => {
                let index = (addr as usize) % self.chr_ram.len();
                self.chr_ram[index] = value;
            }
            // Mapper register at $8000-$FFFF
            0x8000..=0xFFFF => {
                // Bits 0-1: PRG bank select (32KB)
                self.prg_bank = value & 0x03;
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

    #[test]
    fn test_mapper34_new() {
        let prg = vec![0u8; 65536]; // 64KB PRG-ROM (2 x 32KB banks)
        let chr = vec![]; // BNROM uses CHR-RAM, CHR-ROM is ignored
        let mapper = Mapper34::new(prg, chr);
        assert_eq!(mapper.prg_rom_banks, 2);
        assert_eq!(mapper.prg_bank, 0);
    }

    #[test]
    fn test_chr_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper34::new(prg, chr);

        // Test CHR RAM read/write
        mapper.write(0x0000, 0x55);
        assert_eq!(mapper.read(0x0000), 0x55);

        mapper.write(0x1FFF, 0x66);
        assert_eq!(mapper.read(0x1FFF), 0x66);
    }

    #[test]
    fn test_prg_bank_switching() {
        let mut prg = vec![0u8; 131072]; // 128KB (4 x 32KB banks)
                                         // Fill each 32KB bank with unique values
        for bank in 0..4 {
            for i in 0..0x8000 {
                prg[bank * 0x8000 + i] = bank as u8;
            }
        }

        let chr = vec![];
        let mut mapper = Mapper34::new(prg, chr);

        // Read from bank 0 (default)
        assert_eq!(mapper.read(0x8000), 0);
        assert_eq!(mapper.read(0xFFFF), 0);

        // Switch to bank 1
        mapper.write(0x8000, 0x01);
        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.read(0x8000), 1);
        assert_eq!(mapper.read(0xFFFF), 1);

        // Switch to bank 2
        mapper.write(0x8000, 0x02);
        assert_eq!(mapper.prg_bank, 2);
        assert_eq!(mapper.read(0x8000), 2);

        // Switch to bank 3
        mapper.write(0x8000, 0x03);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read(0x8000), 3);
    }

    #[test]
    fn test_bank_switching_any_address() {
        let mut prg = vec![0u8; 65536];
        for i in 0..2 {
            prg[i * 0x8000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper34::new(prg, chr);

        // Any address in $8000-$FFFF should work
        mapper.write(0x9000, 1);
        assert_eq!(mapper.read(0x8000), 1);

        mapper.write(0xBFFF, 0);
        assert_eq!(mapper.read(0x8000), 0);

        mapper.write(0xFFFF, 1);
        assert_eq!(mapper.read(0x8000), 1);
    }

    #[test]
    fn test_bank_wraparound() {
        // Test with smaller ROM
        let mut prg = vec![0u8; 65536]; // 64KB (2 x 32KB banks)
        for i in 0..2 {
            prg[i * 0x8000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper34::new(prg, chr);

        // Bank numbers should wrap around
        mapper.write(0x8000, 2);
        assert_eq!(mapper.prg_bank, 2);
        assert_eq!(mapper.read(0x8000), 0); // 2 % 2 = 0

        mapper.write(0x8000, 3);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read(0x8000), 1); // 3 % 2 = 1
    }

    #[test]
    fn test_prg_bank_masking() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper34::new(prg, chr);

        // Only bits 0-1 should affect PRG bank
        mapper.write(0x8000, 0xFF); // All bits set
        assert_eq!(mapper.prg_bank, 0x03); // Only bits 0-1

        mapper.write(0x8000, 0x04); // Bit 2 set
        assert_eq!(mapper.prg_bank, 0x00); // Should not affect bank
    }

    #[test]
    fn test_irq_methods() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper34::new(prg, chr);

        // BNROM doesn't generate IRQs
        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }
}
