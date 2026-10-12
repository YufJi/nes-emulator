//! Mapper 10 - Nintendo MMC4
//!
//! Used by Fire Emblem, Famicom Wars and a few other early Famicom titles.
//!
//! Banks:
//! - $6000-$7FFF: 8KB PRG RAM
//! - $8000-$BFFF: 16KB switchable PRG ROM
//! - $C000-$FFFF: last 16KB PRG ROM, fixed
//! - $0000-$0FFF and $1000-$1FFF: one 4KB switchable CHR bank each
//!
//! The CHR banks are selected by two latches. A PPU fetch of tile $FD or $FE
//! flips the corresponding latch, so all tiles drawn after that point come from
//! the other register. That lets a game use more than 256 tiles on screen
//! without touching the CPU:
//!
//! - reads $0FD8-$0FDF / $1FD8-$1FDF set latch 0/1 to $FD
//! - reads $0FE8-$0FEF / $1FE8-$1FEF set latch 0/1 to $FE
//!
//! Registers (only bits 12-15 of the address matter):
//! - $A000: 16KB PRG bank, $B000/$C000: latch 0 banks for $FD/$FE,
//!   $D000/$E000: latch 1 banks for $FD/$FE, $F000: mirroring
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_010

use crate::prelude::*;
use super::Mapper;
use crate::ppu::rendering::Mirroring;

/// Nintendo MMC4 mapper (Mapper 10)
#[derive(Debug)]
pub struct Mapper10 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_is_ram: bool,
    /// 8KB work RAM at $6000-$7FFF
    prg_ram: [u8; 0x2000],
    /// 16KB PRG bank mapped at $8000
    prg_bank: u8,
    /// CHR bank used by $0000-$0FFF when latch 0 is $FD
    chr_bank_0_fd: u8,
    /// CHR bank used by $0000-$0FFF when latch 0 is $FE
    chr_bank_0_fe: u8,
    /// CHR bank used by $1000-$1FFF when latch 1 is $FD
    chr_bank_1_fd: u8,
    /// CHR bank used by $1000-$1FFF when latch 1 is $FE
    chr_bank_1_fe: u8,
    /// Latch 0 selects the $FD register when false, $FE when true
    latch_0_fe: bool,
    /// Latch 1 selects the $FD register when false, $FE when true
    latch_1_fe: bool,
    mirroring: Mirroring,
    prg_16k_bank_count: usize,
    chr_4k_bank_count: usize,
    irq_pending: bool,
}

impl Mapper10 {
    /// Create a new Mapper10 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };

        Self {
            prg_16k_bank_count: (prg_rom.len() / 0x4000).max(1),
            chr_4k_bank_count: (chr_rom.len() / 0x1000).max(1),
            prg_rom,
            chr_rom,
            chr_is_ram,
            prg_ram: [0; 0x2000],
            prg_bank: 0,
            chr_bank_0_fd: 0,
            chr_bank_0_fe: 1,
            chr_bank_1_fd: 0,
            chr_bank_1_fe: 1,
            // The latches power up pointing at the $FE registers.
            latch_0_fe: true,
            latch_1_fe: true,
            mirroring: Mirroring::Vertical,
            irq_pending: false,
        }
    }

    fn prg_offset(&self, bank: u8) -> usize {
        (bank as usize % self.prg_16k_bank_count) * 0x4000
    }

    fn chr_offset(&self, bank: u8) -> usize {
        (bank as usize % self.chr_4k_bank_count) * 0x1000
    }

    /// 4KB CHR bank currently mapped at $0000-$0FFF.
    fn chr_bank_low(&self) -> u8 {
        if self.latch_0_fe {
            self.chr_bank_0_fe
        } else {
            self.chr_bank_0_fd
        }
    }

    /// 4KB CHR bank currently mapped at $1000-$1FFF.
    fn chr_bank_high(&self) -> u8 {
        if self.latch_1_fe {
            self.chr_bank_1_fe
        } else {
            self.chr_bank_1_fd
        }
    }

    /// Apply the PPU-fetch side effects that flip the CHR latches.
    fn update_latches(&mut self, addr: u16) {
        match addr {
            0x0FD8..=0x0FDF | 0x1FD8..=0x1FDF => {
                if addr < 0x1000 {
                    self.latch_0_fe = false;
                } else {
                    self.latch_1_fe = false;
                }
            }
            0x0FE8..=0x0FEF | 0x1FE8..=0x1FEF => {
                if addr < 0x1000 {
                    self.latch_0_fe = true;
                } else {
                    self.latch_1_fe = true;
                }
            }
            _ => {}
        }
    }
}

