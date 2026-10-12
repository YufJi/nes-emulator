//! Mapper 240 - C&E (Jing Ke Xin Zhuan / Sheng Huo Lie Zhuan PCBs)
//!
//! Used by Jing Ke Xin Zhuan, Sheng Huo Lie Zhuan.
//! Register at $4020-$5FFF: upper nibble selects 32 KB PRG bank,
//! lower nibble selects 8 KB CHR bank.
//!
//! This is an unusual mapper because it uses the $4020-$5FFF address range
//! which is typically unmapped or used for expansion audio.
//!
//! Register at $4020-$5FFF:
//! - Bits 0-3: Select 8KB CHR-ROM bank at $0000-$1FFF
//! - Bits 4-5: Select 32KB PRG-ROM bank at $8000-$FFFF
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_240

use crate::prelude::*;
use super::Mapper;

/// C&E mapper (Mapper 240)
#[derive(Debug)]
pub struct Mapper240 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Current 32KB PRG-ROM bank (0-3)
    prg_bank: u8,
    /// Current 8KB CHR-ROM bank (0-15)
    chr_bank: u8,
    /// Number of 32KB PRG-ROM banks
    prg_rom_banks: usize,
    /// Number of 8KB CHR-ROM banks
    chr_rom_banks: usize,
    irq_pending: bool,
}

impl Mapper240 {
    /// Create a new Mapper240 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let prg_rom_banks = prg_rom.len() / 0x8000;
        let chr_rom_banks = chr_rom.len() / 0x2000;

        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank: 0,
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

impl Mapper for Mapper240 {
    crate::state::mapper_state!(
        chr_rom,
        prg_bank,
        chr_bank,
        prg_rom_banks,
        chr_rom_banks,
        irq_pending,
    );

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let offset = self.chr_bank_offset();
                let index = offset + (addr as usize);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            // $4020-$5FFF: Register area (no memory here)
            0x4020..=0x5FFF => 0,
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
            // Mapper register at $4020-$5FFF
            0x4020..=0x5FFF => {
                // Bits 0-3: CHR bank select (8KB)
                self.chr_bank = value & 0x0F;
                // Bits 4-5: PRG bank select (32KB)
                self.prg_bank = (value >> 4) & 0x03;
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
    fn test_mapper240_new() {
        let prg = vec![0u8; 65536]; // 64KB PRG-ROM (2 x 32KB banks)
        let chr = vec![0u8; 16384]; // 16KB CHR-ROM (2 x 8KB banks)
        let mapper = Mapper240::new(prg, chr);
        assert_eq!(mapper.prg_rom_banks, 2);
        assert_eq!(mapper.chr_rom_banks, 2);
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.chr_bank, 0);
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
        let mut mapper = Mapper240::new(prg, chr);

        // Read from bank 0 (default)
        assert_eq!(mapper.read(0x8000), 0);
        assert_eq!(mapper.read(0xFFFF), 0);

        // Switch to PRG bank 1 (bits 4-5 = 0x10)
        mapper.write(0x4020, 0x10);
        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.read(0x8000), 1);
        assert_eq!(mapper.read(0xFFFF), 1);

        // Switch to PRG bank 2 (bits 4-5 = 0x20)
        mapper.write(0x5000, 0x20);
        assert_eq!(mapper.prg_bank, 2);
        assert_eq!(mapper.read(0x8000), 2);

