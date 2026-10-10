//! Mapper 1 - MMC1 (SxROM)
//!
//! Used by games like The Legend of Zelda, Metroid, Mega Man 2, Final Fantasy.
//! Writes use a 5-bit serial shift register (5 consecutive writes to load a value).
//! Provides switchable 16 KB PRG-ROM banks, 4 KB or 8 KB CHR banks,
//! and software-controlled nametable mirroring.
//! See https://www.nesdev.org/wiki/MMC1

use super::Mapper;

/// MMC1 mapper (Mapper 1)
#[derive(Debug)]
pub struct Mapper1 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 8KB PRG RAM at $6000-$7FFF
    prg_ram: [u8; 0x2000],
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    /// IRQ pending flag
    irq_pending: bool,

    // MMC1-specific registers
    /// 5-bit shift register buffer
    reg_buffer: u8,
    /// Counter for shift register (0-5)
    reg_buffer_counter: u8,

    // Control register (written to $8000-$9FFF)
    /// Mirroring mode (bits 0-1): 0=single-screen (lower), 1=single-screen (upper), 2=vertical, 3=horizontal
    mirroring: u8,
    /// PRG switching area (bit 2): 0=$C000 fixed, $8000 switchable; 1=$8000 fixed, $C000 switchable
    prg_switching_area: u8,
    /// PRG switching size (bit 3): 0=32KB, 1=16KB
    prg_switching_size: u8,
    /// VROM switching size (bit 4): 0=8KB, 1=4KB
    vrom_switching_size: u8,

    // CHR bank registers
    /// CHR bank 0 (4KB mode) or 8KB bank (8KB mode) - written to $A000-$BFFF
    chr_bank_0: u8,
    /// CHR bank 1 (4KB mode only) - written to $C000-$DFFF
    chr_bank_1: u8,
    /// PRG bank select - written to $E000-$FFFF
    prg_bank: u8,

    // Number of 16KB PRG-ROM banks
    prg_rom_banks: usize,
    // Number of 4KB CHR-ROM banks
    chr_rom_banks: usize,
}

