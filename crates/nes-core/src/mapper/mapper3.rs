//! Mapper 3 - CNROM
//!
//! Used by games like Solomon's Key, Arkanoid, Arkista's Ring, Bump 'n' Jump.
//! Fixed PRG-ROM (up to 32 KB), with switchable 8 KB CHR-ROM banks.
//! See https://www.nesdev.org/wiki/INES_Mapper_003

use super::Mapper;

/// CNROM mapper (Mapper 3)
#[derive(Debug)]
pub struct Mapper3 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// Current CHR-ROM bank selected (8KB banks)
    chr_bank: usize,
    /// Number of 8KB CHR-ROM banks
    chr_rom_banks: usize,
    /// IRQ pending flag (CNROM doesn't generate IRQs)
    irq_pending: bool,
}

impl Mapper3 {
    /// Create a new Mapper3 instance
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
            chr_bank: 0,
            chr_rom_banks: chr_rom_banks.max(1),
            irq_pending: false,
        }
    }

    /// Read from PRG-ROM at the given address
    fn read_prg_rom(&self, addr: u16) -> u8 {
        // CNROM has fixed PRG-ROM (16KB or 32KB)
        // $8000-$FFFF maps directly to PRG-ROM with mirroring for 16KB
        let size = self.prg_rom.len();
        let mask = if size > 16384 { 0x7FFF } else { 0x3FFF };
        let index = ((addr & mask) as usize) % size;
        self.prg_rom.get(index).copied().unwrap_or(0)
    }

    /// Read from CHR-ROM at the given address
    fn read_chr(&self, addr: u16) -> u8 {
        let offset = addr as usize % 0x2000;
        let chr_addr = (self.chr_bank * 0x2000 + offset) % self.chr_rom.len();
        self.chr_rom.get(chr_addr).copied().unwrap_or(0)
    }

    /// Write to CHR-RAM (if present)
    fn write_chr(&mut self, addr: u16, value: u8) {
        // CNROM uses CHR-ROM, but some variants may have CHR-RAM
        // We'll allow writes if the CHR data is modifiable
        if self.chr_rom.len() == 8192 && self.chr_rom_banks == 1 {
            // Likely CHR-RAM
            let index = addr as usize % self.chr_rom.len();
            if index < self.chr_rom.len() {
                // Note: In a real implementation, we'd need to track if this is RAM
                // For now, we don't allow writes to CHR-ROM
            }
        }
    }
}

impl Mapper for Mapper3 {
    crate::state::mapper_state!(chr_rom, chr_bank, chr_rom_banks, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM: $0000-$1FFF
            0x0000..=0x1FFF => self.read_chr(addr),
            // PRG-ROM: $8000-$FFFF
            0x8000..=0xFFFF => self.read_prg_rom(addr),
            // Open bus for other addresses
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // CHR-ROM is read-only, but some variants may have CHR-RAM
            0x0000..=0x1FFF => {
                self.write_chr(addr, value);
            }
            // Bank select: $8000-$FFFF
            0x8000..=0xFFFF => {
                // Any write to $8000-$FFFF selects the CHR-ROM bank
                // The bank number is in the lower bits of the value
                self.chr_bank = (value as usize) % self.chr_rom_banks;
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
    fn test_mapper3_new() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 0x10000]; // 64KB (8 x 8KB banks)
        let mapper = Mapper3::new(prg, chr);
        assert_eq!(mapper.chr_rom_banks, 8);
        assert_eq!(mapper.chr_bank, 0);
    }

    #[test]
    fn test_chr_bank_switching() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 0x10000]; // 64KB (8 x 8KB banks)
        // Mark different banks
        for i in 0..8 {
            chr[i * 0x2000] = i as u8;
        }
        let mut mapper = Mapper3::new(prg, chr);

        // Initial state: bank 0
        assert_eq!(mapper.read(0x0000), 0);

        // Switch to bank 3
        mapper.write(0x8000, 3);
        assert_eq!(mapper.chr_bank, 3);
        assert_eq!(mapper.read(0x0000), 3);

        // Switch to bank 7
        mapper.write(0xFFFF, 7);
        assert_eq!(mapper.chr_bank, 7);
        assert_eq!(mapper.read(0x0000), 7);

        // Switch to bank 0
        mapper.write(0x9000, 0);
        assert_eq!(mapper.read(0x0000), 0);
    }

    #[test]
    fn test_chr_bank_switching_any_address() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 0x10000];
        for i in 0..8 {
            chr[i * 0x2000] = i as u8;
        }
        let mut mapper = Mapper3::new(prg, chr);

        // Any address in $8000-$FFFF should work
        mapper.write(0x8000, 1);
        assert_eq!(mapper.read(0x0000), 1);

        mapper.write(0xBFFF, 2);
        assert_eq!(mapper.read(0x0000), 2);

        mapper.write(0xE000, 5);
        assert_eq!(mapper.read(0x0000), 5);
    }

    #[test]
    fn test_chr_bank_wraparound() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 0x4000]; // 16KB (2 x 8KB banks)
        chr[0] = 0;
        chr[0x2000] = 1;
        let mut mapper = Mapper3::new(prg, chr);

        assert_eq!(mapper.chr_rom_banks, 2);

        // Bank numbers should wrap around
        mapper.write(0x8000, 2);
        assert_eq!(mapper.chr_bank, 0); // 2 % 2 = 0
        assert_eq!(mapper.read(0x0000), 0);

        mapper.write(0x8000, 5);
        assert_eq!(mapper.chr_bank, 1); // 5 % 2 = 1
        assert_eq!(mapper.read(0x0000), 1);

        mapper.write(0x8000, 255);
        assert_eq!(mapper.chr_bank, 1); // 255 % 2 = 1
        assert_eq!(mapper.read(0x0000), 1);
    }

    #[test]
    fn test_prg_rom_fixed() {
        let mut prg = vec![0u8; 0x8000]; // 32KB
        prg[0] = 0xAA;
        prg[0x4000] = 0xBB;
        prg[0x7FFF] = 0xCC;
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper3::new(prg, chr);

        // PRG-ROM should be fixed
        assert_eq!(mapper.read(0x8000), 0xAA);
        assert_eq!(mapper.read(0xC000), 0xBB);
        assert_eq!(mapper.read(0xFFFF), 0xCC);

        // Writing to $8000 should NOT affect PRG-ROM, only CHR bank
        mapper.write(0x8000, 1);
        assert_eq!(mapper.read(0x8000), 0xAA);
    }

    #[test]
    fn test_prg_rom_16kb_mirror() {
        let mut prg = vec![0u8; 0x4000]; // 16KB
        prg[0] = 0xDD;
        prg[0x3FFF] = 0xEE;
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper3::new(prg, chr);

        // 16KB ROM should be mirrored at $8000 and $C000
        assert_eq!(mapper.read(0x8000), 0xDD);
        assert_eq!(mapper.read(0xC000), 0xDD);
        assert_eq!(mapper.read(0xBFFF), 0xEE);
        assert_eq!(mapper.read(0xFFFF), 0xEE);
    }

    #[test]
    fn test_irq_not_pending() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mapper = Mapper3::new(prg, chr);

        // CNROM doesn't generate IRQs
        assert!(!mapper.irq_pending());
    }
}
