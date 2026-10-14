//! Mapper 117 - Future Media (Crayon Shin-Chan, San Guo Zhi 4)
//!
//! Three switchable 8 KB PRG windows, eight 1 KB CHR windows, and a scanline
//! IRQ whose counter is compared against the scanline number rather than
//! counted down from a loaded value.
//!
//! Registers:
//! - $8000-$8002: 8 KB PRG bank at $8000, $A000, $C000 ($E000 stays fixed)
//! - $A000-$A007: 1 KB CHR bank, one per window
//! - $C000-$C002: IRQ counter and latch; also acknowledges the IRQ
//! - $E000:       IRQ enable (bit 0)
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_117>

use crate::prelude::*;
use super::Mapper;

/// Future Media mapper (Mapper 117)
#[derive(Debug)]
pub struct Mapper117 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 8 KB PRG banks for $8000, $A000 and $C000
    prg: [u8; 3],
    /// 1 KB CHR banks, one per window
    chr: [u8; 8],
    irq_counter: u8,
    irq_enabled: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper117 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg: [0, 1, 2],
            chr: [0, 1, 2, 3, 4, 5, 6, 7],
            irq_counter: 0,
            irq_enabled: false,
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let banks = (self.prg_rom.len() / 0x2000).max(1);
        let bank = match addr {
            0x8000..=0x9FFF => self.prg[0] as usize % banks,
            0xA000..=0xBFFF => self.prg[1] as usize % banks,
            0xC000..=0xDFFF => self.prg[2] as usize % banks,
            _ => banks.saturating_sub(1),
        };
        bank * 0x2000 + (addr as usize & 0x1FFF)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let banks = (self.chr_rom.len() / 0x400).max(1);
        let window = ((addr >> 10) & 0x07) as usize;
        (self.chr[window] as usize % banks) * 0x400 + (addr as usize & 0x3FF)
    }
}

impl Mapper for Mapper117 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg, chr, irq_counter, irq_enabled,
        irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset(addr);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            0x8000..=0xFFFF => {
                let index = self.prg_offset(addr);
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let index = self.chr_offset(addr);
                if let Some(byte) = self.chr_rom.get_mut(index) {
                    *byte = value;
                }
            }
            0x8000..=0x8002 => self.prg[(addr - 0x8000) as usize] = value,
            0xA000..=0xA007 => self.chr[(addr - 0xA000) as usize] = value,
            0xC000..=0xC002 => {
                self.irq_counter = value;
                self.irq_pending = false;
            }
            0xE000 => self.irq_enabled = value & 0x01 != 0,
            _ => {}
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    /// The counter holds the scanline the IRQ should fire on.
    fn on_scanline(&mut self, scanline: u16, rendering: bool) {
        if !rendering || !self.irq_enabled || self.irq_counter == 0 {
            return;
        }
        if u16::from(self.irq_counter) == scanline {
            self.irq_counter = 0;
            self.irq_pending = true;
        }
    }
}
