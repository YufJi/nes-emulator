//! Mapper 6 - FFE F4 series (Front Fareast)
//!
//! A bank-switching board whose PRG and CHR banks are written through the
//! *high* half of the address space, with the IRQ and nametable registers
//! living in the expansion area.
//!
//! Registers:
//! - $8000-$FFFF: 16 KB PRG bank at $8000 ($C000 stays fixed), 8 KB CHR bank
//!   - Bits 2-5: PRG bank
//!   - Bits 0-1: CHR bank
//! - $42FE: four-screen, lower half
//! - $42FF: bit 4 = four-screen, upper half; else horizontal/vertical
//! - $4501: IRQ disable
//! - $4502/$4503: IRQ latch, low then high; writing the high byte enables it
//!
//! The IRQ counts CPU-ish time: it adds 133 per scanline while enabled and
//! fires when the 16-bit counter wraps.
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_006>

use crate::prelude::*;
use crate::ppu::rendering::Mirroring;
use super::Mapper;

/// FFE F4 mapper (Mapper 6)
#[derive(Debug)]
pub struct Mapper6 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper6 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank: 0,
            mirroring: Mirroring::Vertical,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let banks = (self.prg_rom.len() / 0x4000).max(1);
        let bank = if addr < 0xC000 {
            self.prg_bank as usize % banks
        } else {
            banks.saturating_sub(1)
        };
        bank * 0x4000 + (addr as usize & 0x3FFF)
    }

    fn chr_offset(&self) -> usize {
        let banks = (self.chr_rom.len() / 0x2000).max(1);
        (self.chr_bank as usize % banks) * 0x2000
    }
}

impl Mapper for Mapper6 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_bank, chr_bank, mirroring,
        irq_latch, irq_counter, irq_enabled, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset() + (addr as usize);
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
                let index = self.chr_offset() + (addr as usize);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            0x42FE => {
                // Only the "lower half" variant is described by the header, so
                // this is recorded as four-screen either way.
                self.mirroring = Mirroring::FourScreen;
                let _ = value;
            }
            0x42FF => {
                self.mirroring = if value & 0x10 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
            }
            0x4501 => {
                self.irq_enabled = false;
                self.irq_pending = false;
            }
            0x4502 => self.irq_latch = (self.irq_latch & 0xFF00) | u16::from(value),
            0x4503 => {
                self.irq_latch = (self.irq_latch & 0x00FF) | (u16::from(value) << 8);
                // The latch is what the counter counts up to.
                self.irq_counter = self.irq_latch;
                self.irq_enabled = true;
                self.irq_pending = false;
            }
            0x8000..=0xFFFF => {
                self.prg_bank = (value >> 2) & 0x0F;
                self.chr_bank = value & 0x03;
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

    /// The counter is clocked by scanline time rather than by A12: 133 per
    /// scanline, wrapping at $FFFF.
    fn on_scanline(&mut self, _scanline: u16, rendering: bool) {
        if !self.irq_enabled || !rendering {
            return;
        }
        let (counter, carry) = self.irq_counter.overflowing_add(133);
        self.irq_counter = counter;
        if carry || self.irq_counter < 133 {
            self.irq_counter = 0;
            self.irq_pending = true;
        }
    }
}
