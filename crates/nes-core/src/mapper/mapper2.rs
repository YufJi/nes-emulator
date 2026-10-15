//! Mapper 2 - UxROM (UNROM/UOROM)
//!
//! Used by games like Mega Man, Castlevania, Contra, Duck Tales, Metal Gear.
//! 16 KB switchable PRG-ROM bank at $8000, last 16 KB bank fixed at $C000.
//! Uses CHR-RAM (no CHR-ROM bank switching).
//! See https://www.nesdev.org/wiki/UxROM

use super::Mapper;
use crate::prelude::*;

/// UxROM mapper (Mapper 2)
#[derive(Debug)]
pub struct Mapper2 {
    prg_rom: Vec<u8>,
    /// 8KB CHR-RAM at $0000-$1FFF (UxROM uses CHR-RAM, not CHR-ROM)
    chr_ram: [u8; 0x2000],
    /// 8KB PRG RAM at $6000-$7FFF (optional, not all UxROM carts have this)
    prg_ram: [u8; 0x2000],
    /// Current PRG-ROM bank selected for $8000-$BFFF
    prg_bank: usize,
    /// Number of 16KB PRG-ROM banks
    prg_rom_banks: usize,
    /// IRQ pending flag (UxROM doesn't generate IRQs)
    irq_pending: bool,
}

impl Mapper2 {
    /// Create a new Mapper2 instance
    pub fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>) -> Self {
        let prg_rom_banks = prg_rom.len() / 0x4000;

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

impl Mapper for Mapper2 {
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

    /// UxROM: the ROM's data lines are on the CPU's bus, so a bank write
    /// carries the AND of what the CPU drove and what the ROM holds at that
    /// address.
    fn has_bus_conflicts(&self) -> bool {
        true
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
                // Any write to $8000-$FFFF selects the PRG-ROM bank
                // Only lower 4-7 bits are used depending on ROM size
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

    /// A UxROM board's ROM data lines are on the CPU's bus, so a bank write is
    /// masked by what the ROM holds at the address the write lands on. This is
    /// the board's own claim; the bus applies it (`NesBus::write_now`).
    #[test]
    fn test_uxrom_reports_bus_conflicts() {
        let mapper = Mapper2::new(vec![0u8; 0x40000], vec![]);
        assert!(mapper.has_bus_conflicts());
    }

    #[test]
    fn test_mapper2_new() {
        let prg = vec![0u8; 0x40000]; // 256KB (16 x 16KB banks)
        let chr = vec![]; // UxROM uses CHR-RAM, CHR-ROM is ignored
        let mapper = Mapper2::new(prg, chr);
        assert_eq!(mapper.prg_rom_banks, 16);
        assert_eq!(mapper.prg_bank, 0);
    }

    #[test]
    fn test_chr_ram() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mut mapper = Mapper2::new(prg, chr);

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
        let mut mapper = Mapper2::new(prg, chr);

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
        let mut prg = vec![0u8; 0x40000]; // 256KB (16 x 16KB banks)
                                          // Mark different banks
        for i in 0..16 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper2::new(prg, chr);

        // Initial state: bank 0 at $8000, last bank at $C000
        assert_eq!(mapper.read(0x8000), 0);
        assert_eq!(mapper.read(0xC000), 15);

        // Switch to bank 3
        mapper.write(0x8000, 3);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read(0x8000), 3);
        // $C000 should still be last bank
        assert_eq!(mapper.read(0xC000), 15);

        // Switch to bank 7
        mapper.write(0xFFFF, 7);
        assert_eq!(mapper.prg_bank, 7);
        assert_eq!(mapper.read(0x8000), 7);
        assert_eq!(mapper.read(0xC000), 15);

        // Switch to bank 15 (same as fixed bank)
        mapper.write(0x8000, 15);
        assert_eq!(mapper.read(0x8000), 15);
        assert_eq!(mapper.read(0xC000), 15);
    }

    #[test]
    fn test_bank_switching_any_address() {
        let mut prg = vec![0u8; 0x40000];
        for i in 0..16 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper2::new(prg, chr);

        // Any address in $8000-$FFFF should work
        mapper.write(0x9000, 5);
        assert_eq!(mapper.read(0x8000), 5);

        mapper.write(0xBFFF, 2);
        assert_eq!(mapper.read(0x8000), 2);

        mapper.write(0xE000, 10);
        assert_eq!(mapper.read(0x8000), 10);
    }

    #[test]
    fn test_bank_wraparound() {
        // Test with smaller ROM
        let mut prg = vec![0u8; 0x10000]; // 64KB (4 x 16KB banks)
        for i in 0..4 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper2::new(prg, chr);

        // Bank numbers should wrap around
        mapper.write(0x8000, 5);
        assert_eq!(mapper.prg_bank, 1); // 5 % 4 = 1
        assert_eq!(mapper.read(0x8000), 1);

        mapper.write(0x8000, 255);
        assert_eq!(mapper.prg_bank, 3); // 255 % 4 = 3
        assert_eq!(mapper.read(0x8000), 3);
    }

    #[test]
    fn test_fixed_last_bank() {
        let mut prg = vec![0u8; 0x20000]; // 128KB (8 x 16KB banks)
        for i in 0..8 {
            prg[i * 0x4000] = i as u8;
            prg[i * 0x4000 + 0x3FFF] = i as u8;
        }
        let chr = vec![];
        let mut mapper = Mapper2::new(prg, chr);

        // $C000-$FFFF should always be bank 7 (last bank)
        for bank in 0..8 {
            mapper.write(0x8000, bank as u8);
            assert_eq!(mapper.read(0xC000), 7);
            assert_eq!(mapper.read(0xFFFF), 7);
        }
    }

    #[test]
    fn test_irq_not_pending() {
        let prg = vec![0u8; 32768];
        let chr = vec![];
        let mapper = Mapper2::new(prg, chr);

        // UxROM doesn't generate IRQs
        assert!(!mapper.irq_pending());
    }
}
