//! Mapper 79 - NINA-003
//!
//! Used by games from American Video Entertainment (AVE) like
//! Deathbots, Puzzle, and Krazy Kreatures.
//!
//! Features:
//! - 32KB fixed PRG-ROM at $8000-$FFFF
//! - 8KB switchable CHR-ROM bank at $0000-$1FFF
//! - Mirroring control
//!
//! The unusual aspect of this mapper is that the register is at
//! $4100-$5FFF (in the expansion area), not the typical $8000+ range.
//!
//! Register format ($4100-$5FFF):
//! - Bits 0-3: Select 8KB CHR-ROM bank
//! - Bit 4: Mirroring (0=vertical, 1=horizontal)
//! - Bits 5-7: Unused
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_079

use crate::prelude::*;
use super::Mapper;

/// NINA-003 mapper (Mapper 79)
#[derive(Debug)]
pub struct Mapper79 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    /// Current CHR-ROM bank selected (8KB banks)
    chr_bank: usize,
    /// Number of 8KB CHR-ROM banks
    chr_rom_banks: usize,
    /// Current mirroring mode (0=vertical, 1=horizontal)
    mirroring: u8,
    /// IRQ pending flag (NINA-003 doesn't generate IRQs)
    irq_pending: bool,
}

impl Mapper79 {
    /// Create a new Mapper79 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram {
            vec![0u8; 8192] // 8KB CHR-RAM
        } else {
            chr_rom
        };

        let chr_rom_banks = chr_rom.len() / 0x2000;

        Self {
            prg_rom,
            chr_rom,
            chr_is_ram,
            chr_bank: 0,
            chr_rom_banks: chr_rom_banks.max(1),
            mirroring: 0,
            irq_pending: false,
        }
    }

    /// Read from PRG-ROM at the given address
    fn read_prg_rom(&self, addr: u16) -> u8 {
        // NINA-003 has fixed 32KB PRG-ROM
        // $8000-$FFFF maps directly to PRG-ROM
        let size = self.prg_rom.len();
        let index = ((addr & 0x7FFF) as usize) % size.max(1);
        self.prg_rom.get(index).copied().unwrap_or(0)
    }

    /// Physical CHR address the current 8KB bank maps `addr` to.
    fn chr_addr(&self, addr: u16) -> usize {
        let offset = addr as usize % 0x2000;
        (self.chr_bank * 0x2000 + offset) % self.chr_rom.len().max(1)
    }

    /// Read from CHR-ROM/RAM at the given address
    fn read_chr(&self, addr: u16) -> u8 {
        self.chr_rom.get(self.chr_addr(addr)).copied().unwrap_or(0)
    }

    /// Write to CHR-RAM.
    ///
    /// NINA-003 boards ship CHR-ROM, where writes are dropped; this only has
    /// an effect when the header leaves CHR empty so the image doubles as RAM.
    fn write_chr(&mut self, addr: u16, value: u8) {
        if !self.chr_is_ram {
            return;
        }
        let chr_addr = self.chr_addr(addr);
        if let Some(byte) = self.chr_rom.get_mut(chr_addr) {
            *byte = value;
        }
    }
}

impl Mapper for Mapper79 {
    crate::state::mapper_state!(chr_rom, chr_bank, chr_rom_banks, mirroring, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM/RAM: $0000-$1FFF
            0x0000..=0x1FFF => self.read_chr(addr),
            // PRG-ROM: $8000-$FFFF
            0x8000..=0xFFFF => self.read_prg_rom(addr),
            // Open bus for other addresses
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // $0000-$1FFF: CHR-RAM uploads land on the cartridge image; on
            // CHR-ROM boards the mapper drops them.
            0x0000..=0x1FFF => self.write_chr(addr, value),
            // Register: $4100-$5FFF
            0x4100..=0x5FFF => {
                // Bits 0-3: CHR bank select
                self.chr_bank = ((value & 0x0F) as usize) % self.chr_rom_banks;
                // Bit 4: Mirroring (0=vertical, 1=horizontal)
                self.mirroring = (value >> 4) & 0x01;
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
    fn test_mapper79_new() {
        let prg = vec![0u8; 32768]; // 32KB
        let chr = vec![0u8; 0x10000]; // 64KB (8 x 8KB banks)
        let mapper = Mapper79::new(prg, chr);
        assert_eq!(mapper.chr_rom_banks, 8);
        assert_eq!(mapper.chr_bank, 0);
        assert_eq!(mapper.mirroring, 0);
    }

    #[test]
    fn test_chr_bank_switching() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 0x10000]; // 64KB (8 x 8KB banks)
        // Mark different banks
        for i in 0..8 {
            chr[i * 0x2000] = i as u8;
        }
        let mut mapper = Mapper79::new(prg, chr);

        // Initial state: bank 0
        assert_eq!(mapper.read(0x0000), 0);

        // Switch to bank 3 via $4100
        mapper.write(0x4100, 3);
        assert_eq!(mapper.chr_bank, 3);
        assert_eq!(mapper.read(0x0000), 3);

        // Switch to bank 7 via $5FFF
        mapper.write(0x5FFF, 7);
        assert_eq!(mapper.chr_bank, 7);
        assert_eq!(mapper.read(0x0000), 7);

        // Switch to bank 0
        mapper.write(0x5000, 0);
        assert_eq!(mapper.read(0x0000), 0);
    }

