//! NES Emulator Core - a portable NES emulation library
//!
//! This crate provides the core emulation components:
//! - CPU: 6502 processor emulation
//! - PPU: Picture Processing Unit for graphics
//! - APU: Audio Processing Unit for sound
//! - Bus: Memory bus interface
//! - Mapper: Cartridge mapper interface

#![cfg_attr(not(feature = "std"), no_std)]

// `alloc` is available on every target that ships an allocator, `std` builds
// included. Depending on it directly is what lets the core also run without
// the `std` crate.
extern crate alloc;

use alloc::string::String;

pub mod apu;
pub mod bus;
pub mod clock;
pub mod cpu;
pub mod mapper;
pub mod nes;
pub mod ppu;
pub mod state;

pub use bus::Bus;
pub use clock::FrameClock;
pub use mapper::{supported_mappers, Mapper};
pub use nes::Nes;
pub use ppu::rendering::{PixelFormat, HEIGHT, PIXEL_FORMAT, WIDTH};

/// The allocation-backed items that the `std` prelude normally injects.
///
/// Every module does `use crate::prelude::*;` so the same source builds with or
/// without `std`; under `std` these names resolve to the exact same types.
pub(crate) mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::ToString;
    pub use alloc::vec;
    pub use alloc::vec::Vec;
}

/// Result type for emulator operations
pub type Result<T> = core::result::Result<T, NesError>;

/// Error types for the emulator.
#[derive(Debug, Clone)]
pub enum NesError {
    /// The image is not a usable iNES file.
    InvalidRomFormat(String),
    /// The cartridge uses a mapper the core does not implement yet.
    UnsupportedMapper(u16),
    /// A memory access fell outside anything the machine can answer.
    MemoryAccessError(u16),
    /// A host-side I/O operation failed.
    IoError(String),
}

impl core::fmt::Display for NesError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NesError::InvalidRomFormat(msg) => write!(f, "Invalid ROM format: {msg}"),
            NesError::UnsupportedMapper(mapper) => write!(f, "Unsupported mapper: {mapper}"),
            NesError::MemoryAccessError(addr) => {
                write!(f, "Memory access error at address {addr:#04X}")
            }
            NesError::IoError(msg) => write!(f, "IO error: {msg}"),
        }
    }
}

/// Only implemented with `std`; no_std hosts work off [`Display`](core::fmt::Display).
#[cfg(feature = "std")]
impl std::error::Error for NesError {}

/// NES system configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Force a TV system instead of trusting the cartridge header.
    ///
    /// `None` (the default) uses the region advertised by the iNES header.
    pub region_override: Option<Region>,
    /// Collect audio samples for the host.
    ///
    /// The APU keeps running either way — its frame IRQ and the DMC DMA
    /// requests change CPU timing — so this only controls whether samples are
    /// buffered for the host to play.
    pub enable_audio: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            region_override: None,
            enable_audio: true,
        }
    }
}

/// PPU dots per scanline. Each CPU cycle covers three of them.
pub const PPU_CYCLES_PER_SCANLINE: u32 = 341;

/// NES TV system regions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// NTSC (North America, Japan) - 60 FPS
    Ntsc,
    /// PAL (Europe, Australia) - 50 FPS
    Pal,
}

impl crate::state::StateField for Region {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u8(match self {
            Region::Ntsc => 0,
            Region::Pal => 1,
        });
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        *self = match r.u8() {
            1 => Region::Pal,
            _ => Region::Ntsc,
        };
    }
}

impl Region {
    /// Get the CPU clock rate in Hz
    pub fn cpu_clock_rate(&self) -> u32 {
        match self {
            Region::Ntsc => 1_789_773,
            Region::Pal => 1_662_607,
        }
    }

    /// Get the PPU clock rate in Hz
    pub fn ppu_clock_rate(&self) -> u32 {
        match self {
            Region::Ntsc => 5_369_318,
            Region::Pal => 5_320_313,
        }
    }

    /// Scanlines in one frame, i.e. the index of the last one plus one.
    pub fn scanlines(&self) -> u16 {
        self.last_scanline() + 1
    }

    /// Exact frame rate in Hz, derived from the PPU clock and the frame
    /// geometry rather than rounded to a nominal 60/50.
    ///
    /// NTSC works out to ~60.0985 Hz and PAL to ~50.0067 Hz; a front end that
    /// paces against a rounded 60 would drift away from the emulated console.
    pub fn fps(&self) -> f64 {
        self.ppu_clock_rate() as f64 / (PPU_CYCLES_PER_SCANLINE as f64 * self.scanlines() as f64)
    }

    /// Scanline number of the pre-render line, i.e. the last scanline of a
    /// frame. PAL adds 50 extra vblank scanlines.
    pub fn last_scanline(&self) -> u16 {
        match self {
            Region::Ntsc => 261,
            Region::Pal => 311,
        }
    }

    /// CPU cycles in one frame (341 PPU dots per scanline, 3 dots per CPU
    /// cycle).
    pub fn cpu_cycles_per_frame(&self) -> u32 {
        self.scanlines() as u32 * PPU_CYCLES_PER_SCANLINE / 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_frame_timing() {
        // NTSC: 262 scanlines, PAL: 312. The PPU runs three dots per CPU cycle.
        assert_eq!(Region::Ntsc.last_scanline(), 261);
        assert_eq!(Region::Pal.last_scanline(), 311);

        assert_eq!(Region::Ntsc.cpu_cycles_per_frame(), 29780);
        assert_eq!(Region::Pal.cpu_cycles_per_frame(), 35464);

        assert!(Region::Ntsc.cpu_cycles_per_frame() < Region::Pal.cpu_cycles_per_frame());

        // The frame rate comes from the PPU clock, so it is not exactly 60/50.
        assert_eq!(Region::Ntsc.scanlines(), 262);
        assert_eq!(Region::Pal.scanlines(), 312);
        assert!((Region::Ntsc.fps() - 60.098).abs() < 1e-3, "{}", Region::Ntsc.fps());
        assert!((Region::Pal.fps() - 50.007).abs() < 1e-3, "{}", Region::Pal.fps());

        // Front-end pacing must not assume a nominal 60 Hz.
        assert!(Region::Ntsc.fps() > 60.0);
        assert!(Region::Pal.fps() > 50.0);
    }
}
