//! Mapper 16 - Bandai FCG (Famicom Jump, Dragon Ball, SD Gundam)
//!
//! Eight 1 KB CHR windows, one 16 KB PRG window, a nametable select and a
//! programmable IRQ. The registers are decoded from the low nibble of the
//! written address, so all of $8000-$FFFF (and the mirrored expansion area)
//! reach them.
//!
//! Registers (address & $000F):
//! - 0-7: 1 KB CHR banks
//! - 8:   16 KB PRG bank at $8000 ($C000 stays fixed)
//! - 9:   mirroring (0 = vertical, 1 = horizontal, 2 = one screen lower,
//!   3 = one screen upper)
//! - A:   IRQ latch low, reload the counter, acknowledge
//! - B:   IRQ latch high
//! - C:   IRQ mode
//! - D:   IRQ enable (bit 0) and acknowledge
//!
//! The later FCG boards carry a 24C01/24C02 EEPROM used by the Datach
//! barcode games; that peripheral is not emulated, so those cartridges run
//! without their save data.
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_016>

use super::Mapper;
use crate::ppu::rendering::Mirroring;
use crate::prelude::*;

/// Bandai FCG mapper (Mapper 16)
#[derive(Debug)]
pub struct Mapper16 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    prg_bank: u8,
    /// 1 KB CHR banks, one per window
    chr: [u8; 8],
    mirroring: Mirroring,
    irq_latch: u16,
    irq_counter: u16,
    irq_mode: u8,
    irq_enabled: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper16 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg_ram: vec![0u8; 0x2000],
            prg_bank: 0,
            chr: [0, 1, 2, 3, 4, 5, 6, 7],
            mirroring: Mirroring::Vertical,
            irq_latch: 0,
            irq_counter: 0,
            irq_mode: 0,
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

    fn chr_offset(&self, addr: u16) -> usize {
        let banks = (self.chr_rom.len() / 0x400).max(1);
        let window = ((addr >> 10) & 0x07) as usize;
        (self.chr[window] as usize % banks) * 0x400 + (addr as usize & 0x3FF)
    }

    /// One CPU cycle of the IRQ counter, in the mode the game selected.
    fn clock_irq(&mut self, cycles: u16) {
        if !self.irq_enabled || self.irq_counter == 0 {
            return;
        }
        let before = self.irq_counter;
        self.irq_counter = self.irq_counter.saturating_sub(cycles);

        let fired = match self.irq_mode {
            0 => self.irq_counter == 0,
            1 => (before & 0xF000) != (self.irq_counter & 0xF000),
            2 | 3 => (before & 0xFF00) != (self.irq_counter & 0xFF00),
            _ => (before & 0xFFF0) != (self.irq_counter & 0xFFF0),
        };

        if fired {
            self.irq_counter = 0;
            self.irq_enabled = false;
            self.irq_pending = true;
        }
    }
}

impl Mapper for Mapper16 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_ram, prg_bank, chr, mirroring,
        irq_latch, irq_counter, irq_mode, irq_enabled, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset(addr);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize & 0x1FFF],
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
            0x6000..=0x7FFF => {
                // The EEPROM bit on the upper cartridges lives here; the
                // battery RAM itself is what this core keeps.
                self.prg_ram[(addr - 0x6000) as usize & 0x1FFF] = value;
            }
            // A mirror of the register window so a write to the expansion area
            // reaches the same registers as one in the cartridge area.
            0x4020..=0x40FF | 0x8000..=0xFFFF => self.write_register(addr, value),
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

    fn on_cpu_cycles(&mut self, cycles: u8) {
        self.clock_irq(u16::from(cycles));
    }

    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    fn load_save_ram(&mut self, data: &[u8]) {
        let len = data.len().min(self.prg_ram.len());
        self.prg_ram[..len].copy_from_slice(&data[..len]);
    }
}

impl Mapper16 {
    /// The register file, decoded from the low nibble of the address.
    fn write_register(&mut self, addr: u16, value: u8) {
        match addr & 0x000F {
            bank @ 0..=7 => self.chr[bank as usize] = value,
            8 => self.prg_bank = value,
            9 => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreen,
                    _ => Mirroring::SingleScreenUpper,
                }
            }
            0x0A => {
                self.irq_latch = (self.irq_latch & 0xFF00) | u16::from(value);
                self.irq_counter = self.irq_latch;
                self.irq_pending = false;
            }
            0x0B => self.irq_latch = (self.irq_latch & 0x00FF) | (u16::from(value) << 8),
            0x0C => self.irq_mode = value,
            _ => {
                self.irq_enabled = value & 0x01 != 0;
                self.irq_pending = false;
            }
        }
    }
}