        // Switch to PRG bank 3 (bits 4-5 = 0x30)
        mapper.write(0x5FFF, 0x30);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read(0x8000), 3);
    }

    #[test]
    fn test_chr_bank_switching() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 131072]; // 128KB (16 x 8KB banks)
        // Fill each 8KB bank with unique values
        for bank in 0..16 {
            for i in 0..0x2000 {
                chr[bank * 0x2000 + i] = (bank + 10) as u8;
            }
        }

        let mut mapper = Mapper240::new(prg, chr);

        // Read from bank 0 (default)
        assert_eq!(mapper.read(0x0000), 10);
        assert_eq!(mapper.read(0x1FFF), 10);

        // Switch to CHR bank 1 (bits 0-3 = 0x01)
        mapper.write(0x4020, 0x01);
        assert_eq!(mapper.chr_bank, 1);
        assert_eq!(mapper.read(0x0000), 11);
        assert_eq!(mapper.read(0x1FFF), 11);

        // Switch to CHR bank 15 (bits 0-3 = 0x0F)
        mapper.write(0x5000, 0x0F);
        assert_eq!(mapper.chr_bank, 15);
        assert_eq!(mapper.read(0x0000), 25);
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

        let mut mapper = Mapper240::new(prg, chr);

        // Initial state
        assert_eq!(mapper.read(0x8000), 0x10);
        assert_eq!(mapper.read(0x0000), 0x30);

        // Switch both PRG and CHR banks
        // PRG bank 1 (bit 4 = 0x10), CHR bank 1 (bit 0 = 0x01)
        mapper.write(0x4020, 0x11);

        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.chr_bank, 1);
        assert_eq!(mapper.read(0x8000), 0x20);
        assert_eq!(mapper.read(0x0000), 0x40);
    }

    #[test]
    fn test_register_at_any_address_in_range() {
        let mut prg = vec![0u8; 65536];
        for i in 0..2 {
            prg[i * 0x8000] = i as u8;
        }
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper240::new(prg, chr);

        // Any address in $4020-$5FFF should work
        // Writing 0x01 sets CHR bank to 1, PRG bank stays 0
        mapper.write(0x4020, 0x01);
        assert_eq!(mapper.read(0x8000), 0);

        // Writing 0x10 sets PRG bank to 1 (bits 4-5)
        mapper.write(0x5000, 0x10);
        assert_eq!(mapper.read(0x8000), 1);

        // Writing 0x11 sets both banks to 1
        mapper.write(0x5FFF, 0x11);
        assert_eq!(mapper.read(0x8000), 1);
    }

    #[test]
    fn test_write_outside_register_range() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper240::new(prg, chr);

        // Writes outside $4020-$5FFF should not affect banks
        mapper.write(0x401F, 0x11); // Just before range
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.chr_bank, 0);

        mapper.write(0x6000, 0x11); // Just after range
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.chr_bank, 0);

        mapper.write(0x8000, 0x11); // PRG-ROM area
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.chr_bank, 0);
    }

    #[test]
    fn test_prg_bank_wraparound() {
        // Test with smaller ROM
        let mut prg = vec![0u8; 65536]; // 64KB (2 x 32KB banks)
        for i in 0..2 {
            prg[i * 0x8000] = i as u8;
        }
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper240::new(prg, chr);

        // Bank numbers should wrap around (only bits 4-5 used)
        mapper.write(0x4020, 0x40); // Bit 6 set, but bits 4-5 = 0
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.read(0x8000), 0);

        mapper.write(0x4020, 0x50); // Bits 4-5 = 1
        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.read(0x8000), 1);
    }

    #[test]
    fn test_chr_bank_wraparound() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 16384]; // 16KB (2 x 8KB banks)
        for i in 0..2 {
            chr[i * 0x2000] = i as u8;
        }
        let mut mapper = Mapper240::new(prg, chr);

        // CHR bank should wrap around
        mapper.write(0x4020, 0x02);
        assert_eq!(mapper.chr_bank, 2);
        assert_eq!(mapper.read(0x0000), 0); // 2 % 2 = 0

        mapper.write(0x4020, 0x03);
        assert_eq!(mapper.chr_bank, 3);
        assert_eq!(mapper.read(0x0000), 1); // 3 % 2 = 1
    }

    #[test]
    fn test_unusual_address_range() {
        // This mapper uses $4020-$5FFF which is unusual
        // Verify that the range boundaries work correctly
        let mut prg = vec![0u8; 65536]; // 64KB (2 x 32KB banks)
        prg[0x0000] = 0xAA; // First byte of bank 0
        prg[0x8000] = 0xBB; // First byte of bank 1
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper240::new(prg, chr);

        // Start of range - write 0x10 to set PRG bank 1 (bits 4-5)
        mapper.write(0x4020, 0x10);
        assert_eq!(mapper.read(0x8000), 0xBB);

        // End of range - write 0x00 to set PRG bank 0
        mapper.write(0x5FFF, 0x00);
        assert_eq!(mapper.read(0x8000), 0xAA);
    }

    #[test]
    fn test_irq_methods() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper240::new(prg, chr);

        // Mapper 240 doesn't generate IRQs
        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }
}
