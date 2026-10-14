//! Mapper 244 - C&E "Decathlon"
//!
//! One game, one trick: the banks are selected by the *address* of the write.
//!
//! - $8065-$80A4: 32 KB PRG bank = (address - $8065) & 3
//! - $80A5-$80E4: 8 KB CHR bank = (address - $80A5) & 7
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_244>

use super::Mapper;
use crate::prelude::*;

/// Decathlon mapper (Mapper 244)
#[derive(Debug)]
pub struct Mapper244 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_bank: u8,
    chr_bank: u8,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper244 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank: 0,
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn prg_offset(&self) -> usize {
        let banks = (self.prg_rom.len() / 0x8000).max(1);
        (self.prg_bank as usize % banks) * 0x8000
    }

    fn chr_offset(&self) -> usize {
        let banks = (self.chr_rom.len() / 0x2000).max(1);
        (self.chr_bank as usize % banks) * 0x2000
    }
}

impl Mapper for Mapper244 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_bank, chr_bank, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset() + (addr as usize);
                self.chr_rom.get(index).copied().unwrap_or(0)
            }
            0x8000..=0xFFFF => {
                let index = self.prg_offset() + (addr - 0x8000) as usize;
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
            // Both registers ignore the data byte entirely.
            0x8065..=0x80A4 => self.prg_bank = ((addr - 0x8065) & 0x03) as u8,
            0x80A5..=0x80E4 => self.chr_bank = ((addr - 0x80A5) & 0x07) as u8,
            _ => {
                let _ = value;
            }
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
    fn test_banks_come_from_the_write_address() {
        let mut m = Mapper244::new(vec![0u8; 0x20000], vec![0u8; 0x10000]);
        m.write(0x8067, 0x00); // any data byte
        assert_eq!(m.prg_bank, 2);
        m.write(0x80A9, 0xFF);
        assert_eq!(m.chr_bank, 4);
    }
}
