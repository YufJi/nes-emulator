//! Mapper 0 - NROM
//!
//! The simplest mapper with no bank switching.
//! Supports up to 32KB PRG-ROM and 8KB CHR-ROM (or CHR-RAM).

use super::Mapper;

/// NROM mapper (Mapper 0)
#[derive(Debug)]
pub struct Mapper0 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 8KB PRG RAM at $6000-$7FFF
    prg_ram: [u8; 0x2000],
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper0 {
    /// Create a new Mapper0 instance
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
            prg_ram: [0u8; 0x2000], // 8KB PRG RAM initialized to zero
            chr_is_ram,
            irq_pending: false,
        }
    }

    /// Get PRG-ROM size (16KB or 32KB)
    fn prg_size(&self) -> usize {
        self.prg_rom.len()
    }
}

impl Mapper for Mapper0 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_ram, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index]
            }
            // PRG-ROM: $8000-$FFFF
            0x8000..=0xFFFF => {
                let size = self.prg_size();
                let mask = if size > 16384 { 0x7FFF } else { 0x3FFF };
                let index = ((addr & mask) as usize) % size;
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            // CHR-ROM/RAM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let index = addr as usize % self.chr_rom.len().max(1);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index] = value;
            }
            // CHR-RAM writes (if enabled)
            0x0000..=0x1FFF if self.chr_is_ram => {
                let index = addr as usize % self.chr_rom.len().max(1);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            // PRG-ROM is read-only, writes are ignored
            _ => {}
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    /// Battery-backed work RAM ($6000-$7FFF).
    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    /// Restore battery-backed work RAM.
    fn load_save_ram(&mut self, data: &[u8]) {
        let n = data.len().min(self.prg_ram.len());
        self.prg_ram[..n].copy_from_slice(&data[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapper0_new() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mapper = Mapper0::new(prg, chr);
        assert!(!mapper.chr_is_ram);
    }

    #[test]
    fn test_mapper0_chr_ram() {
        let prg = vec![0u8; 16384];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mapper = Mapper0::new(prg, chr);
        assert!(mapper.chr_is_ram);
        assert_eq!(mapper.chr_rom.len(), 8192);
    }

    #[test]
    fn test_read_write() {
        let prg = vec![0u8; 16384];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper0::new(prg, chr);

        // Test PRG RAM read/write at $6000-$7FFF
        mapper.write(0x6000, 0x42);
        assert_eq!(mapper.read(0x6000), 0x42);

        mapper.write(0x7000, 0xAB);
        assert_eq!(mapper.read(0x7000), 0xAB);

        mapper.write(0x7FFF, 0xEF);
        assert_eq!(mapper.read(0x7FFF), 0xEF);

        // Test PRG RAM at different addresses
        mapper.write(0x6500, 0x12);
        assert_eq!(mapper.read(0x6500), 0x12);
    }

    #[test]
    fn test_prg_rom_mirror() {
        // Test 16KB PRG ROM mirroring (loaded at both $8000 and $C000)
        let mut prg = vec![0u8; 16384];
        prg[0] = 0xAA; // First byte
        prg[16383] = 0xBB; // Last byte
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper0::new(prg, chr);

        // Read from $8000 (start of PRG ROM)
        assert_eq!(mapper.read(0x8000), 0xAA);

        // Read from $C000 (mirror of $8000 for 16KB ROM)
        assert_eq!(mapper.read(0xC000), 0xAA);

        // Read from end of first 16KB
        assert_eq!(mapper.read(0xBFFF), 0xBB);

        // Read from end of second 16KB (mirror)
        assert_eq!(mapper.read(0xFFFF), 0xBB);
    }

    #[test]
    fn test_32k_prg_rom_no_mirror() {
        // Test 32KB PRG ROM (no mirroring within $8000-$FFFF)
        let mut prg = vec![0u8; 32768];
        prg[0] = 0x11; // First byte of first 16KB
        prg[16384] = 0x22; // First byte of second 16KB
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper0::new(prg, chr);

        // Read from $8000 (start of first 16KB)
        assert_eq!(mapper.read(0x8000), 0x11);

        // Read from $C000 (start of second 16KB, not a mirror)
        assert_eq!(mapper.read(0xC000), 0x22);
    }

    #[test]
    fn test_chr_access() {
        // Test CHR ROM read
        let prg = vec![0u8; 16384];
        let mut chr = vec![0u8; 8192];
        chr[0] = 0x33;
        chr[8191] = 0x44;
        let mut mapper = Mapper0::new(prg, chr);

        assert!(!mapper.chr_is_ram);
        assert_eq!(mapper.read(0x0000), 0x33);
        assert_eq!(mapper.read(0x1FFF), 0x44);
    }

    #[test]
    fn test_chr_ram_write() {
        // Test CHR RAM write
        let prg = vec![0u8; 16384];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mut mapper = Mapper0::new(prg, chr);

        assert!(mapper.chr_is_ram);

        // Write to CHR RAM
        mapper.write(0x0000, 0x55);
        assert_eq!(mapper.read(0x0000), 0x55);

        mapper.write(0x1FFF, 0x66);
        assert_eq!(mapper.read(0x1FFF), 0x66);
    }

    #[test]
    fn test_save_state_keeps_chr_ram() {
        use crate::state::{StateReader, StateWriter};

        let mut mapper = Mapper0::new(vec![0u8; 0x4000], vec![]); // CHR-RAM
        mapper.write(0x0000, 0x5A);

        let mut w = StateWriter::new();
        mapper.save_state(&mut w);
        let saved = w.into_bytes();

        mapper.write(0x0000, 0x00);
        let mut r = StateReader::new(&saved);
        mapper.load_state(&mut r);
        assert_eq!(mapper.read(0x0000), 0x5A, "CHR-RAM was not restored");
    }

    #[test]
    fn test_save_state_leaves_chr_rom_out() {
        use crate::state::{StateReader, StateWriter};

        let mut mapper = Mapper0::new(vec![0u8; 0x4000], vec![0x11u8; 8192]); // CHR-ROM
        assert!(!mapper.chr_is_ram);

        let mut w = StateWriter::new();
        mapper.save_state(&mut w);
        let saved = w.into_bytes();

        // Stand in for a different cartridge's CHR image.
        mapper.chr_rom[0] = 0x22;
        let mut r = StateReader::new(&saved);
        mapper.load_state(&mut r);
        assert_eq!(
            mapper.chr_rom[0], 0x22,
            "CHR-ROM must not be part of a save state"
        );
    }
}
