//! Mapper 38 - PCI556 (UNL-PCI556) - Bit Corp
//!
//! Used by Crime Busters.
//! Nearly identical to GxROM (mapper 66) but the register is at $7000-$7FFF.
//!
//! Register at $7000-$7FFF:
//! - Bits 0-1: Select 32KB PRG-ROM bank at $8000-$FFFF
//! - Bits 2-3: Select 8KB CHR-ROM bank at $0000-$1FFF
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_038

use super::Mapper;
use crate::prelude::*;

/// PCI556 mapper (Mapper 38) - Crime Busters
#[derive(Debug)]
pub struct Mapper38 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Current 32KB PRG-ROM bank (0-3)
    prg_bank: u8,
    /// Current 8KB CHR-ROM bank (0-3)
    chr_bank: u8,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    /// Number of 32KB PRG-ROM banks
    prg_rom_banks: usize,
    /// Number of 8KB CHR-ROM banks
    chr_rom_banks: usize,
    irq_pending: bool,
}

impl Mapper38 {
    /// Create a new Mapper38 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram {
            vec![0u8; 8192] // 8KB CHR-RAM
        } else {
            chr_rom
        };

        let prg_rom_banks = prg_rom.len() / 0x8000;
        let chr_rom_banks = chr_rom.len() / 0x2000;

        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank: 0,
            chr_is_ram,
            prg_rom_banks: prg_rom_banks.max(1),
            chr_rom_banks: chr_rom_banks.max(1),
            irq_pending: false,
        }
    }

    /// Get the current PRG bank offset
    fn prg_bank_offset(&self) -> usize {
        ((self.prg_bank as usize) % self.prg_rom_banks) * 0x8000 // 32KB bank
    }

    /// Get the current CHR bank offset
    fn chr_bank_offset(&self) -> usize {
        ((self.chr_bank as usize) % self.chr_rom_banks) * 0x2000 // 8KB bank
    }
}