impl Mapper1 {
    /// Create a new Mapper1 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram {
            vec![0u8; 8192] // 8KB CHR-RAM
        } else {
            chr_rom
        };

        let prg_rom_banks = prg_rom.len() / 0x4000;
        let chr_rom_banks = chr_rom.len() / 0x1000;

        Self {
            prg_rom,
            chr_rom,
            prg_ram: [0u8; 0x2000],
            chr_is_ram,
            irq_pending: false,
            reg_buffer: 0,
            reg_buffer_counter: 0,
            mirroring: 3, // Default to horizontal mirroring
            prg_switching_area: 1,
            prg_switching_size: 1,
            vrom_switching_size: 0,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            prg_rom_banks,
            chr_rom_banks,
        }
    }

    /// Get the register number from the address written to
    fn get_reg_number(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => 0,
            0xA000..=0xBFFF => 1,
            0xC000..=0xDFFF => 2,
            _ => 3, // 0xE000-0xFFFF
        }
    }

    /// Set a register value
    fn set_reg(&mut self, reg: u8, value: u8) {
        match reg {
            0 => {
                // Control register
                self.mirroring = value & 0x03;
                self.prg_switching_area = (value >> 2) & 0x01;
                self.prg_switching_size = (value >> 3) & 0x01;
                self.vrom_switching_size = (value >> 4) & 0x01;
            }
            1 => {
                // CHR bank 0
                self.chr_bank_0 = value & 0x1F;
            }
            2 => {
                // CHR bank 1
                self.chr_bank_1 = value & 0x1F;
            }
            3 => {
                // PRG bank
                self.prg_bank = value & 0x0F;
            }
            _ => {}
        }
    }

    /// Read from PRG-ROM at the given address
    fn read_prg_rom(&self, addr: u16) -> u8 {
        let bank: usize = if self.prg_switching_size == 0 {
            // 32KB mode - use prg_bank with bit 0 ignored
            let bank_32k = ((self.prg_bank & 0x0E) >> 1) as usize;
            let offset = (addr - 0x8000) as usize;
            let rom_addr = (bank_32k * 0x8000 + offset) % self.prg_rom.len();
            return self.prg_rom.get(rom_addr).copied().unwrap_or(0);
        } else {
            // 16KB mode
            let bank_16k = self.prg_bank as usize;
            let is_upper_bank = if self.prg_switching_area == 0 {
                // $C000 is switchable, $8000 is fixed to first bank
                addr >= 0xC000
            } else {
                // $8000 is switchable, $C000 is fixed to last bank
                addr < 0xC000
            };

            if is_upper_bank {
                bank_16k
            } else if self.prg_switching_area == 0 {
                // $8000 fixed to first bank
                0
            } else {
                // $C000 fixed to last bank
                self.prg_rom_banks.saturating_sub(1)
            }
        };

        let offset = (addr & 0x3FFF) as usize;
        let rom_addr = (bank * 0x4000 + offset) % self.prg_rom.len();
        self.prg_rom.get(rom_addr).copied().unwrap_or(0)
    }

    /// Read from CHR-ROM/RAM at the given address
    fn read_chr(&self, addr: u16) -> u8 {
        if self.chr_is_ram || self.chr_rom_banks == 0 {
            // CHR-RAM mode
            let index = addr as usize % self.chr_rom.len().max(1);
            return self.chr_rom.get(index).copied().unwrap_or(0);
        }

        let bank: usize = if self.vrom_switching_size == 0 {
            // 8KB mode - use chr_bank_0 with bit 0 ignored
            let bank_8k = ((self.chr_bank_0 & 0x1E) >> 1) as usize;
            let offset = addr as usize;
            let chr_addr = (bank_8k * 0x2000 + offset) % self.chr_rom.len();
            return self.chr_rom.get(chr_addr).copied().unwrap_or(0);
        } else {
            // 4KB mode
            let is_upper_bank = addr >= 0x1000;
            let bank_4k = if is_upper_bank {
                self.chr_bank_1 & 0x1F
            } else {
                self.chr_bank_0 & 0x1F
            };
            let offset = (addr & 0x0FFF) as usize;
            let chr_addr = (bank_4k as usize * 0x1000 + offset) % self.chr_rom.len();
            return self.chr_rom.get(chr_addr).copied().unwrap_or(0);
        };
    }

    /// Write to CHR-RAM
    fn write_chr(&mut self, addr: u16, value: u8) {
        if self.chr_is_ram {
            let index = addr as usize % self.chr_rom.len().max(1);
            if index < self.chr_rom.len() {
                self.chr_rom[index] = value;
            }
        }
    }
}

