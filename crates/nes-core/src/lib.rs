//! NES Emulator Core - a portable NES emulation library
//!
//! This crate provides the core emulation components:
//! - CPU: 6502 processor emulation
//! - PPU: Picture Processing Unit for graphics
//! - APU: Audio Processing Unit for sound
//! - Bus: Memory bus interface
//! - Mapper: Cartridge mapper interface

#![cfg_attr(not(feature = "std"), no_std)]

pub mod apu;
pub mod bus;
pub mod cpu;
pub mod mapper;
pub mod nes;
pub mod ppu;
pub mod state;

pub use bus::Bus;
pub use mapper::Mapper;
pub use nes::Nes;

/// Result type for emulator operations
pub type Result<T> = core::result::Result<T, NesError>;

/// Error types for the emulator
#[derive(Debug, Clone, thiserror::Error)]
pub enum NesError {
    #[error("Invalid ROM format: {0}")]
    InvalidRomFormat(String),
    #[error("Unsupported mapper: {0}")]
    UnsupportedMapper(u16),
    #[error("Memory access error at address {0:#04X}")]
    MemoryAccessError(u16),
    #[error("IO error: {0}")]
    IoError(String),
}

/// NES system configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Region/TV system
    pub region: Region,
    /// Enable audio
    pub enable_audio: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            region: Region::Ntsc,
            enable_audio: true,
        }
    }
}

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

    /// Get the frame rate
    pub fn fps(&self) -> f64 {
        match self {
            Region::Ntsc => 60.0,
            Region::Pal => 50.0,
        }
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
        let scanlines = self.last_scanline() as u32 + 1;
        scanlines * 341 / 3
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
        assert_eq!(Region::Pal.fps(), 50.0);
    }
}
