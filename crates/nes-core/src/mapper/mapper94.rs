//! Mapper 94 - Senjou no Ookami (UNROM variant)
//!
//! Used by the game "Senjou no Ookami" (Commando) for Famicom.
//! This is a variant of UxROM (Mapper 2) with a limited bank count.
//!
//! Features:
//! - 16KB switchable PRG-ROM bank at $8000-$BFFF
//! - Fixed last 16KB PRG-ROM bank at $C000-$FFFF
//! - 8KB CHR-RAM at $0000-$1FFF
//! - Only uses bits 0-2 for bank selection (maximum 8 banks = 128KB)
//!
//! The key difference from Mapper 2 is that Mapper 94 only uses
//! the lower 3 bits of the bank register, limiting it to 8 banks
//! (128KB total PRG-ROM). This is a cost-reduced board design.
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_094

use super::Mapper;
use crate::prelude::*;

/// Senjou no Ookami mapper (Mapper 94)
#[derive(Debug)]
pub struct Mapper94 {
    prg_rom: Vec<u8>,
    /// 8KB CHR-RAM at $0000-$1FFF
    chr_ram: [u8; 0x2000],
    /// 8KB PRG RAM at $6000-$7FFF
    prg_ram: [u8; 0x2000],
    /// Current PRG-ROM bank selected for $8000-$BFFF
    prg_bank: usize,
    /// Number of 16KB PRG-ROM banks (max 8 for this mapper)
    prg_rom_banks: usize,
    /// IRQ pending flag (Mapper 94 doesn't generate IRQs)
    irq_pending: bool,
}

impl Mapper94 {
    /// Create a new Mapper94 instance
    pub fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>) -> Self {
        let prg_rom_banks = (prg_rom.len() / 0x4000).min(8); // Max 8 banks

        Self {
            prg_rom,
            chr_ram: [0u8; 0x2000],
            prg_ram: [0u8; 0x2000],
            prg_bank: 0,
            prg_rom_banks: prg_rom_banks.max(1),
            irq_pending: false,
        }
    }

    /// Read from PRG-ROM at the given address
    fn read_prg_rom(&self, addr: u16) -> u8 {
        let bank = if addr < 0xC000 {
            // $8000-$BFFF: switchable bank
            self.prg_bank
        } else {
            // $C000-$FFFF: fixed to last bank
            self.prg_rom_banks.saturating_sub(1)
        };

        let offset = (addr & 0x3FFF) as usize;
        let rom_addr = (bank * 0x4000 + offset) % self.prg_rom.len();
        self.prg_rom.get(rom_addr).copied().unwrap_or(0)
    }
}

