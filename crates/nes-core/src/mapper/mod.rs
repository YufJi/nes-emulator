//! Mapper module - Cartridge memory mappers
//!
//! Mappers handle the mapping of CPU/PPU addresses to cartridge ROM/RAM.
//! Different games use different mappers to support larger ROM sizes
//! and additional hardware features.

use core::fmt::Debug;

/// Mapper trait for NES cartridge mappers
///
/// Mappers translate CPU/PPU addresses to cartridge memory locations.
/// They also handle any special hardware features like bank switching
/// and IRQ generation.
pub trait Mapper: Send + Sync + Debug {
    /// Read a byte from the mapper at the given address
    ///
    /// This is called for addresses in the cartridge space ($4020-$FFFF for CPU,
    /// $0000-$1FFF for PPU pattern tables, $2000-$2FFF for PPU nametables).
    fn read(&mut self, addr: u16) -> u8;

    /// Write a byte to the mapper at the given address
    ///
    /// Writes can trigger bank switching or other mapper-specific behavior.
    fn write(&mut self, addr: u16, value: u8);

    /// Check if the mapper has an IRQ pending
    fn irq_pending(&self) -> bool;

    /// Clear the pending IRQ flag
    fn clear_irq(&mut self);
}

/// Mapper factory - creates appropriate mapper based on mapper number
pub fn create_mapper(
    mapper_number: u16,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
) -> crate::Result<Box<dyn Mapper>> {
    match mapper_number {
        0 => Ok(Box::new(mapper0::Mapper0::new(prg_rom, chr_rom))),
        1 => Ok(Box::new(mapper1::Mapper1::new(prg_rom, chr_rom))),
        2 => Ok(Box::new(mapper2::Mapper2::new(prg_rom, chr_rom))),
        3 => Ok(Box::new(mapper3::Mapper3::new(prg_rom, chr_rom))),
        4 => Ok(Box::new(mapper4::Mapper4::new(prg_rom, chr_rom))),
        5 => Ok(Box::new(mapper5::Mapper5::new(prg_rom, chr_rom))),
        7 => Ok(Box::new(mapper7::Mapper7::new(prg_rom, chr_rom))),
        9 => Ok(Box::new(mapper9::Mapper9::new(prg_rom, chr_rom))),
        11 => Ok(Box::new(mapper11::Mapper11::new(prg_rom, chr_rom))),
        34 => Ok(Box::new(mapper34::Mapper34::new(prg_rom, chr_rom))),
        38 => Ok(Box::new(mapper38::Mapper38::new(prg_rom, chr_rom))),
        66 => Ok(Box::new(mapper66::Mapper66::new(prg_rom, chr_rom))),
        71 => Ok(Box::new(mapper71::Mapper71::new(prg_rom, chr_rom))),
        79 => Ok(Box::new(mapper79::Mapper79::new(prg_rom, chr_rom))),
        94 => Ok(Box::new(mapper94::Mapper94::new(prg_rom, chr_rom))),
        118 => Ok(Box::new(mapper118::Mapper118::new(prg_rom, chr_rom))),
        119 => Ok(Box::new(mapper119::Mapper119::new(prg_rom, chr_rom))),
        140 => Ok(Box::new(mapper140::Mapper140::new(prg_rom, chr_rom))),
        180 => Ok(Box::new(mapper180::Mapper180::new(prg_rom, chr_rom))),
        240 => Ok(Box::new(mapper240::Mapper240::new(prg_rom, chr_rom))),
        241 => Ok(Box::new(mapper241::Mapper241::new(prg_rom, chr_rom))),
        _ => Err(crate::JsnesError::UnsupportedMapper(mapper_number)),
    }
}

pub mod mapper0;
pub mod mapper1;
pub mod mapper11;
pub mod mapper118;
pub mod mapper119;
pub mod mapper140;
pub mod mapper180;
pub mod mapper2;
pub mod mapper240;
pub mod mapper241;
pub mod mapper3;
pub mod mapper34;
pub mod mapper38;
pub mod mapper4;
pub mod mapper5;
pub mod mapper66;
pub mod mapper7;
pub mod mapper71;
pub mod mapper79;
pub mod mapper9;
pub mod mapper94;