impl Mapper for Mapper10 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_ram, prg_bank,
        chr_bank_0_fd, chr_bank_0_fe, chr_bank_1_fd, chr_bank_1_fe, latch_0_fe, latch_1_fe,
        mirroring, prg_16k_bank_count, chr_4k_bank_count, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x0FFF => {
                let offset = self.chr_offset(self.chr_bank_low());
                let value = self
                    .chr_rom
                    .get(offset + (addr as usize & 0x0FFF))
                    .copied()
                    .unwrap_or(0);
                self.update_latches(addr);
                value
            }
            0x1000..=0x1FFF => {
                let offset = self.chr_offset(self.chr_bank_high());
                let value = self
                    .chr_rom
                    .get(offset + (addr as usize & 0x0FFF))
                    .copied()
                    .unwrap_or(0);
                self.update_latches(addr);
                value
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xBFFF => {
                let offset = self.prg_offset(self.prg_bank);
                self.prg_rom.get(offset + (addr as usize & 0x3FFF)).copied().unwrap_or(0)
            }
            0xC000..=0xFFFF => {
                let offset = self.prg_offset((self.prg_16k_bank_count - 1) as u8);
                self.prg_rom.get(offset + (addr as usize & 0x3FFF)).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let bank = if addr < 0x1000 {
                    self.chr_bank_low()
                } else {
                    self.chr_bank_high()
                };
                let offset = self.chr_offset(bank);
                let index = offset + (addr as usize & 0x0FFF);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = value,
            // Only bits 12-15 select the register.
            0xA000..=0xAFFF => self.prg_bank = value & 0x0F,
            0xB000..=0xBFFF => self.chr_bank_0_fd = value & 0x1F,
            0xC000..=0xCFFF => self.chr_bank_0_fe = value & 0x1F,
            0xD000..=0xDFFF => self.chr_bank_1_fd = value & 0x1F,
            0xE000..=0xEFFF => self.chr_bank_1_fe = value & 0x1F,
            0xF000..=0xFFFF => {
                self.mirroring = if value & 0x01 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
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

    fn mirroring(&self) -> Option<Mirroring> {
        Some(self.mirroring)
    }

    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    fn load_save_ram(&mut self, data: &[u8]) {
        let n = data.len().min(self.prg_ram.len());
        self.prg_ram[..n].copy_from_slice(&data[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mapper() -> Mapper10 {
        let mut prg = vec![0u8; 4 * 0x4000];
        for bank in 0..4 {
            prg[bank * 0x4000] = bank as u8;
        }
        let mut chr = vec![0u8; 8 * 0x1000];
        for bank in 0..8 {
            chr[bank * 0x1000] = 0x60 + bank as u8;
        }
        Mapper10::new(prg, chr)
    }

    #[test]
    fn test_prg_banks() {
        let mut mapper = make_mapper();

        mapper.write(0xA000, 1);
        assert_eq!(mapper.read(0x8000), 1);
        assert_eq!(mapper.read(0xC000), 3, "$C000 is fixed to the last bank");
    }

    #[test]
    fn test_chr_latch_switches_banks() {
        let mut mapper = make_mapper();

        // Latches power up on the $FE registers.
        mapper.write(0xD000, 4); // latch 1, $FD
        mapper.write(0xE000, 5); // latch 1, $FE
        assert_eq!(mapper.read(0x1000), 0x65);

        // Reading the high plane of tile $FD in the upper half flips latch 1.
        let _ = mapper.read(0x1FD8);
        assert_eq!(mapper.read(0x1000), 0x64);

        // Tile $FE flips it back.
        let _ = mapper.read(0x1FE8);
        assert_eq!(mapper.read(0x1000), 0x65);

        // The lower half has its own latch.
        mapper.write(0xB000, 2); // latch 0, $FD
        mapper.write(0xC000, 3); // latch 0, $FE
        assert_eq!(mapper.read(0x0000), 0x63);
        let _ = mapper.read(0x0FD8);
        assert_eq!(mapper.read(0x0000), 0x62);
    }

    #[test]
    fn test_mirroring_register() {
        let mut mapper = make_mapper();
        assert_eq!(mapper.mirroring(), Some(Mirroring::Vertical));
        mapper.write(0xF000, 1);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Horizontal));
    }
}
