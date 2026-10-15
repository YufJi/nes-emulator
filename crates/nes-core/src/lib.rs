//! NES Emulator Core - a portable NES emulation library
//!
//! This crate provides the core emulation components:
//! - CPU: 6502 processor emulation
//! - PPU: Picture Processing Unit for graphics
//! - APU: Audio Processing Unit for sound
//! - Bus: Memory bus interface
//! - Mapper: Cartridge mapper interface
//!
//! A personal, non-commercial study project: it ships no ROM or game files and
//! is not affiliated with Nintendo. The full statement is `DISCLAIMER.md` in the
//! repository root.

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
pub use nes::{Nes, PpuBusSample, DEFAULT_PPU_BUS_TRACE_DOTS};
pub use ppu::rendering::{PixelFormat, HEIGHT, PIXEL_FORMAT, WIDTH};

/// The allocation-backed items that the `std` prelude normally injects.
///
/// Every module does `use crate::prelude::*;` so the same source builds with or
/// without `std`; under `std` these names resolve to the exact same types.
pub(crate) mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::collections::VecDeque;
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
    /// Rate the core resamples its audio to, in Hz.
    ///
    /// Set this to the audio device's own rate so the host never has to
    /// resample: a 44.1 kHz device asks for 44_100 and reads the result with
    /// [`Nes::fill_audio`]. Zero means "use the default" (48_000).
    pub sample_rate: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            region_override: None,
            enable_audio: true,
            sample_rate: DEFAULT_SAMPLE_RATE,
        }
    }
}

/// Sample rate used when [`Config::sample_rate`] is left at zero.
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

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

    /// Master-clock ticks one CPU cycle takes.
    ///
    /// The machine keeps a single master clock the way Mesen does — the NTSC
    /// colour crystal, 21.477 MHz — and derives both chips from it: a CPU cycle
    /// advances it by this many ticks and a PPU dot by [`Region::ppu_divider`].
    /// Keeping the ratio as integers rather than a float is what lets a region
    /// whose PPU:CPU ratio is not a whole number of dots be expressed exactly
    /// instead of drifting.
    ///
    /// PAL's 16 over the PPU's 5 is the real board's 3.2:1, so a PAL CPU cycle
    /// is not a whole number of PPU dots and the machine runs its
    /// fractional-dot path (`NesBus::has_whole_dot_ratio`).
    pub fn cpu_divider(&self) -> u32 {
        match self {
            Region::Ntsc => 12,
            Region::Pal => 16,
        }
    }

    /// Master-clock ticks one PPU dot takes.
    pub fn ppu_divider(&self) -> u32 {
        match self {
            Region::Ntsc => 4,
            Region::Pal => 5,
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

    /// CPU cycles in one frame: 341 PPU dots per scanline, `cpu_divider /
    /// ppu_divider` dots per CPU cycle. PAL's 3.2 means a frame is not a whole
    /// number of CPU cycles, so this rounds down and the run loop's bound
    /// admits the cycle the boundary falls in.
    pub fn cpu_cycles_per_frame(&self) -> u32 {
        self.scanlines() as u32 * PPU_CYCLES_PER_SCANLINE * self.ppu_divider() / self.cpu_divider()
    }

    /// PPU dots that go by in one CPU cycle, as the real ratio rather than as a
    /// whole number: 3 on NTSC, 3.2 on PAL.
    pub fn dots_per_cpu_cycle(&self) -> f64 {
        self.cpu_divider() as f64 / self.ppu_divider() as f64
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
        // PAL's 3.2 dots per cycle makes a frame 33247.5 cycles; the count
        // rounds down and the run loop admits the cycle the boundary is in.
        assert_eq!(Region::Pal.cpu_cycles_per_frame(), 33247);
        assert_eq!(Region::Pal.dots_per_cpu_cycle(), 3.2);

        assert!(Region::Ntsc.cpu_cycles_per_frame() < Region::Pal.cpu_cycles_per_frame());

        // The frame rate comes from the PPU clock, so it is not exactly 60/50.
        assert_eq!(Region::Ntsc.scanlines(), 262);
        assert_eq!(Region::Pal.scanlines(), 312);
        assert!(
            (Region::Ntsc.fps() - 60.098).abs() < 1e-3,
            "{}",
            Region::Ntsc.fps()
        );
        assert!(
            (Region::Pal.fps() - 50.007).abs() < 1e-3,
            "{}",
            Region::Pal.fps()
        );

        // Front-end pacing must not assume a nominal 60 Hz.
        assert!(Region::Ntsc.fps() > 60.0);
        assert!(Region::Pal.fps() > 50.0);
    }

    /// The divider pair is the machine's whole notion of "how fast the two
    /// chips run relative to each other", so it has to agree with the frame
    /// geometry the core publishes.
    #[test]
    fn test_clock_dividers_agree_with_the_frame_geometry() {
        for region in [Region::Ntsc, Region::Pal] {
            let dots_per_cycle = region.ppu_divider();
            // NTSC is exactly 12/4 = 3; PAL is the real board's 16/5 = 3.2,
            // which is what puts a PAL cycle out of step with the dot grid.
            let expected = match region {
                Region::Ntsc => dots_per_cycle * 3,
                Region::Pal => dots_per_cycle * 16 / 5,
            };
            assert_eq!(region.cpu_divider(), expected);

            // A frame is `scanlines * 341` dots, and a whole number of CPU
            // cycles covers all of it bar the division's remainder.
            let frame_dots = region.scanlines() as u64 * PPU_CYCLES_PER_SCANLINE as u64;
            let covered = region.cpu_cycles_per_frame() as u64 * region.cpu_divider() as u64;
            let frame_ticks = frame_dots * region.ppu_divider() as u64;
            assert!(
                covered <= frame_ticks && frame_ticks - covered < region.cpu_divider() as u64,
                "{}: {covered} ticks of a {frame_ticks}-tick frame",
                region.fps()
            );
        }
    }

    /// NTSC's dividers are the real crystal's, scaled so the ratio is exact.
    #[test]
    fn test_ntsc_dividers_mirror_the_real_crystal() {
        // 21.477272 MHz / 1.789773 MHz = 12, and 21.477272 / 4 = 5.369318 MHz.
        assert_eq!(Region::Ntsc.cpu_divider(), 12);
        assert_eq!(Region::Ntsc.ppu_divider(), 4);
        let from_ppu = Region::Ntsc.ppu_clock_rate() as u64 * Region::Ntsc.ppu_divider() as u64;
        let from_cpu = Region::Ntsc.cpu_clock_rate() as u64 * Region::Ntsc.cpu_divider() as u64;
        assert!(
            from_ppu.abs_diff(from_cpu) <= Region::Ntsc.cpu_divider() as u64,
            "both chips are derived from the same crystal: {from_ppu} vs {from_cpu} \
             (the published rates are rounded to whole Hz)"
        );
    }
}
