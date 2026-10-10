//! Mapper 241 - BxROM variant (Hengge Technology)
//!
//! Used by various Hengge Technology titles and educational cartridges.
//! BxROM-like 32 KB PRG bank switching via writes to $8000-$FFFF,
//! with optional battery-backed WRAM at $6000-$7FFF.
//!
//! This is similar to Mapper 34 (BNROM) but may have different
//! bank count or mirroring behavior.
//!
//! Register at $8000-$FFFF:
//! - Bits 0-7: Select 32KB PRG-ROM bank at $8000-$FFFF
//!
//! Uses CHR-RAM (no CHR-ROM bank switching).
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_241

use super::Mapper;

/// BxROM variant (Mapper 241)
#[derive(Debug)]
pub struct Mapper241 {
    prg_rom: Vec<u8>,
    /// 8KB CHR-RAM at $0000-$1FFF
    chr_ram: [u8; 0x2000],
    /// 8KB PRG RAM at $6000-$7FFF (optional, battery-backed on some carts)
    prg_ram: [u8; 0x2000],
    /// Current 32KB PRG-ROM bank
    prg_bank: u8,
    /// Number of 32KB PRG-ROM banks
    prg_rom_banks: usize,
    irq_pending: bool,
}

impl Mapper241 {
    /// Create a new Mapper241 instance
    pub fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>) -> Self {
        let prg_rom_banks = prg_rom.len() / 0x8000;

        Self {
            prg_rom,
            chr_ram: [0u8; 0x2000],
            prg_ram: [0u8; 0x2000],
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

impl Mapper for Mapper241 {
    crate::state::mapper_state!(chr_ram, prg_ram, prg_bank, prg_rom_banks, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-RAM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let index = (addr as usize) % self.chr_ram.len();
                self.chr_ram[index]
            }
            // PRG RAM: $6000-$7FFF (optional)
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index]
            }
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
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index] = value;
            }
            // Mapper register at $8000-$FFFF
            0x8000..=0xFFFF => {
                // Full byte selects PRG bank (32KB)
                self.prg_bank = value;
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
    fn test_mapper241_new() {
        let prg = vec![0u8; 65536]; // 64KB PRG-ROM (2 x 32KB banks)
        let chr = vec![]; // Mapper 241 uses CHR-RAM
        let mapper = Mapper241::new(prg, chr);
        assert_eq!(mapper.prg_rom_banks, 2);
        assert_eq!(mapper.prg_bank, 0);
    }

    #[test]
    fn test_chr_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper241::new(prg, chr);

        // Test CHR RAM read/write
        mapper.write(0x0000, 0x55);
        assert_eq!(mapper.read(0x0000), 0x55);

        mapper.write(0x1FFF, 0x66);
        assert_eq!(mapper.read(0x1FFF), 0x66);
    }

    #[test]
    fn test_prg_ram_read_write() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper241::new(prg, chr);

        // Test PRG RAM read/write at $6000-$7FFF
        mapper.write(0x6000, 0x42);
        assert_eq!(mapper.read(0x6000), 0x42);

        mapper.write(0x7000, 0xAB);
        assert_eq!(mapper.read(0x7000), 0xAB);

        mapper.write(0x7FFF, 0xEF);
        assert_eq!(mapper.read(0x7FFF), 0xEF);
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
        let mut mapper = Mapper241::new(prg, chr);

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
        let mut mapper = Mapper241::new(prg, chr);

        // Any address in $8000-$FFFF should work
        mapper.write(0x9000, 1);
        assert_eq!(mapper.read(0x8000), 1);

        mapper.write(0xBFFF, 0);
        assert_eq!(mapper.read(0x8000), 0);

        mapper.write(0xFFFF, 1);
        assert_eq!(mapper.read(0x8000), 1);
    }

    #[test]
    fn test_full_byte_bank_select() {
        // Mapper 241 uses full byte for bank select (unlike Mapper 34 which masks)
        let mut prg = vec![0u8; 0x200000]; // 2MB (64 x 32KB banks)
        for i in 0..64 {
            prg[i * 0x8000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper241::new(prg, chr);

        // Test that all 8 bits are used
        mapper.write(0x8000, 0x10);
        assert_eq!(mapper.prg_bank, 0x10);
        assert_eq!(mapper.read(0x8000), 0x10);

        mapper.write(0x8000, 0x20);
        assert_eq!(mapper.prg_bank, 0x20);
        assert_eq!(mapper.read(0x8000), 0x20);

        mapper.write(0x8000, 0x3F);
        assert_eq!(mapper.prg_bank, 0x3F);
        assert_eq!(mapper.read(0x8000), 0x3F);
    }

    #[test]
    fn test_bank_wraparound() {
        // Test with smaller ROM
        let mut prg = vec![0u8; 65536]; // 64KB (2 x 32KB banks)
        for i in 0..2 {
            prg[i * 0x8000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper241::new(prg, chr);

        // Bank numbers should wrap around
        mapper.write(0x8000, 2);
        assert_eq!(mapper.prg_bank, 2);
        assert_eq!(mapper.read(0x8000), 0); // 2 % 2 = 0

        mapper.write(0x8000, 3);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read(0x8000), 1); // 3 % 2 = 1

        mapper.write(0x8000, 255);
        assert_eq!(mapper.prg_bank, 255);
        assert_eq!(mapper.read(0x8000), 1); // 255 % 2 = 1
    }

    #[test]
    fn test_comparison_with_mapper34() {
        // Mapper 241 is similar to Mapper 34 (BNROM) but uses full byte for bank select
        // Mapper 34 only uses bits 0-1
        // Mapper 241 uses full byte (0-255)
        let mut prg = vec![0u8; 0x100000]; // 1MB (32 x 32KB banks)
        for i in 0..32 {
            prg[i * 0x8000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper241::new(prg, chr);

        // Write value with bits beyond 0-1 set
        mapper.write(0x8000, 0x04); // Bit 2 set
        assert_eq!(mapper.prg_bank, 0x04);
        assert_eq!(mapper.read(0x8000), 0x04); // Should read bank 4

        mapper.write(0x8000, 0x10); // Bit 4 set
        assert_eq!(mapper.prg_bank, 0x10);
        assert_eq!(mapper.read(0x8000), 0x10); // Should read bank 16
    }

    #[test]
    fn test_irq_methods() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper241::new(prg, chr);

        // Mapper 241 doesn't generate IRQs
        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }
}
