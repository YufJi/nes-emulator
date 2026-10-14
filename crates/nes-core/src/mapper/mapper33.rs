//! Mapper 33 - Taito TC0190
//!
//! Used by Taito titles such as Don Doko Don and Insector X.
//!
//! Registers ($8000-$FFFF):
//! - $8000: bits 0-1 PRG mode, bit 6 mirroring (0 = vertical, 1 = horizontal)
//! - $8001: 8KB PRG bank at $8000
//! - $8002: 8KB PRG bank at $A000
//! - $A000-$A003: 2KB CHR banks for $0000-$1FFF
//! - $C000-$C003: 1KB CHR banks overriding $1000-$1BFF
//!
//! $C000-$FFFF is hard-wired to the last 8KB PRG bank.
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_033

use super::Mapper;
use crate::ppu::rendering::Mirroring;
use crate::prelude::*;

/// Taito TC0190 mapper (Mapper 33)
#[derive(Debug)]
pub struct Mapper33 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_is_ram: bool,
    prg_bank_8000: u8,
    prg_bank_a000: u8,
    /// Two 2KB CHR banks for $0000-$0FFF
    chr_banks_2k: [u8; 2],
    /// Four 1KB CHR banks for $1000-$1FFF
    chr_banks_1k: [u8; 4],
    mirroring: Mirroring,
    prg_bank_count: usize,
    chr_1k_bank_count: usize,
    irq_pending: bool,
    /// Whether the IRQ counter is enabled (mapper 48 only)
    irq_enabled: bool,
    irq_counter: u16,
    irq_reload: u16,
    irq_latch: u8,
    irq_mode: u8,
    /// Whether this instance emulates mapper 48 (TC0690) and has an IRQ
    has_irq: bool,
}

impl Mapper33 {
    /// Create a new Mapper33 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self::with_irq(prg_rom, chr_rom, false)
    }

    /// Create the TC0690 variant (mapper 48), which adds a scanline IRQ.
    pub fn new_with_irq(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self::with_irq(prg_rom, chr_rom, true)
    }

    fn with_irq(prg_rom: Vec<u8>, chr_rom: Vec<u8>, has_irq: bool) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };

        Self {
            prg_bank_count: (prg_rom.len() / 0x2000).max(1),
            chr_1k_bank_count: (chr_rom.len() / 0x400).max(1),
            prg_rom,
            chr_rom,
            chr_is_ram,
            prg_bank_8000: 0,
            prg_bank_a000: 1,
            chr_banks_2k: [0, 1],
            chr_banks_1k: [0, 1, 2, 3],
            mirroring: Mirroring::Vertical,
            irq_pending: false,
            irq_enabled: false,
            irq_counter: 0,
            irq_reload: 0,
            irq_latch: 0,
            irq_mode: 0,
            has_irq,
        }
    }

    fn prg_offset(&self, bank: u8) -> usize {
        (bank as usize % self.prg_bank_count) * 0x2000
    }

    fn chr_1k_offset(&self, bank: u8) -> usize {
        (bank as usize % self.chr_1k_bank_count) * 0x400
    }

    /// CHR byte for $0000-$0FFF, which uses 2KB banks.
    fn read_chr_low(&self, addr: usize) -> u8 {
        let bank = self.chr_banks_2k[(addr >> 11) & 0x01] as usize;
        let offset = (bank % (self.chr_1k_bank_count / 2).max(1)) * 0x800;
        self.chr_rom
            .get(offset + (addr & 0x7FF))
            .copied()
            .unwrap_or(0)
    }
}

