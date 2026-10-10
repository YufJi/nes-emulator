//! Mapper 66 - GxROM (NES-GNROM, NES-MHROM)
//!
//! Used by games like Doraemon, Dragon Power, Gumshoe, Super Mario Bros. + Duck Hunt.
//! Discrete mapper with 32KB PRG and 8KB CHR bank switching via a single register
//! at $8000-$FFFF.
//!
//! Register at $8000-$FFFF:
//! - Bits 0-1: Select 8KB CHR-ROM bank at $0000-$1FFF
//! - Bits 4-5: Select 32KB PRG-ROM bank at $8000-$FFFF
//! - Bit 6:    Mirroring select (0 = vertical, 1 = horizontal)
//!
//! See https://www.nesdev.org/wiki/GxROM

use super::Mapper;

/// GxROM mapper (Mapper 66)
#[derive(Debug)]
pub struct Mapper66 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Current 32KB PRG-ROM bank (0-3)
    prg_bank: u8,
    /// Current 8KB CHR-ROM bank (0-3)
    chr_bank: u8,
    /// Mirroring select (false = vertical, true = horizontal)
    mirroring_select: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    /// Number of 32KB PRG-ROM banks
    prg_rom_banks: usize,
    /// Number of 8KB CHR-ROM banks
    chr_rom_banks: usize,
    irq_pending: bool,
}

impl Mapper66 {
    /// Create a new Mapper66 instance
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
            mirroring_select: false,
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

impl Mapper for Mapper66 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_bank, chr_bank,
        mirroring_select, prg_rom_banks, chr_rom_banks, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM/RAM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let offset = self.chr_bank_offset();
                let index = offset + (addr as usize);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            // No PRG-RAM on GxROM
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
                // Bits 0-1: CHR bank select (8KB)
                self.chr_bank = value & 0x03;
                // Bits 4-5: PRG bank select (32KB)
                self.prg_bank = (value >> 4) & 0x03;
                // Bit 6: Mirroring select (0 = vertical, 1 = horizontal)
                self.mirroring_select = (value & 0x40) != 0;
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

    /// GxROM mirroring select: 0 = vertical, 1 = horizontal.
    fn mirroring(&self) -> Option<crate::ppu::rendering::Mirroring> {
        Some(if self.mirroring_select {
            crate::ppu::rendering::Mirroring::Horizontal
        } else {
            crate::ppu::rendering::Mirroring::Vertical
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapper66_new() {
        let prg = vec![0u8; 65536]; // 64KB PRG-ROM
        let chr = vec![0u8; 16384]; // 16KB CHR-ROM
        let mapper = Mapper66::new(prg, chr);
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.chr_bank, 0);
        assert!(!mapper.mirroring_select);
        assert!(!mapper.chr_is_ram);
        assert_eq!(mapper.prg_rom_banks, 2);
        assert_eq!(mapper.chr_rom_banks, 2);
    }

    #[test]
    fn test_mapper66_chr_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mapper = Mapper66::new(prg, chr);
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
        let mut mapper = Mapper66::new(prg, chr);

        // Read from bank 0 (default)
        assert_eq!(mapper.read(0x8000), 0);
        assert_eq!(mapper.read(0xFFFF), 0);

        // Switch to bank 1 (bits 4-5 = 0x10)
        mapper.write(0x8000, 0x10);
        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.read(0x8000), 1);
        assert_eq!(mapper.read(0xFFFF), 1);

        // Switch to bank 2 (bits 4-5 = 0x20)
        mapper.write(0x8000, 0x20);
        assert_eq!(mapper.prg_bank, 2);
        assert_eq!(mapper.read(0x8000), 2);

        // Switch to bank 3 (bits 4-5 = 0x30)
        mapper.write(0x8000, 0x30);
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

        let mut mapper = Mapper66::new(prg, chr);

        // Read from bank 0 (default)
        assert_eq!(mapper.read(0x0000), 10);
        assert_eq!(mapper.read(0x1FFF), 10);

        // Switch to CHR bank 1 (bits 0-1 = 1)
        mapper.write(0x8000, 0x01);
        assert_eq!(mapper.chr_bank, 1);
        assert_eq!(mapper.read(0x0000), 11);
        assert_eq!(mapper.read(0x1FFF), 11);

        // Switch to CHR bank 2 (bits 0-1 = 2)
        mapper.write(0x8000, 0x02);
        assert_eq!(mapper.chr_bank, 2);
        assert_eq!(mapper.read(0x0000), 12);

        // Switch to CHR bank 3 (bits 0-1 = 3)
        mapper.write(0x8000, 0x03);
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

        let mut mapper = Mapper66::new(prg, chr);

        // Initial state
        assert_eq!(mapper.read(0x8000), 0x10);
        assert_eq!(mapper.read(0x0000), 0x30);

        // Switch both PRG and CHR banks
        // PRG bank 1 (bits 4-5 = 0x10), CHR bank 1 (bits 0-1 = 1)
        mapper.write(0x8000, 0x11);

        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.chr_bank, 1);
        assert_eq!(mapper.read(0x8000), 0x20);
        assert_eq!(mapper.read(0x0000), 0x40);
    }

    #[test]
    fn test_mirroring_select() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper66::new(prg, chr);

        // Default: mirroring_select = false (vertical)
        assert!(!mapper.mirroring_select);

        // Set bit 6 to select horizontal mirroring
        mapper.write(0x8000, 0x40);
        assert!(mapper.mirroring_select);

        // Clear bit 6 to select vertical mirroring
        mapper.write(0x8000, 0x00);
        assert!(!mapper.mirroring_select);
    }

    #[test]
    fn test_chr_ram_write() {
        let prg = vec![0u8; 32768];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mut mapper = Mapper66::new(prg, chr);

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
        let mut mapper = Mapper66::new(prg, chr);

        // Only bits 4-5 should affect PRG bank
        mapper.write(0x8000, 0xFF); // All bits set
        assert_eq!(mapper.prg_bank, 0x03); // Only bits 4-5 (shifted right)
    }

    #[test]
    fn test_chr_bank_masking() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper66::new(prg, chr);

        // Only bits 0-1 should affect CHR bank
        mapper.write(0x8000, 0xFF); // All bits set
        assert_eq!(mapper.chr_bank, 0x03); // Only bits 0-1
    }

    #[test]
    fn test_mirroring_bit_isolation() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper66::new(prg, chr);

        // Only bit 6 should affect mirroring
        mapper.write(0x8000, 0x40); // Bit 6 set
        assert!(mapper.mirroring_select);

        mapper.write(0x8000, 0x20); // Bit 5 set
        assert!(!mapper.mirroring_select);

        mapper.write(0x8000, 0x80); // Bit 7 set
        assert!(!mapper.mirroring_select);
    }

    #[test]
    fn test_irq_methods() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper66::new(prg, chr);

        // GxROM doesn't generate IRQs
        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }
}