impl Mapper for Mapper38 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_bank, chr_bank,
        prg_rom_banks, chr_rom_banks, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM/RAM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let offset = self.chr_bank_offset();
                let index = offset + (addr as usize);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            // No PRG-RAM on this mapper
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
            // CHR-RAM writes (if enabled)
            0x0000..=0x1FFF if self.chr_is_ram => {
                let offset = self.chr_bank_offset();
                let index = offset + (addr as usize);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            // Mapper register at $7000-$7FFF (NOT $8000+!)
            0x7000..=0x7FFF => {
                // Bits 0-1: PRG bank select (32KB)
                self.prg_bank = value & 0x03;
                // Bits 2-3: CHR bank select (8KB)
                self.chr_bank = (value >> 2) & 0x03;
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
    fn test_mapper38_new() {
        let prg = vec![0u8; 65536]; // 64KB PRG-ROM
        let chr = vec![0u8; 16384]; // 16KB CHR-ROM
        let mapper = Mapper38::new(prg, chr);
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.chr_bank, 0);
        assert!(!mapper.chr_is_ram);
        assert_eq!(mapper.prg_rom_banks, 2);
        assert_eq!(mapper.chr_rom_banks, 2);
    }

    #[test]
    fn test_mapper38_chr_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mapper = Mapper38::new(prg, chr);
        assert!(mapper.chr_is_ram);
        assert_eq!(mapper.chr_rom.len(), 8192);
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

        let chr = vec![0u8; 8192];
        let mut mapper = Mapper38::new(prg, chr);

        // Read from bank 0 (default)
        assert_eq!(mapper.read(0x8000), 0);
        assert_eq!(mapper.read(0xFFFF), 0);

        // Switch to bank 1 (bits 0-1 = 1)
        mapper.write(0x7000, 0x01);
        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.read(0x8000), 1);
        assert_eq!(mapper.read(0xFFFF), 1);

        // Switch to bank 2 (bits 0-1 = 2)
        mapper.write(0x7000, 0x02);
        assert_eq!(mapper.prg_bank, 2);
        assert_eq!(mapper.read(0x8000), 2);

        // Switch to bank 3 (bits 0-1 = 3)
        mapper.write(0x7000, 0x03);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read(0x8000), 3);
    }

    #[test]
    fn test_chr_bank_switching() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 32768]; // 32KB (4 x 8KB banks)
                                        // Fill each 8KB bank with unique values
        for bank in 0..4 {
            for i in 0..0x2000 {
                chr[bank * 0x2000 + i] = (bank + 10) as u8;
            }
        }

        let mut mapper = Mapper38::new(prg, chr);

        // Read from bank 0 (default)
        assert_eq!(mapper.read(0x0000), 10);
        assert_eq!(mapper.read(0x1FFF), 10);

        // Switch to CHR bank 1 (bits 2-3 = 0x04)
        mapper.write(0x7000, 0x04);
        assert_eq!(mapper.chr_bank, 1);
        assert_eq!(mapper.read(0x0000), 11);
        assert_eq!(mapper.read(0x1FFF), 11);

        // Switch to CHR bank 2 (bits 2-3 = 0x08)
        mapper.write(0x7000, 0x08);
        assert_eq!(mapper.chr_bank, 2);
        assert_eq!(mapper.read(0x0000), 12);

        // Switch to CHR bank 3 (bits 2-3 = 0x0C)
        mapper.write(0x7000, 0x0C);
        assert_eq!(mapper.chr_bank, 3);
        assert_eq!(mapper.read(0x0000), 13);
    }

    #[test]
    fn test_combined_bank_switching() {
        let mut prg = vec![0u8; 65536]; // 64KB (2 x 32KB banks)
        let mut chr = vec![0u8; 16384]; // 16KB (2 x 8KB banks)

        // Fill PRG banks
        for i in 0..0x8000 {
            prg[i] = 0x10; // Bank 0
            prg[0x8000 + i] = 0x20; // Bank 1
        }

        // Fill CHR banks
        for i in 0..0x2000 {
            chr[i] = 0x30; // Bank 0
            chr[0x2000 + i] = 0x40; // Bank 1
        }

        let mut mapper = Mapper38::new(prg, chr);

        // Initial state
        assert_eq!(mapper.read(0x8000), 0x10);
        assert_eq!(mapper.read(0x0000), 0x30);

        // Switch both PRG and CHR banks
        // PRG bank 1 (bit 0 = 1), CHR bank 1 (bits 2-3 = 0x04)
        mapper.write(0x7000, 0x05);

        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.chr_bank, 1);
        assert_eq!(mapper.read(0x8000), 0x20);
        assert_eq!(mapper.read(0x0000), 0x40);
    }

    #[test]
    fn test_register_at_7000_range() {
        let mut prg = vec![0u8; 65536];
        prg[0x8000] = 0xAA; // Bank 1 start
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper38::new(prg, chr);

        // Writing to $7000 should work
        mapper.write(0x7000, 0x01);
        assert_eq!(mapper.prg_bank, 1);

        // Writing to $7FFF should work
        mapper.write(0x7FFF, 0x00);
        assert_eq!(mapper.prg_bank, 0);

        // Writing to $8000 should NOT affect the mapper
        mapper.write(0x8000, 0x01);
        assert_eq!(mapper.prg_bank, 0); // Should still be 0
    }

    #[test]
    fn test_chr_ram_write() {
        let prg = vec![0u8; 32768];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mut mapper = Mapper38::new(prg, chr);

        assert!(mapper.chr_is_ram);

        // Write to CHR-RAM
        mapper.write(0x0000, 0x55);
        assert_eq!(mapper.read(0x0000), 0x55);

        mapper.write(0x1FFF, 0x66);
        assert_eq!(mapper.read(0x1FFF), 0x66);
    }

    #[test]
    fn test_prg_bank_masking() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper38::new(prg, chr);

        // Only bits 0-1 should affect PRG bank
        mapper.write(0x7000, 0xFF); // All bits set
        assert_eq!(mapper.prg_bank, 0x03); // Only bits 0-1
    }

    #[test]
    fn test_chr_bank_masking() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper38::new(prg, chr);

        // Only bits 2-3 should affect CHR bank
        mapper.write(0x7000, 0xFF); // All bits set
        assert_eq!(mapper.chr_bank, 0x03); // Only bits 2-3 (shifted right)
    }

    #[test]
    fn test_irq_methods() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper38::new(prg, chr);

        // Mapper 38 doesn't generate IRQs
        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }
}
