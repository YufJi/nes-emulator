//! Mapper 69 - Sunsoft FME-7 (and the 5B variant with expansion audio)
//!
//! Used by Gimmick!, Batman: Return of the Joker and a few other Sunsoft
//! titles.
//!
//! Registers:
//! - $8000: command (bits 0-3 select a subregister)
//! - $A000: parameter for the selected subregister
//! - $C000: bit 7 enables the IRQ counter
//! - $E000: any write acknowledges the IRQ
//!
//! Subregisters:
//! - $0-$7: 1KB CHR banks for $0000-$1FFF
//! - $8-$B: 8KB PRG banks for $6000, $8000, $A000 and $C000
//! - $C: mirroring (0 = vertical, 1 = horizontal, 2/3 = single screen)
//! - $D/$E/$F: IRQ counter low, middle and high bits (plus 5B audio)
//!
//! $E000-$FFFF is hard-wired to the last 8KB PRG bank. The expansion audio of
//! the 5B revision is not emulated yet; its registers are accepted and ignored.
//!
//! See https://www.nesdev.org/wiki/Sunsoft_FME-7

use super::Mapper;
use crate::apu::sunsoft5b::Sunsoft5bAudio;
use crate::ppu::rendering::Mirroring;

/// Sunsoft FME-7 mapper (Mapper 69)
#[derive(Debug)]
pub struct Mapper69 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_is_ram: bool,
    /// Currently selected subregister
    command: u8,
    /// Eight 1KB CHR banks covering $0000-$1FFF
    chr_banks: [u8; 8],
    /// 8KB PRG banks for $6000, $8000, $A000 and $C000
    prg_banks: [u8; 4],
    /// Nametable mirroring
    mirroring: Mirroring,
    /// IRQ counter, clocked once per CPU cycle
    irq_counter: u16,
    /// Whether the counter may raise an IRQ
    irq_enabled: bool,
    irq_pending: bool,
    /// The board's Sunsoft 5B sound chip
    audio: Sunsoft5bAudio,
    prg_bank_count: usize,
    chr_bank_count: usize,
}

impl Mapper69 {
    /// Create a new Mapper69 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };

        Self {
            prg_bank_count: (prg_rom.len() / 0x2000).max(1),
            chr_bank_count: (chr_rom.len() / 0x400).max(1),
            prg_rom,
            chr_rom,
            chr_is_ram,
            command: 0,
            chr_banks: [0; 8],
            prg_banks: [0, 1, 2, 3],
            mirroring: Mirroring::Vertical,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
            audio: Sunsoft5bAudio::new(),
        }
    }

    fn prg_offset(&self, bank: u8) -> usize {
        (bank as usize % self.prg_bank_count) * 0x2000
    }

    fn chr_offset(&self, bank: u8) -> usize {
        (bank as usize % self.chr_bank_count) * 0x400
    }

    /// Apply a write to the currently selected subregister.
    fn write_subregister(&mut self, value: u8) {
        match self.command & 0x0F {
            0x0..=0x07 => self.chr_banks[(self.command & 0x07) as usize] = value,
            0x08..=0x0B => {
                // $6000, $8000, $A000, $C000
                self.prg_banks[(self.command - 0x08) as usize] = value & 0x3F;
            }
            0x0C => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreen,
                    _ => Mirroring::SingleScreenUpper,
                };
            }
            0x0D => self.irq_counter = (self.irq_counter & 0xFF00) | value as u16,
            0x0E => self.irq_counter = (self.irq_counter & 0x00FF) | ((value as u16) << 8),
            0x0F => {
                self.irq_counter = (self.irq_counter & 0x7FFF) | ((value as u16 & 0x01) << 15);
            }
            _ => {}
        }
    }
}

impl Mapper for Mapper69 {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_banks[slot]);
                self.chr_rom.get(offset + (addr as usize & 0x3FF)).copied().unwrap_or(0)
            }
            0x6000..=0x7FFF => {
                // Cartridge work RAM (bank register $8 selects a ROM bank on
                // boards without RAM, which reads back as 0).
                0
            }
            0x8000..=0x9FFF => {
                let offset = self.prg_offset(self.prg_banks[1]);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            0xA000..=0xBFFF => {
                let offset = self.prg_offset(self.prg_banks[2]);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            0xC000..=0xDFFF => {
                let offset = self.prg_offset(self.prg_banks[3]);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            0xE000..=0xFFFF => {
                let offset = self.prg_offset((self.prg_bank_count - 1) as u8);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_banks[slot]);
                let index = offset + (addr as usize & 0x3FF);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            0x8000..=0x9FFF => self.command = value & 0x0F,
            0xA000..=0xBFFF => self.write_subregister(value),
            0xC000..=0xDFFF => {
                self.irq_enabled = value & 0x80 != 0;
                // $C000 also selects the 5B's internal register.
                self.audio.write_select(value);
            }
            0xE000..=0xFFFF => {
                self.irq_pending = false;
                self.audio.write_data(value);
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

    fn audio_output(&self) -> f32 {
        self.audio.output()
    }

    /// The FME-7 counter runs at the CPU clock rate and raises an IRQ when it
    /// wraps past $FFFF. The 5B sound chip is clocked alongside it.
    fn on_cpu_cycles(&mut self, cycles: u8) {
        for _ in 0..cycles {
            self.audio.tick();
        }
        for _ in 0..cycles {
            self.irq_counter = self.irq_counter.wrapping_add(1);
            if self.irq_counter == 0 && self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mapper() -> Mapper69 {
        let mut prg = vec![0u8; 8 * 0x2000];
        for bank in 0..8 {
            prg[bank * 0x2000] = bank as u8;
        }
        let mut chr = vec![0u8; 8 * 0x400];
        for bank in 0..8 {
            chr[bank * 0x400] = 0x40 + bank as u8;
        }
        Mapper69::new(prg, chr)
    }

    #[test]
    fn test_prg_and_chr_subregisters() {
        let mut mapper = make_mapper();

        mapper.write(0x8000, 0x09); // PRG bank for $8000
        mapper.write(0xA000, 4);
        assert_eq!(mapper.read(0x8000), 4);

        mapper.write(0x8000, 0x03); // CHR bank 3
        mapper.write(0xA000, 6);
        assert_eq!(mapper.read(0x0C00), 0x46);

        assert_eq!(mapper.read(0xE000), 7, "$E000 is fixed to the last bank");
    }

    #[test]
    fn test_mirroring_subregister() {
        let mut mapper = make_mapper();

        mapper.write(0x8000, 0x0C);
        mapper.write(0xA000, 1);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Horizontal));

        mapper.write(0xA000, 2);
        assert_eq!(mapper.mirroring(), Some(Mirroring::SingleScreen));
    }

    #[test]
    fn test_irq_counter_wraps_into_an_irq() {
        let mut mapper = make_mapper();

        // Set the counter just below the wrap point.
        mapper.write(0x8000, 0x0D);
        mapper.write(0xA000, 0xFF);
        mapper.write(0x8000, 0x0E);
        mapper.write(0xA000, 0xFF);

        mapper.write(0xC000, 0x80); // enable
        mapper.on_cpu_cycles(1);
        assert!(mapper.irq_pending());

        mapper.write(0xE000, 0); // acknowledge
        assert!(!mapper.irq_pending());

        // Disabled: no further IRQs.
        mapper.write(0xC000, 0x00);
        mapper.on_cpu_cycles(4);
        assert!(!mapper.irq_pending());
    }
}