impl Mapper for Mapper33 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_bank_8000, prg_bank_a000,
        chr_banks_2k, chr_banks_1k, mirroring, prg_bank_count, chr_1k_bank_count, irq_pending,
        irq_enabled, irq_counter, irq_reload, irq_latch, irq_mode, has_irq);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x0FFF => self.read_chr_low(addr as usize),
            0x1000..=0x1FFF => {
                let slot = ((addr as usize) >> 10) & 0x03;
                let offset = self.chr_1k_offset(self.chr_banks_1k[slot]);
                self.chr_rom
                    .get(offset + (addr as usize & 0x3FF))
                    .copied()
                    .unwrap_or(0)
            }
            0x6000..=0x7FFF => 0,
            0x8000..=0x9FFF => {
                let offset = self.prg_offset(self.prg_bank_8000);
                self.prg_rom
                    .get(offset + (addr as usize & 0x1FFF))
                    .copied()
                    .unwrap_or(0)
            }
            0xA000..=0xBFFF => {
                let offset = self.prg_offset(self.prg_bank_a000);
                self.prg_rom
                    .get(offset + (addr as usize & 0x1FFF))
                    .copied()
                    .unwrap_or(0)
            }
            0xC000..=0xFFFF => {
                let offset = self.prg_offset((self.prg_bank_count - 1) as u8);
                self.prg_rom
                    .get(offset + (addr as usize & 0x1FFF))
                    .copied()
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let index = addr as usize;
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            0x8000..=0x8003 => match addr & 0x03 {
                0 => {
                    self.mirroring = if value & 0x40 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
                1 => self.prg_bank_8000 = value & 0x3F,
                2 => self.prg_bank_a000 = value & 0x3F,
                _ => {}
            },
            0xA000..=0xA003 => {
                let slot = (addr as usize) & 0x03;
                self.chr_banks_2k[slot & 0x01] = value;
            }
            0xC000..=0xC003 => {
                let slot = (addr as usize) & 0x03;
                self.chr_banks_1k[slot] = value;
            }
            0xE000..=0xE003 if self.has_irq => match addr & 0x03 {
                0 => {
                    self.irq_latch = value;
                    self.irq_counter = 0;
                    self.irq_reload = 1;
                }
                1 => {
                    self.irq_reload = 0;
                    if self.irq_counter == 0 {
                        self.irq_mode = 0;
                        self.irq_enabled = true;
                    } else {
                        self.irq_mode = 1;
                    }
                }
                2 => {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                }
                _ => {
                    self.irq_mode = 1;
                    self.irq_enabled = true;
                }
            },
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

    /// The TC0690 counts CPU cycles while enabled, reloading from the latch the
    /// first time after a write to $E000.
    fn on_scanline(&mut self, _scanline: u16, _rendering: bool) {
        if !self.has_irq || !self.irq_enabled {
            return;
        }

        if self.irq_counter == 0 {
            self.irq_counter = self.irq_latch as u16;
            // The reload happens once; the following wrap raises the IRQ.
            self.irq_mode = 1;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_mode == 1 {
            self.irq_pending = true;
        }
        self.irq_reload = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mapper(with_irq: bool) -> Mapper33 {
        let mut prg = vec![0u8; 8 * 0x2000];
        for bank in 0..8 {
            prg[bank * 0x2000] = bank as u8;
        }
        let mut chr = vec![0u8; 8 * 0x400];
        for bank in 0..8 {
            chr[bank * 0x400] = 0x30 + bank as u8;
        }
        if with_irq {
            Mapper33::new_with_irq(prg, chr)
        } else {
            Mapper33::new(prg, chr)
        }
    }

    #[test]
    fn test_prg_and_chr_banking() {
        let mut mapper = make_mapper(false);

        mapper.write(0x8001, 3);
        mapper.write(0x8002, 4);
        assert_eq!(mapper.read(0x8000), 3);
        assert_eq!(mapper.read(0xA000), 4);
        assert_eq!(mapper.read(0xC000), 7);

        mapper.write(0xC002, 5);
        assert_eq!(mapper.read(0x1800), 0x35);
    }

    #[test]
    fn test_mirroring_bit() {
        let mut mapper = make_mapper(false);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Vertical));
        mapper.write(0x8000, 0x40);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Horizontal));
    }

    #[test]
    fn test_irq_only_on_the_tc0690_variant() {
        let mut plain = make_mapper(false);
        plain.write(0xE000, 2);
        plain.on_scanline(10, true);
        assert!(!plain.irq_pending());

        let mut irq = make_mapper(true);
        irq.write(0xE000, 2); // latch = 2, reload
        irq.write(0xE001, 0); // enable
        irq.on_scanline(10, true); // reload to 2
        irq.on_scanline(11, true); // 2 -> 1
        assert!(!irq.irq_pending());
        irq.on_scanline(12, true); // 1 -> 0 -> IRQ
        assert!(irq.irq_pending());

        irq.write(0xE002, 0); // acknowledge
        assert!(!irq.irq_pending());
    }
}