impl Mapper for Mapper1 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_ram, irq_pending,
        reg_buffer, reg_buffer_counter, mirroring, prg_switching_area, prg_switching_size,
        vrom_switching_size, chr_bank_0, chr_bank_1, prg_bank, prg_rom_banks, chr_rom_banks);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM/RAM: $0000-$1FFF
            0x0000..=0x1FFF => self.read_chr(addr),
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
            // CHR-RAM writes (if enabled)
            0x0000..=0x1FFF => {
                self.write_chr(addr, value);
            }
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index] = value;
            }
            // MMC1 registers: $8000-$FFFF
            0x8000..=0xFFFF => {
                if value & 0x80 != 0 {
                    // Reset shift register
                    self.reg_buffer = 0;
                    self.reg_buffer_counter = 0;
                    // Also reset PRG switching to 16KB mode with $C000 fixed
                    self.prg_switching_area = 1;
                    self.prg_switching_size = 1;
                } else {
                    // Shift in the LSB
                    self.reg_buffer |= (value & 0x01) << self.reg_buffer_counter;
                    self.reg_buffer_counter += 1;

                    if self.reg_buffer_counter == 5 {
                        // Use the buffered value
                        let reg = self.get_reg_number(addr);
                        self.set_reg(reg, self.reg_buffer);

                        // Reset buffer
                        self.reg_buffer = 0;
                        self.reg_buffer_counter = 0;
                    }
                }
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

    /// MMC1 mirroring register: 0/1 single screen, 2 vertical, 3 horizontal.
    fn mirroring(&self) -> Option<crate::ppu::rendering::Mirroring> {
        Some(match self.mirroring & 0x03 {
            0 => crate::ppu::rendering::Mirroring::SingleScreen,
            1 => crate::ppu::rendering::Mirroring::SingleScreenUpper,
            2 => crate::ppu::rendering::Mirroring::Vertical,
            _ => crate::ppu::rendering::Mirroring::Horizontal,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_mapper() -> Mapper1 {
        let prg = vec![0u8; 0x40000]; // 256KB (16 x 16KB banks)
        let chr = vec![0u8; 0x10000]; // 64KB (16 x 4KB banks)
        Mapper1::new(prg, chr)
    }

    #[test]
    fn test_mapper1_new() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mapper = Mapper1::new(prg, chr);
        assert!(!mapper.chr_is_ram);
        assert_eq!(mapper.prg_rom_banks, 2);
        assert_eq!(mapper.chr_rom_banks, 2);
    }

    #[test]
    fn test_mapper1_chr_ram() {
        let prg = vec![0u8; 16384];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mapper = Mapper1::new(prg, chr);
        assert!(mapper.chr_is_ram);
        assert_eq!(mapper.chr_rom.len(), 8192);
    }

    #[test]
    fn test_prg_ram_read_write() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper1::new(prg, chr);

        // Test PRG RAM read/write at $6000-$7FFF
        mapper.write(0x6000, 0x42);
        assert_eq!(mapper.read(0x6000), 0x42);

        mapper.write(0x7000, 0xAB);
        assert_eq!(mapper.read(0x7000), 0xAB);

        mapper.write(0x7FFF, 0xEF);
        assert_eq!(mapper.read(0x7FFF), 0xEF);
    }

    #[test]
    fn test_shift_register() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper1::new(prg, chr);

        // Write control register (register 0) with value 0x0C (16KB mode, $C000 fixed)
        // Binary: 01100 - we write LSB first
        // Write sequence: 0, 0, 1, 1, 0 (reading from right to left)
        mapper.write(0x8000, 0); // bit 0
        mapper.write(0x8000, 0); // bit 1
        mapper.write(0x8000, 1); // bit 2
        mapper.write(0x8000, 1); // bit 3
        mapper.write(0x8000, 0); // bit 4

        assert_eq!(mapper.mirroring, 0);
        assert_eq!(mapper.prg_switching_area, 1);
        assert_eq!(mapper.prg_switching_size, 1);
        assert_eq!(mapper.vrom_switching_size, 0);
    }

    #[test]
    fn test_shift_register_reset() {
        let prg = vec![0u8; 32768];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper1::new(prg, chr);

        // Start writing some bits
        mapper.write(0x8000, 1);
        mapper.write(0x8000, 0);
        assert_eq!(mapper.reg_buffer_counter, 2);

        // Reset with bit 7 set
        mapper.write(0x8000, 0x80);
        assert_eq!(mapper.reg_buffer_counter, 0);
        assert_eq!(mapper.reg_buffer, 0);
    }

    #[test]
    fn test_prg_bank_switching_16kb() {
        let mut prg = vec![0u8; 0x40000]; // 256KB
        // Mark different banks
        for i in 0..16 {
            prg[i * 0x4000] = i as u8;
        }
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper1::new(prg, chr);

        // Set to 16KB mode with $8000 switchable, $C000 fixed to last bank
        // Control register: mirroring=0, prg_switching_area=1, prg_switching_size=1, vrom_switching_size=0
        // Value: 0x0C = 0b01100
        // Write LSB first: 0,0,1,1,0
        mapper.write(0x8000, 0); // bit 0
        mapper.write(0x8000, 0); // bit 1
        mapper.write(0x8000, 1); // bit 2
        mapper.write(0x8000, 1); // bit 3
        mapper.write(0x8000, 0); // bit 4

        // Set PRG bank to 2
        // Binary: 00010 - write LSB first
        mapper.write(0xE000, 0); // bit 0
        mapper.write(0xE000, 1); // bit 1
        mapper.write(0xE000, 0); // bit 2
        mapper.write(0xE000, 0); // bit 3
        mapper.write(0xE000, 0); // bit 4

        // $8000 should now map to bank 2
        assert_eq!(mapper.read(0x8000), 2);
        // $C000 should map to last bank (15)
        assert_eq!(mapper.read(0xC000), 15);
    }

    #[test]
    fn test_chr_bank_switching_4kb() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 0x10000]; // 64KB
        // Mark different 4KB banks
        for i in 0..16 {
            chr[i * 0x1000] = i as u8;
        }
        let mut mapper = Mapper1::new(prg, chr);

        // Set to 4KB CHR mode
        // Control register: vrom_switching_size=1
        // Value: 0x10 = 0b10000
        mapper.write(0x8000, 0); // bit 0
        mapper.write(0x8000, 0); // bit 1
        mapper.write(0x8000, 0); // bit 2
        mapper.write(0x8000, 0); // bit 3
        mapper.write(0x8000, 1); // bit 4

        assert_eq!(mapper.vrom_switching_size, 1);

        // Set CHR bank 0 to 3
        mapper.write(0xA000, 1); // bit 0
        mapper.write(0xA000, 1); // bit 1
        mapper.write(0xA000, 0); // bit 2
        mapper.write(0xA000, 0); // bit 3
        mapper.write(0xA000, 0); // bit 4

        // Set CHR bank 1 to 5
        mapper.write(0xC000, 1); // bit 0
        mapper.write(0xC000, 0); // bit 1
        mapper.write(0xC000, 1); // bit 2
        mapper.write(0xC000, 0); // bit 3
        mapper.write(0xC000, 0); // bit 4

        // $0000 should map to bank 3
        assert_eq!(mapper.read(0x0000), 3);
        // $1000 should map to bank 5
        assert_eq!(mapper.read(0x1000), 5);
    }

    #[test]
    fn test_chr_bank_switching_8kb() {
        let prg = vec![0u8; 32768];
        let mut chr = vec![0u8; 0x10000]; // 64KB
        // Mark different 8KB banks
        for i in 0..8 {
            chr[i * 0x2000] = i as u8;
        }
        let mut mapper = Mapper1::new(prg, chr);

        // Set to 8KB CHR mode (default)
        assert_eq!(mapper.vrom_switching_size, 0);

        // Set CHR bank 0 to 4 (will use bits 1-4 for 8KB bank)
        // Value: 0x04 = 0b00100
        // In 8KB mode: bank = (value & 0x1E) >> 1 = (0x04 & 0x1E) >> 1 = 0x02
        mapper.write(0xA000, 0); // bit 0
        mapper.write(0xA000, 0); // bit 1
        mapper.write(0xA000, 1); // bit 2
        mapper.write(0xA000, 0); // bit 3
        mapper.write(0xA000, 0); // bit 4

        // $0000 should map to 8KB bank 2
        assert_eq!(mapper.read(0x0000), 2);
    }

    #[test]
    fn test_prg_bank_switching_32kb() {
        let mut prg = vec![0u8; 0x40000]; // 256KB (8 x 32KB banks)
        // Mark different 32KB banks
        for i in 0..8 {
            prg[i * 0x8000] = i as u8;
        }
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper1::new(prg, chr);

        // Set to 32KB PRG mode
        // Control register: prg_switching_size=0
        // Value: 0x04 = 0b00100
        mapper.write(0x8000, 0); // bit 0
        mapper.write(0x8000, 0); // bit 1
        mapper.write(0x8000, 1); // bit 2
        mapper.write(0x8000, 0); // bit 3
        mapper.write(0x8000, 0); // bit 4

        assert_eq!(mapper.prg_switching_size, 0);

        // Set PRG bank to 4 (will use bits 1-4 for 32KB bank)
        // Value: 0x04 = 0b00100
        // In 32KB mode: bank = (value & 0x0E) >> 1 = (0x04 & 0x0E) >> 1 = 0x02
        mapper.write(0xE000, 0); // bit 0
        mapper.write(0xE000, 0); // bit 1
        mapper.write(0xE000, 1); // bit 2
        mapper.write(0xE000, 0); // bit 3
        mapper.write(0xE000, 0); // bit 4

        // $8000 should map to 32KB bank 2
        assert_eq!(mapper.read(0x8000), 2);
    }

    #[test]
    fn test_chr_ram_write() {
        let prg = vec![0u8; 32768];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mut mapper = Mapper1::new(prg, chr);

        assert!(mapper.chr_is_ram);

        // Write to CHR RAM
        mapper.write(0x0000, 0x55);
        assert_eq!(mapper.read(0x0000), 0x55);

        mapper.write(0x1FFF, 0x66);
        assert_eq!(mapper.read(0x1FFF), 0x66);
    }
}
