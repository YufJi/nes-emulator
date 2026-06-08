//! Mapper 11 - Color Dreams (unlicensed discrete mapper)
//!
//! Used by games like Bible Adventures, Crystal Mines, Chiller, Metal Fighter.
//! This is a simple mapper with a single register at $8000-$FFFF.
//!
//! Register at $8000-$FFFF:
//! - Bits 0-3: Select 32KB PRG-ROM bank (mapped to $8000-$FFFF)
//! - Bits 4-7: Select 8KB CHR-ROM bank (mapped to $0000-$1FFF)
//!
//! See https://www.nesdev.org/wiki/Color_Dreams

use super::Mapper;

/// Color Dreams mapper (Mapper 11)
#[derive(Debug)]
pub struct Mapper11 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Current 32KB PRG-ROM bank (0-15)
    prg_bank: u8,
    /// Current 8KB CHR-ROM bank (0-15)
    chr_bank: u8,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper11 {
    /// Create a new Mapper11 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram {
            vec![0u8; 8192] // 8KB CHR-RAM
        } else {
            chr_rom
        };

        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank: 0,
            chr_is_ram,
            irq_pending: false,
        }
    }

    /// Get the current PRG bank offset
    fn prg_bank_offset(&self) -> usize {
        (self.prg_bank as usize) * 0x8000 // 32KB bank
    }

    /// Get the current CHR bank offset
    fn chr_bank_offset(&self) -> usize {
        (self.chr_bank as usize) * 0x2000 // 8KB bank
    }
}

impl Mapper for Mapper11 {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM/RAM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let offset = self.chr_bank_offset();
                let index = offset + (addr as usize);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            // No PRG-RAM on Color Dreams
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
            // Mapper register at $8000-$FFFF
            0x8000..=0xFFFF => {
                // Bits 0-3: PRG bank select (32KB)
                self.prg_bank = value & 0x0F;
                // Bits 4-7: CHR bank select (8KB)
                self.chr_bank = (value >> 4) & 0x0F;
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
    fn test_mapper11_new() {
        let prg = vec![0u8; 65536]; // 64KB PRG-ROM
        let chr = vec![0u8; 16384]; // 16KB CHR-ROM
        let mapper = Mapper11::new(prg, chr);
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.chr_bank, 0);
        assert!(!mapper.chr_is_ram);
    }

    #[test]
    fn test_mapper11_chr_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mapper = Mapper11::new(prg, chr);
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
        let mut mapper = Mapper11::new(prg, chr);

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
    fn test_chr_bank_switching() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 32768]; // 32KB (4 x 8KB banks)
        // Fill each 8KB bank with unique values
        for bank in 0..4 {
            for i in 0..0x2000 {
                chr[bank * 0x2000 + i] = (bank + 10) as u8;
            }
        }

        let mut mapper = Mapper11::new(prg, chr);

        // Read from bank 0 (default)
        assert_eq!(mapper.read(0x0000), 10);
        assert_eq!(mapper.read(0x1FFF), 10);

        // Switch to CHR bank 1 (bits 4-7 = 0x10)
        mapper.write(0x8000, 0x10);
        assert_eq!(mapper.chr_bank, 1);
        assert_eq!(mapper.read(0x0000), 11);
        assert_eq!(mapper.read(0x1FFF), 11);

        // Switch to CHR bank 2 (bits 4-7 = 0x20)
        mapper.write(0x8000, 0x20);
        assert_eq!(mapper.chr_bank, 2);
        assert_eq!(mapper.read(0x0000), 12);

        // Switch to CHR bank 3 (bits 4-7 = 0x30)
        mapper.write(0x8000, 0x30);
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

        let mut mapper = Mapper11::new(prg, chr);

        // Initial state
        assert_eq!(mapper.read(0x8000), 0x10);
        assert_eq!(mapper.read(0x0000), 0x30);

        // Switch both PRG and CHR banks
        // PRG bank 1 (bit 0 = 1), CHR bank 1 (bit 4 = 0x10)
        mapper.write(0x8000, 0x11);

        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.chr_bank, 1);
        assert_eq!(mapper.read(0x8000), 0x20);
        assert_eq!(mapper.read(0x0000), 0x40);
    }

    #[test]
    fn test_chr_ram_write() {
        let prg = vec![0u8; 32768];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mut mapper = Mapper11::new(prg, chr);

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
        let mut mapper = Mapper11::new(prg, chr);

        // Only bits 0-3 should affect PRG bank
        mapper.write(0x8000, 0xFF); // All bits set
        assert_eq!(mapper.prg_bank, 0x0F); // Only bits 0-3
    }

    #[test]
    fn test_chr_bank_masking() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper11::new(prg, chr);

        // Only bits 4-7 should affect CHR bank
        mapper.write(0x8000, 0xFF); // All bits set
        assert_eq!(mapper.chr_bank, 0x0F); // Only bits 4-7 (shifted right)
    }

    #[test]
    fn test_irq_methods() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper11::new(prg, chr);

        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }
}
