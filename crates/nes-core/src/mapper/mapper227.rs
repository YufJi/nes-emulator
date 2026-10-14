//! Mapper 227 - "1200-in-1" multicarts
//!
//! A multicart board where the bank number is encoded in the *address* of the
//! write rather than the data: the menu jumps to a fixed address per game, and
//! the board decodes it.
//!
//! Write ($8000-$FFFF), bank derived from the address as
//! `((addr & 0x0100) >> 4) | ((addr & 0x0078) >> 3)`:
//! - Bit 0 of the address picks 32 KB mode (odd) or 16 KB pairs (even)
//! - Bit 2 swaps the order of the two 16 KB halves
//! - Bit 7 keeps the last 16 KB independent, otherwise the second half mirrors
//!   the outer bank
//! - Bit 9 (of the address) and bit 1 pick which fixed 16 KB bank the top gets
//! - Bit 1 also selects mirroring (0 = vertical, 1 = horizontal)
//!
//! See <https://www.nesdev.org/wiki/INES_Mapper_227>

use crate::prelude::*;
use crate::ppu::rendering::Mirroring;
use super::Mapper;

/// Multicart mapper (Mapper 227)
#[derive(Debug)]
pub struct Mapper227 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 8 KB bank for each of the four PRG slots
    slots: [u8; 4],
    horizontal_mirroring: bool,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,
    irq_pending: bool,
}

impl Mapper227 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self {
            prg_rom,
            chr_rom,
            slots: [0, 1, 0, 1],
            horizontal_mirroring: false,
            chr_is_ram,
            irq_pending: false,
        }
    }

    fn slot(&self, addr: u16) -> usize {
        self.slots[((addr - 0x8000) / 0x2000) as usize] as usize
    }

    fn banks(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }
}

impl Mapper for Mapper227 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, slots, horizontal_mirroring,
        irq_pending);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.chr_rom.get(addr as usize).copied().unwrap_or(0),
            0x8000..=0xFFFF => {
                let bank = self.slot(addr) % self.banks();
                let index = bank * 0x2000 + (addr as usize & 0x1FFF);
                self.prg_rom.get(index).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                if let Some(byte) = self.chr_rom.get_mut(addr as usize) {
                    *byte = value;
                }
            }
            0x8000..=0xFFFF => {
                let _ = value; // the address carries everything
                let bank = (((addr & 0x0100) >> 4) | ((addr & 0x0078) >> 3)) as u8;
                let base = bank.wrapping_mul(4);

                if addr & 0x0001 != 0 {
                    // 32 KB mode: one 32 KB bank across the whole window.
                    self.slots = [base, base.wrapping_add(1), base.wrapping_add(2), base.wrapping_add(3)];
                } else if addr & 0x0004 != 0 {
                    // 16 KB pairs, high pair first.
                    self.slots[0] = base.wrapping_add(2);
                    self.slots[1] = base.wrapping_add(3);
                    self.slots[2] = base.wrapping_add(2);
                    self.slots[3] = base.wrapping_add(3);
                } else {
                    // 16 KB pairs, low pair first.
                    self.slots[0] = base;
                    self.slots[1] = base.wrapping_add(1);
                    self.slots[2] = base;
                    self.slots[3] = base.wrapping_add(1);
                }

                if addr & 0x0080 == 0 {
                    // The top 16 KB follows the outer bank unless bit 7 says
                    // otherwise.
                    let outer = (bank & 0x1C).wrapping_mul(4);
                    let top = if addr & 0x0200 != 0 { outer.wrapping_add(14) } else { outer };
                    self.slots[2] = top;
                    self.slots[3] = top.wrapping_add(1);
                }

                self.horizontal_mirroring = addr & 0x0002 != 0;
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
        Some(if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        })
    }
}
