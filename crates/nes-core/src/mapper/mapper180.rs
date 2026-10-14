//! Mapper 180 - UNROM (Crazy Climber variant)
//!
//! Used by Crazy Climber (JPN).
//! This is an inverted UxROM variant: first 16 KB bank fixed at $8000,
//! switchable bank at $C000. Standard UxROM fixes the last bank; this
//! variant uses AND logic instead of OR logic on the bank select lines,
//! producing the opposite fixed-bank behavior.
//!
//! Uses CHR-RAM (no CHR-ROM bank switching).
//!
//! Register at $8000-$FFFF:
//! - Bits 0-3: Select 16KB PRG-ROM bank at $C000-$FFFF
//!
//! PRG-ROM layout:
//! - $8000-$BFFF: Fixed to first 16KB bank (bank 0)
//! - $C000-$FFFF: Switchable 16KB bank
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_180

use super::Mapper;
use crate::prelude::*;

/// Crazy Climber UNROM variant (Mapper 180)
#[derive(Debug)]
pub struct Mapper180 {
    prg_rom: Vec<u8>,
    /// 8KB CHR-RAM at $0000-$1FFF
    chr_ram: [u8; 0x2000],
    /// Current 16KB PRG-ROM bank for $C000-$FFFF
    prg_bank: usize,
    /// Number of 16KB PRG-ROM banks
    prg_rom_banks: usize,
    irq_pending: bool,
}

impl Mapper180 {
    /// Create a new Mapper180 instance
    pub fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>) -> Self {
        let prg_rom_banks = prg_rom.len() / 0x4000;

        Self {
            prg_rom,
            chr_ram: [0u8; 0x2000],
            prg_bank: 0,
            prg_rom_banks: prg_rom_banks.max(1),
            irq_pending: false,
        }
    }

    /// Read from PRG-ROM at the given address
    fn read_prg_rom(&self, addr: u16) -> u8 {
        let bank = if addr < 0xC000 {
            // $8000-$BFFF: fixed to first bank (bank 0)
            0
        } else {
            // $C000-$FFFF: switchable bank
            self.prg_bank
        };

        let offset = (addr & 0x3FFF) as usize;
        let rom_addr = (bank * 0x4000 + offset) % self.prg_rom.len();
        self.prg_rom.get(rom_addr).copied().unwrap_or(0)
    }
}

impl Mapper for Mapper180 {
    crate::state::mapper_state!(chr_ram, prg_bank, prg_rom_banks, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-RAM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let index = addr as usize % self.chr_ram.len();
                self.chr_ram[index]
            }
            // No PRG-RAM on this mapper
            0x6000..=0x7FFF => 0,
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
            // Bank select: $8000-$FFFF
            0x8000..=0xFFFF => {
                // Any write to $8000-$FFFF selects the PRG-ROM bank for $C000-$FFFF
                self.prg_bank = (value as usize) % self.prg_rom_banks;
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
    fn test_mapper180_new() {
        let prg = vec![0u8; 0x40000]; // 256KB (16 x 16KB banks)
        let chr = vec![]; // Mapper 180 uses CHR-RAM, CHR-ROM is ignored
        let mapper = Mapper180::new(prg, chr);
        assert_eq!(mapper.prg_rom_banks, 16);
        assert_eq!(mapper.prg_bank, 0);
    }

    #[test]
    fn test_chr_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper180::new(prg, chr);

        // Test CHR RAM read/write
        mapper.write(0x0000, 0x55);
        assert_eq!(mapper.read(0x0000), 0x55);

        mapper.write(0x1FFF, 0x66);
        assert_eq!(mapper.read(0x1FFF), 0x66);
    }

    #[test]
    fn test_fixed_first_bank() {
        let mut prg = vec![0u8; 0x20000]; // 128KB (8 x 16KB banks)
        for i in 0..8 {
            prg[i * 0x4000] = i as u8;
            prg[i * 0x4000 + 0x3FFF] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper180::new(prg, chr);

        // $8000-$BFFF should always be bank 0 (first bank)
        for bank in 0..8 {
            mapper.write(0x8000, bank as u8);
            assert_eq!(mapper.read(0x8000), 0);
            assert_eq!(mapper.read(0xBFFF), 0);
        }
    }

    #[test]
    fn test_switchable_upper_bank() {
        let mut prg = vec![0u8; 0x20000]; // 128KB (8 x 16KB banks)
        for i in 0..8 {
            prg[i * 0x4000] = i as u8;
            prg[i * 0x4000 + 0x3FFF] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper180::new(prg, chr);

        // $C000-$FFFF should be switchable
        assert_eq!(mapper.read(0xC000), 0);
        assert_eq!(mapper.read(0xFFFF), 0);

        // Switch to bank 3
        mapper.write(0x8000, 3);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read(0xC000), 3);
        assert_eq!(mapper.read(0xFFFF), 3);

        // Switch to bank 7
        mapper.write(0xFFFF, 7);
        assert_eq!(mapper.prg_bank, 7);
        assert_eq!(mapper.read(0xC000), 7);
        assert_eq!(mapper.read(0xFFFF), 7);
    }

    #[test]
    fn test_bank_switching_any_address() {
        let mut prg = vec![0u8; 0x40000];
        for i in 0..16 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper180::new(prg, chr);

        // Any address in $8000-$FFFF should work
        mapper.write(0x9000, 5);
        assert_eq!(mapper.read(0xC000), 5);

        mapper.write(0xBFFF, 2);
        assert_eq!(mapper.read(0xC000), 2);

        mapper.write(0xE000, 10);
        assert_eq!(mapper.read(0xC000), 10);
    }

    #[test]
    fn test_bank_wraparound() {
        // Test with smaller ROM
        let mut prg = vec![0u8; 0x10000]; // 64KB (4 x 16KB banks)
        for i in 0..4 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper180::new(prg, chr);

        // Bank numbers should wrap around
        mapper.write(0x8000, 5);
        assert_eq!(mapper.prg_bank, 1); // 5 % 4 = 1
        assert_eq!(mapper.read(0xC000), 1);

        mapper.write(0x8000, 255);
        assert_eq!(mapper.prg_bank, 3); // 255 % 4 = 3
        assert_eq!(mapper.read(0xC000), 3);
    }

    #[test]
    fn test_opposite_of_uxrom() {
        // This test demonstrates the difference between Mapper 2 (UxROM) and Mapper 180
        // Mapper 2: switchable at $8000, fixed last bank at $C000
        // Mapper 180: fixed first bank at $8000, switchable at $C000
        let mut prg = vec![0u8; 0x10000]; // 64KB (4 x 16KB banks)
        for i in 0..4 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper180::new(prg, chr);

        // With bank 0 selected, both regions should read bank 0
        assert_eq!(mapper.read(0x8000), 0); // Fixed first bank
        assert_eq!(mapper.read(0xC000), 0); // Switchable bank (also 0)

        // Switch to bank 2
        mapper.write(0x8000, 2);

        // $8000 should still be bank 0 (fixed)
        assert_eq!(mapper.read(0x8000), 0);
        // $C000 should now be bank 2 (switchable)
        assert_eq!(mapper.read(0xC000), 2);
    }

    #[test]
    fn test_no_prg_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper180::new(prg, chr);

        // $6000-$7FFF should return 0 (no PRG-RAM)
        mapper.write(0x6000, 0x42);
        assert_eq!(mapper.read(0x6000), 0);
        assert_eq!(mapper.read(0x7FFF), 0);
    }

    #[test]
    fn test_irq_methods() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper180::new(prg, chr);

        // Mapper 180 doesn't generate IRQs
        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }
}
