//! Mapper module - Cartridge memory mappers
//!
//! Mappers handle the mapping of CPU/PPU addresses to cartridge ROM/RAM.
//! Different games use different mappers to support larger ROM sizes
//! and additional hardware features.

use crate::prelude::*;
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

    /// Called by the PPU at the start of every scanline.
    ///
    /// Mappers with a scanline counter (MMC3, MMC5, VRC4…) clock it here and
    /// raise an IRQ when it expires. This is a scanline approximation of the
    /// hardware's A12 rise.
    ///
    /// # Arguments
    /// * `scanline` - NES scanline that is about to start (0-239 visible,
    ///   241-260 VBlank, pre-render last)
    /// * `rendering` - whether background or sprite rendering is enabled
    fn on_scanline(&mut self, scanline: u16, rendering: bool) {
        let _ = (scanline, rendering);
    }

    /// Contents of the cartridge's battery-backed work RAM, if it has any.
    ///
    /// The bus persists this when the cartridge header marks the board as
    /// battery-backed, and restores it through [`Mapper::load_save_ram`].
    fn save_ram(&self) -> Option<&[u8]> {
        None
    }

    /// Restore battery-backed work RAM previously obtained from
    /// [`Mapper::save_ram`]. Extra bytes are ignored and missing bytes are
    /// left untouched.
    fn load_save_ram(&mut self, data: &[u8]) {
        let _ = data;
    }

    /// Nametable mirroring currently selected by the mapper.
    ///
    /// `None` means the mapper does not control mirroring and the cartridge
    /// header's value should be used. Mappers that switch mirroring at runtime
    /// (MMC1, MMC3, AxROM…) return `Some` so the PPU can follow along.
    fn mirroring(&self) -> Option<crate::ppu::rendering::Mirroring> {
        None
    }

    /// Current expansion-audio level, mixed in with the APU output.
    ///
    /// Mappers with extra sound hardware (VRC6, MMC5, FDS, Sunsoft 5B, N163)
    /// return their channel mix here and advance it from
    /// [`Mapper::on_cpu_cycles`].
    fn audio_output(&self) -> f32 {
        0.0
    }

    /// Called once per executed CPU instruction with the cycles it consumed.
    ///
    /// Mappers whose IRQ counter runs at the CPU clock rate (Sunsoft FME-7)
    /// advance it here.
    fn on_cpu_cycles(&mut self, cycles: u8) {
        let _ = cycles;
    }

    /// Write the mapper's mutable state (banks, IRQ counters, work RAM…) into
    /// a save state.
    ///
    /// ROM images are intentionally not included: a state is restored onto the
    /// cartridge that is already loaded. Mappers implement this with
    /// [`crate::state::mapper_state`].
    fn save_state(&self, w: &mut crate::state::StateWriter);

    /// Restore the mutable state written by [`Mapper::save_state`].
    fn load_state(&mut self, r: &mut crate::state::StateReader<'_>);
}

impl crate::state::StateField for Option<Box<dyn Mapper>> {
    fn write(&self, w: &mut crate::state::StateWriter) {
        match self {
            Some(mapper) => {
                w.u8(1);
                mapper.save_state(w);
            }
            None => w.u8(0),
        }
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        if r.u8() != 0 {
            if let Some(mapper) = self.as_mut() {
                mapper.load_state(r);
            }
        }
    }
}