impl Mapper for Mapper94 {
    crate::state::mapper_state!(chr_ram, prg_ram, prg_bank, prg_rom_banks, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-RAM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let index = addr as usize % self.chr_ram.len();
                self.chr_ram[index]
            }
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index]
            }
            // PRG-ROM: $8000-$FFFF
            0x8000..=0xFFFF => self.read_prg_rom(addr),
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // CHR-RAM writes
            0x0000..=0x1FFF => {
                let index = addr as usize % self.chr_ram.len();
                self.chr_ram[index] = value;
            }
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index] = value;
            }
            // Bank select: $8000-$FFFF
            0x8000..=0xFFFF => {
                // Only bits 0-2 are used (8 banks max)
                let bank = (value & 0x07) as usize;
                self.prg_bank = bank % self.prg_rom_banks;
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
    fn test_mapper94_new() {
        let prg = vec![0u8; 0x20000]; // 128KB (8 x 16KB banks, max for Mapper 94)
        let chr = vec![]; // Mapper 94 uses CHR-RAM
        let mapper = Mapper94::new(prg, chr);
        assert_eq!(mapper.prg_rom_banks, 8);
        assert_eq!(mapper.prg_bank, 0);
    }

    #[test]
    fn test_mapper94_larger_rom_clamped() {
        // Test that larger ROMs are clamped to 8 banks
        let prg = vec![0u8; 0x40000]; // 256KB (16 x 16KB banks)
        let chr = vec![];
        let mapper = Mapper94::new(prg, chr);
        // Should be clamped to 8 banks
        assert_eq!(mapper.prg_rom_banks, 8);
    }

    #[test]
    fn test_chr_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper94::new(prg, chr);

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
        let mut mapper = Mapper94::new(prg, chr);

        // Test PRG RAM read/write at $6000-$7FFF
        mapper.write(0x6000, 0x42);
        assert_eq!(mapper.read(0x6000), 0x42);

        mapper.write(0x7000, 0xAB);
        assert_eq!(mapper.read(0x7000), 0xAB);

        mapper.write(0x7FFF, 0xEF);
        assert_eq!(mapper.read(0x7FFF), 0xEF);
    }

    #[test]
    fn test_bank_switching() {
        let mut prg = vec![0u8; 0x20000]; // 128KB (8 x 16KB banks)
                                          // Mark different banks
        for i in 0..8 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper94::new(prg, chr);

        // Initial state: bank 0 at $8000, last bank at $C000
        assert_eq!(mapper.read(0x8000), 0);
        assert_eq!(mapper.read(0xC000), 7);

        // Switch to bank 3
        mapper.write(0x8000, 3);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read(0x8000), 3);
        // $C000 should still be last bank
        assert_eq!(mapper.read(0xC000), 7);

        // Switch to bank 7
        mapper.write(0xFFFF, 7);
        assert_eq!(mapper.prg_bank, 7);
        assert_eq!(mapper.read(0x8000), 7);
        assert_eq!(mapper.read(0xC000), 7);
    }

    #[test]
    fn test_bank_switching_any_address() {
        let mut prg = vec![0u8; 0x20000];
        for i in 0..8 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper94::new(prg, chr);

        // Any address in $8000-$FFFF should work
        mapper.write(0x9000, 5);
        assert_eq!(mapper.read(0x8000), 5);

        mapper.write(0xBFFF, 2);
        assert_eq!(mapper.read(0x8000), 2);

        mapper.write(0xE000, 6);
        assert_eq!(mapper.read(0x8000), 6);
    }

    #[test]
    fn test_bank_only_uses_bits_0_2() {
        // Test that only bits 0-2 are used for bank selection
        let mut prg = vec![0u8; 0x20000]; // 128KB (8 x 16KB banks)
        for i in 0..8 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper94::new(prg, chr);

        // Writing 0x08 (1000) should select bank 0 (only bits 0-2 matter)
        mapper.write(0x8000, 0x08);
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.read(0x8000), 0);

        // Writing 0x0B (1011) should select bank 3
        mapper.write(0x8000, 0x0B);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read(0x8000), 3);

        // Writing 0xFF (11111111) should select bank 7
        mapper.write(0x8000, 0xFF);
        assert_eq!(mapper.prg_bank, 7);
        assert_eq!(mapper.read(0x8000), 7);

        // Writing 0x10 (10000) should select bank 0
        mapper.write(0x8000, 0x10);
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.read(0x8000), 0);
    }

    #[test]
    fn test_fixed_last_bank() {
        let mut prg = vec![0u8; 0x10000]; // 64KB (4 x 16KB banks)
        for i in 0..4 {
            prg[i * 0x4000] = i as u8;
            prg[i * 0x4000 + 0x3FFF] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper94::new(prg, chr);

        // $C000-$FFFF should always be bank 3 (last bank)
        for bank in 0..4 {
            mapper.write(0x8000, bank as u8);
            assert_eq!(mapper.read(0xC000), 3);
            assert_eq!(mapper.read(0xFFFF), 3);
        }
    }

    #[test]
    fn test_irq_not_pending() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mapper = Mapper94::new(prg, chr);

        // Mapper 94 doesn't generate IRQs
        assert!(!mapper.irq_pending());
    }
}
