//! Mapper 7 - AxROM (NES-AMROM, NES-ANROM, NES-AOROM)
//!
//! Used by games like Battletoads, Marble Madness, Wizards & Warriors.
//! Features:
//! - 32KB PRG-ROM bank switching (bits 0-2)
//! - Single-screen nametable mirroring selectable (bit 4)
//! - 8KB CHR-RAM (no CHR bank switching)
//!
//! Register at $8000-$FFFF:
//! - Bits 0-2: Select 32KB PRG-ROM bank at $8000
//! - Bit 4:    Select nametable (0 = $2000, 1 = $2400)
//!
//! See https://www.nesdev.org/wiki/AxROM

use crate::prelude::*;
use super::Mapper;

/// AxROM mapper (Mapper 7)
#[derive(Debug)]
pub struct Mapper7 {
    prg_rom: Vec<u8>,
    /// 8KB CHR-RAM
    chr_ram: [u8; 0x2000],
    /// Current 32KB PRG-ROM bank (0-7)
    prg_bank: u8,
    /// Single-screen mirroring selection (false = $2000, true = $2400)
    mirroring_select: bool,
    irq_pending: bool,
}

impl Mapper7 {
    /// Create a new Mapper7 instance
    pub fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>) -> Self {
        // AxROM always uses CHR-RAM, ignore any CHR-ROM
        Self {
            prg_rom,
            chr_ram: [0u8; 0x2000],
            prg_bank: 0,
            mirroring_select: false,
            irq_pending: false,
        }
    }

    /// Get the current PRG bank offset
    fn prg_bank_offset(&self) -> usize {
        (self.prg_bank as usize) * 0x8000 // 32KB bank
    }
}

impl Mapper for Mapper7 {
    crate::state::mapper_state!(chr_ram, prg_bank, mirroring_select, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-RAM: $0000-$1FFF
            0x0000..=0x1FFF => {
                let index = (addr as usize) % self.chr_ram.len();
                self.chr_ram[index]
            }
            // No PRG-RAM on AxROM
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
                // Bits 0-2: PRG bank select
                self.prg_bank = value & 0x07;
                // Bit 4: Mirroring select (0 = first screen, 1 = second screen)
                self.mirroring_select = (value & 0x10) != 0;
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

    /// AxROM selects one 1KB nametable at a time.
    fn mirroring(&self) -> Option<crate::ppu::rendering::Mirroring> {
        Some(if self.mirroring_select {
            crate::ppu::rendering::Mirroring::SingleScreenUpper
        } else {
            crate::ppu::rendering::Mirroring::SingleScreen
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapper7_new() {
        let prg = vec![0u8; 65536]; // 64KB PRG-ROM
        let chr = vec![];
        let mapper = Mapper7::new(prg, chr);
        assert_eq!(mapper.prg_bank, 0);
        assert!(!mapper.mirroring_select);
    }

    #[test]
    fn test_prg_bank_switching() {
        let mut prg = vec![0u8; 65536]; // 64KB (2 x 32KB banks)
        // Fill first 32KB with 0x11
        for i in 0..0x8000 {
            prg[i] = 0x11;
        }
        // Fill second 32KB with 0x22
        for i in 0x8000..0x10000 {
            prg[i] = 0x22;
        }

        let chr = vec![];
        let mut mapper = Mapper7::new(prg, chr);

        // Read from bank 0 (default)
        assert_eq!(mapper.read(0x8000), 0x11);
        assert_eq!(mapper.read(0xFFFF), 0x11);

        // Switch to bank 1
        mapper.write(0x8000, 0x01);
        assert_eq!(mapper.prg_bank, 1);
        assert_eq!(mapper.read(0x8000), 0x22);
        assert_eq!(mapper.read(0xFFFF), 0x22);

        // Switch back to bank 0
        mapper.write(0x8000, 0x00);
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.read(0x8000), 0x11);
    }

    #[test]
    fn test_mirroring_select() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper7::new(prg, chr);

        // Default: mirroring_select = false
        assert!(!mapper.mirroring_select);

        // Set bit 4 to select second screen
        mapper.write(0x8000, 0x10);
        assert!(mapper.mirroring_select);

        // Clear bit 4
        mapper.write(0x8000, 0x00);
        assert!(!mapper.mirroring_select);
    }

    #[test]
    fn test_chr_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper7::new(prg, chr);

        // Write to CHR-RAM
        mapper.write(0x0000, 0xAA);
        mapper.write(0x1FFF, 0xBB);

        // Read from CHR-RAM
        assert_eq!(mapper.read(0x0000), 0xAA);
        assert_eq!(mapper.read(0x1FFF), 0xBB);
    }

    #[test]
    fn test_prg_bank_masking() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper7::new(prg, chr);

        // Only bits 0-2 should affect PRG bank
        mapper.write(0x8000, 0xFF); // All bits set
        assert_eq!(mapper.prg_bank, 0x07); // Only bits 0-2

        mapper.write(0x8000, 0x08); // Bit 3 set
        assert_eq!(mapper.prg_bank, 0x00); // Should not affect bank
    }

    #[test]
    fn test_irq_methods() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper7::new(prg, chr);

        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }
}