    #[test]
    fn test_chr_bank_switching_different_addresses() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 0x10000];
        for i in 0..8 {
            chr[i * 0x2000] = i as u8;
        }
        let mut mapper = Mapper79::new(prg, chr);

        // Any address in $4100-$5FFF should work
        mapper.write(0x4100, 1);
        assert_eq!(mapper.read(0x0000), 1);

        mapper.write(0x4500, 2);
        assert_eq!(mapper.read(0x0000), 2);

        mapper.write(0x5000, 5);
        assert_eq!(mapper.read(0x0000), 5);

        mapper.write(0x5FFF, 6);
        assert_eq!(mapper.read(0x0000), 6);
    }

    #[test]
    fn test_chr_bank_wraparound() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 0x4000]; // 16KB (2 x 8KB banks)
        chr[0] = 0;
        chr[0x2000] = 1;
        let mut mapper = Mapper79::new(prg, chr);

        assert_eq!(mapper.chr_rom_banks, 2);

        // Bank numbers should wrap around (only bits 0-3 are used)
        mapper.write(0x4100, 2);
        assert_eq!(mapper.chr_bank, 0); // 2 % 2 = 0
        assert_eq!(mapper.read(0x0000), 0);

        mapper.write(0x4100, 5);
        assert_eq!(mapper.chr_bank, 1); // 5 % 2 = 1
        assert_eq!(mapper.read(0x0000), 1);

        mapper.write(0x4100, 15);
        assert_eq!(mapper.chr_bank, 1); // 15 % 2 = 1
        assert_eq!(mapper.read(0x0000), 1);
    }

    #[test]
    fn test_prg_rom_fixed() {
        let mut prg = vec![0u8; 0x8000]; // 32KB
        prg[0] = 0xAA;
        prg[0x4000] = 0xBB;
        prg[0x7FFF] = 0xCC;
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper79::new(prg, chr);

        // PRG-ROM should be fixed
        assert_eq!(mapper.read(0x8000), 0xAA);
        assert_eq!(mapper.read(0xC000), 0xBB);
        assert_eq!(mapper.read(0xFFFF), 0xCC);

        // Writing to $4100 should NOT affect PRG-ROM, only CHR bank
        mapper.write(0x4100, 1);
        assert_eq!(mapper.read(0x8000), 0xAA);
    }

    #[test]
    fn test_mirroring_control() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 0x10000]; // 64KB (8 banks) to allow bank 5
        let mut mapper = Mapper79::new(prg, chr);

        // Default mirroring should be 0 (vertical)
        assert_eq!(mapper.mirroring, 0);

        // Set horizontal mirroring (bit 4 = 1)
        mapper.write(0x4100, 0x10);
        assert_eq!(mapper.mirroring, 1);

        // Set vertical mirroring (bit 4 = 0)
        mapper.write(0x4100, 0x00);
        assert_eq!(mapper.mirroring, 0);

        // Set both bank and mirroring
        mapper.write(0x4100, 0x15); // bank 5, horizontal
        assert_eq!(mapper.chr_bank, 5);
        assert_eq!(mapper.mirroring, 1);
    }

    #[test]
    fn test_register_range_only() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 0x10000];
        for i in 0..8 {
            chr[i * 0x2000] = i as u8;
        }
        let mut mapper = Mapper79::new(prg, chr);

        // Writing outside $4100-$5FFF should not affect CHR bank
        mapper.write(0x4000, 5);
        assert_eq!(mapper.chr_bank, 0);

        mapper.write(0x6000, 5);
        assert_eq!(mapper.chr_bank, 0);

        mapper.write(0x8000, 5);
        assert_eq!(mapper.chr_bank, 0);

        // Writing inside $4100-$5FFF should work
        mapper.write(0x4100, 3);
        assert_eq!(mapper.chr_bank, 3);
    }

    #[test]
    fn test_irq_not_pending() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mapper = Mapper79::new(prg, chr);

        // NINA-003 doesn't generate IRQs
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_chr_ram_write_is_visible_to_reads() {
        let prg = vec![0u8; 32768];
        let mut mapper = Mapper79::new(prg, Vec::new()); // Empty CHR -> CHR-RAM
        assert!(mapper.chr_is_ram);

        mapper.write(0x0100, 0x5A);
        assert_eq!(mapper.read(0x0100), 0x5A);

        mapper.write(0x1FFF, 0xA5);
        assert_eq!(mapper.read(0x1FFF), 0xA5);
    }

    #[test]
    fn test_chr_rom_write_is_ignored() {
        let prg = vec![0u8; 32768];
        let chr = vec![0x11u8; 0x2000];
        let mut mapper = Mapper79::new(prg, chr);
        assert!(!mapper.chr_is_ram);

        mapper.write(0x0100, 0x5A);
        assert_eq!(mapper.read(0x0100), 0x11);
    }

    #[test]
    fn test_save_state_keeps_chr_ram() {
        use crate::state::{StateReader, StateWriter};

        let prg = vec![0u8; 32768];
        let mut mapper = Mapper79::new(prg, Vec::new()); // Empty CHR -> CHR-RAM
        mapper.write(0x0100, 0x5A);

        let mut w = StateWriter::new();
        mapper.save_state(&mut w);
        let saved = w.into_bytes();

        mapper.write(0x0100, 0x00);
        let mut r = StateReader::new(&saved);
        mapper.load_state(&mut r);
        assert_eq!(mapper.read(0x0100), 0x5A, "CHR-RAM was not restored");
    }
}