/// Mapper factory - creates appropriate mapper based on mapper number
/// Every mapper number [`create_mapper`] can build, in ascending order.
///
/// Hosts use this to reject a cartridge before loading it, rather than
/// discovering [`crate::NesError::UnsupportedMapper`] afterwards.
pub fn supported_mappers() -> &'static [u16] {
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 18, 21, 22, 23, 24, 25, 26, 30, 32, 33, 34,
        38, 48, 65, 66, 69, 71, 74, 79, 87, 94, 95, 112, 113, 114, 117, 118, 119, 133, 140,
        152, 178, 180, 206, 227, 240, 241, 244,
    ]
}

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
        32 => Ok(Box::new(mapper32::Mapper32::new(prg_rom, chr_rom))),
        33 => Ok(Box::new(mapper33::Mapper33::new(prg_rom, chr_rom))),
        48 => Ok(Box::new(mapper33::Mapper33::new_with_irq(prg_rom, chr_rom))),
        65 => Ok(Box::new(mapper65::Mapper65::new(prg_rom, chr_rom))),
        69 => Ok(Box::new(mapper69::Mapper69::new(prg_rom, chr_rom))),
        5 => Ok(Box::new(mapper5::Mapper5::new(prg_rom, chr_rom))),
        7 => Ok(Box::new(mapper7::Mapper7::new(prg_rom, chr_rom))),
        9 => Ok(Box::new(mapper9::Mapper9::new(prg_rom, chr_rom))),
        10 => Ok(Box::new(mapper10::Mapper10::new(prg_rom, chr_rom))),
        11 => Ok(Box::new(mapper11::Mapper11::new(prg_rom, chr_rom))),
        21 => Ok(Box::new(mapper21::Mapper21::new_vrc4a(prg_rom, chr_rom))),
        22 => Ok(Box::new(mapper21::Mapper21::new_vrc2a(prg_rom, chr_rom))),
        23 => Ok(Box::new(mapper21::Mapper21::new_vrc4f(prg_rom, chr_rom))),
        25 => Ok(Box::new(mapper21::Mapper21::new_vrc4b(prg_rom, chr_rom))),
        24 => Ok(Box::new(mapper24::Mapper24::new(prg_rom, chr_rom))),
        26 => Ok(Box::new(mapper24::Mapper24::new_swapped(prg_rom, chr_rom))),
        34 => Ok(Box::new(mapper34::Mapper34::new(prg_rom, chr_rom))),
        38 => Ok(Box::new(mapper38::Mapper38::new(prg_rom, chr_rom))),
        66 => Ok(Box::new(mapper66::Mapper66::new(prg_rom, chr_rom))),
        71 => Ok(Box::new(mapper71::Mapper71::new(prg_rom, chr_rom))),
        74 => Ok(Box::new(mapper74::Mapper74::new(prg_rom, chr_rom))),
        79 => Ok(Box::new(mapper79::Mapper79::new(prg_rom, chr_rom))),
        94 => Ok(Box::new(mapper94::Mapper94::new(prg_rom, chr_rom))),
        118 => Ok(Box::new(mapper118::Mapper118::new(prg_rom, chr_rom))),
        119 => Ok(Box::new(mapper119::Mapper119::new(prg_rom, chr_rom))),
        140 => Ok(Box::new(mapper140::Mapper140::new(prg_rom, chr_rom))),
        180 => Ok(Box::new(mapper180::Mapper180::new(prg_rom, chr_rom))),
        240 => Ok(Box::new(mapper240::Mapper240::new(prg_rom, chr_rom))),
        241 => Ok(Box::new(mapper241::Mapper241::new(prg_rom, chr_rom))),
        6 => Ok(Box::new(mapper6::Mapper6::new(prg_rom, chr_rom))),
        8 => Ok(Box::new(mapper8::Mapper8::new(prg_rom, chr_rom))),
        16 => Ok(Box::new(mapper16::Mapper16::new(prg_rom, chr_rom))),
        18 => Ok(Box::new(mapper18::Mapper18::new(prg_rom, chr_rom))),
        30 => Ok(Box::new(mapper30::Mapper30::new(prg_rom, chr_rom))),
        87 => Ok(Box::new(mapper87::Mapper87::new(prg_rom, chr_rom))),
        95 => Ok(Box::new(mapper95::Mapper95::new(prg_rom, chr_rom))),
        112 => Ok(Box::new(mapper112::Mapper112::new(prg_rom, chr_rom))),
        113 => Ok(Box::new(mapper113::Mapper113::new(prg_rom, chr_rom))),
        114 => Ok(Box::new(mapper114::Mapper114::new(prg_rom, chr_rom))),
        117 => Ok(Box::new(mapper117::Mapper117::new(prg_rom, chr_rom))),
        133 => Ok(Box::new(mapper133::Mapper133::new(prg_rom, chr_rom))),
        152 => Ok(Box::new(mapper152::Mapper152::new(prg_rom, chr_rom))),
        178 => Ok(Box::new(mapper178::Mapper178::new(prg_rom, chr_rom))),
        206 => Ok(Box::new(mapper206::Mapper206::new(prg_rom, chr_rom))),
        227 => Ok(Box::new(mapper227::Mapper227::new(prg_rom, chr_rom))),
        244 => Ok(Box::new(mapper244::Mapper244::new(prg_rom, chr_rom))),
        _ => Err(crate::NesError::UnsupportedMapper(mapper_number)),
    }
}

pub mod mapper0;
pub mod mapper1;
pub mod mapper10;
pub mod mapper11;
pub mod mapper112;
pub mod mapper113;
pub mod mapper114;
pub mod mapper117;
pub mod mapper118;
pub mod mapper119;
pub mod mapper133;
pub mod mapper140;
pub mod mapper152;
pub mod mapper16;
pub mod mapper178;
pub mod mapper18;
pub mod mapper180;
pub mod mapper2;
pub mod mapper21;
pub mod mapper240;
pub mod mapper241;
pub mod mapper244;
pub mod mapper24;
pub mod mapper206;
pub mod mapper227;
pub mod mapper3;
pub mod mapper30;
pub mod mapper32;
pub mod mapper33;
pub mod mapper34;
pub mod mapper38;
pub mod mapper4;
pub mod mapper5;
pub mod mapper6;
pub mod mapper65;
pub mod mapper66;
pub mod mapper69;
pub mod mapper7;
pub mod mapper71;
pub mod mapper74;
pub mod mapper8;
pub mod mapper87;
pub mod mapper95;
pub mod mapper79;
pub mod mapper9;
pub mod mapper94;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_mappers_matches_create_mapper() {
        let mappers = supported_mappers();

        // Sorted and duplicate-free, so hosts can diff or search the list.
        assert!(
            mappers.windows(2).all(|pair| pair[0] < pair[1]),
            "mapper list must be ascending: {mappers:?}"
        );

        // Every advertised number must really build; otherwise the list lies
        // to hosts that gate loading on it.
        for &number in mappers {
            let built = create_mapper(number, vec![0u8; 0x40000], vec![0u8; 0x40000]);
            assert!(
                built.is_ok(),
                "supported_mappers() lists {number} but create_mapper rejected it"
            );
        }

        // A number the list does not advertise must be refused.
        let unlisted = (0..=241u16)
            .find(|number| !mappers.contains(number))
            .expect("0..=241 has a gap");
        assert!(matches!(
            create_mapper(unlisted, vec![0u8; 0x40000], vec![0u8; 0x40000]),
            Err(crate::NesError::UnsupportedMapper(number)) if number == unlisted
        ));
    }
}
