//! Mapper 30 - UNROM 512 (RetroUSB / InfiniteNESLives)
//!
//! An UNROM with a 512 KB address space, bankable CHR RAM and a switchable
//! one-screen nametable. CHR is RAM: 32 KB of it, since iNES has nowhere to
//! record the size and 32 KB is what the board ships with.
//!
//! Register ($8000-$FFFF): `NCCP PPPP`
//! - Bits 0-4: 16 KB PRG bank at $8000-$BFFF ($C000-$FFFF stays fixed)
//! - Bits 5-6: 8 KB CHR-RAM bank
//! - Bit 7:    nametable select (0 = lower, 1 = upper)
//!
//! The board has bus conflicts on writes; this core does not model them yet
//! (see `docs/architecture.md` §15.1).
//!
//! See <https://www.nesdev.org/wiki/UNROM_512>

use super::Mapper;
use crate::ppu::rendering::Mirroring;
use crate::prelude::*;

/// CHR-RAM on this board, in bytes.
const CHR_RAM_SIZE: usize = 0x8000;

/// UNROM 512 mapper (Mapper 30)
#[derive(Debug)]
pub struct Mapper30 {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    /// 16 KB PRG bank mapped at $8000
    prg_bank: u8,
    /// 8 KB CHR-RAM bank
    chr_bank: u8,
    /// Nametable select: false = lower bank, true = upper
    upper_nametable: bool,
    irq_pending: bool,
}

impl Mapper30 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        // A cartridge that declares CHR-ROM is not really UNROM 512; keep the
        // ROM so such a dump still shows something rather than a blank screen.
        let chr_ram = if chr_rom.is_empty() {
            vec![0u8; CHR_RAM_SIZE]
        } else {
            chr_rom
        };
        Self {
            prg_rom,
            chr_ram,
            prg_bank: 0,
            chr_bank: 0,
            upper_nametable: false,
            irq_pending: false,
        }
    }

    fn prg_offset(&self) -> usize {
        let banks = (self.prg_rom.len() / 0x4000).max(1);
        (self.prg_bank as usize % banks) * 0x4000
    }

    fn prg_last_offset(&self) -> usize {
        self.prg_rom.len().saturating_sub(0x4000)
    }

    fn chr_offset(&self) -> usize {
        let banks = (self.chr_ram.len() / 0x2000).max(1);
        (self.chr_bank as usize % banks) * 0x2000
    }
}

impl Mapper for Mapper30 {
    crate::state::mapper_state!(chr_ram, prg_bank, chr_bank, upper_nametable, irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset() + (addr as usize);
                self.chr_ram.get(index).copied().unwrap_or(0)
            }
            0x8000..=0xBFFF => {
                let index = self.prg_offset() + (addr - 0x8000) as usize;
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            0xC000..=0xFFFF => {
                let index = self.prg_last_offset() + (addr - 0xC000) as usize;
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_offset() + (addr as usize);
                if index < self.chr_ram.len() {
                    self.chr_ram[index] = value;
                }
            }
            0x8000..=0xFFFF => {
                self.prg_bank = value & 0x1F;
                self.chr_bank = (value >> 5) & 0x03;
                self.upper_nametable = (value & 0x80) != 0;
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

    /// The register also picks which single-screen nametable is shown.
    fn mirroring(&self) -> Option<Mirroring> {
        Some(if self.upper_nametable {
            Mirroring::SingleScreenUpper
        } else {
            Mirroring::SingleScreen
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_banks_and_nametable_from_one_register() {
        let mut prg = vec![0u8; 0x40000]; // 16 banks of 16 KB
        for (i, byte) in prg.iter_mut().enumerate() {
            *byte = (i / 0x4000) as u8;
        }
        let mut m = Mapper30::new(prg, Vec::new());

        // Bank = bits 0-4, CHR bank = bits 5-6, nametable = bit 7.
        m.write(0x8000, 0b1100_0110);
        assert_eq!(m.read(0x8000), 0x06);
        assert_eq!(
            m.read(0xC000),
            0x0F,
            "the top half is fixed to the last bank"
        );
        assert_eq!(m.chr_bank, (0b1100_0110u8 >> 5) & 0x03);
        assert_eq!(m.mirroring(), Some(Mirroring::SingleScreenUpper));

        m.write(0x8000, 0x00);
        assert_eq!(m.mirroring(), Some(Mirroring::SingleScreen));
    }

    #[test]
    fn test_chr_ram_is_32k_and_banked() {
        let mut m = Mapper30::new(vec![0u8; 0x8000], Vec::new());
        assert_eq!(m.chr_ram.len(), CHR_RAM_SIZE);

        m.write(0x0000, 0x11);
        assert_eq!(m.read(0x0000), 0x11);

        m.write(0x8000, 0x20); // CHR bank 1
        assert_eq!(m.read(0x0000), 0x00, "a different bank is a different 8 KB");
        m.write(0x0000, 0x22);
        m.write(0x8000, 0x00);
        assert_eq!(m.read(0x0000), 0x11, "bank 0 kept its byte");
    }
}
