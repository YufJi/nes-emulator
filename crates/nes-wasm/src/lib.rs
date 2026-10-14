//! WebAssembly bindings for the NES core.
//!
//! Every method here is a thin pass-through to [`nes_core::Nes`]; the emulator
//! itself lives in the core crate so it can also back native front ends and
//! tests.
//!
//! Personal, non-commercial study project (see `DISCLAIMER.md` in the repository
//! root): no ROM or game files are provided, and there is no Nintendo
//! affiliation.

use nes_core::nes::{
    Nes as CoreNes, BUTTON_A, BUTTON_B, BUTTON_DOWN, BUTTON_LEFT, BUTTON_RIGHT, BUTTON_SELECT,
    BUTTON_START, BUTTON_UP,
};
use nes_core::{supported_mappers, HEIGHT, PIXEL_FORMAT, WIDTH};
use nes_core::{FrameClock as CoreFrameClock, Region, RenderTiming as CoreRenderTiming};
use wasm_bindgen::prelude::*;

/// WASM-exposed NES emulator.
#[wasm_bindgen]
pub struct Nes {
    nes: CoreNes,
}

/// Host-agnostic fixed-timestep pacer, shared by every front end.
///
/// Feed it the same monotonic timestamps your display link or timer provides
/// (milliseconds) and it reports how many frames to emulate, absorbing
/// catch-up bursts and measuring the achieved frame rate. Build it with
/// `Nes.get_frame_rate()` so PAL cartridges run at their own speed.
#[wasm_bindgen]
pub struct FrameClock {
    clock: CoreFrameClock,
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new(Region::Ntsc.fps())
    }
}

#[wasm_bindgen]
impl FrameClock {
    /// Create a clock pacing at `frame_rate` Hz.
    #[wasm_bindgen(constructor)]
    pub fn new(frame_rate: f64) -> Self {
        Self {
            clock: CoreFrameClock::new(frame_rate),
        }
    }

    /// Frames due now, given a monotonic timestamp in milliseconds.
    ///
    /// Zero means nothing is due yet. The first call only seeds the clock.
    #[wasm_bindgen]
    pub fn frames_due(&mut self, now_ms: f64) -> u32 {
        self.clock.frames_due(now_ms)
    }

    /// Frame rate measured over the last sample window, in Hz (0 until measured).
    #[wasm_bindgen]
    pub fn fps(&self) -> f64 {
        self.clock.fps()
    }

    /// The rate this clock paces at, in Hz.
    #[wasm_bindgen]
    pub fn frame_rate(&self) -> f64 {
        self.clock.frame_rate()
    }

    /// Retarget the clock, e.g. after loading a PAL cartridge.
    #[wasm_bindgen]
    pub fn set_frame_rate(&mut self, frame_rate: f64) {
        self.clock.set_frame_rate(frame_rate);
    }

    /// Re-seed the clock after loading a ROM, resuming, or returning to the
    /// foreground, so the elapsed pause is not emulated.
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.clock.reset();
    }
}

impl Default for Nes {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Nes {
    /// Create a new NES emulator instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        #[cfg(feature = "console_error_panic_hook")]
        console_error_panic_hook::set_once();

        Self { nes: CoreNes::new() }
    }

    /// Load a ROM from byte data.
    ///
    /// Returns an error message when the image is not a usable iNES file.
    #[wasm_bindgen]
    pub fn load_rom(&mut self, data: &[u8]) -> core::result::Result<(), String> {
        self.nes.load_rom(data).map_err(|e| format!("{e:?}"))
    }

    /// Execute one frame of emulation.
    ///
    /// Runs to the end of the frame even when the cartridge has hung: a KIL
    /// opcode stops the CPU but not the machine. Poll [`Nes::is_cpu_halted`] to
    /// tell the player the game is stuck. Nothing in the frame path panics on
    /// emulated code — `wasm32-unknown-unknown` aborts on panic, which would
    /// take the whole instance with it (see `docs/architecture.md` §15.1).
    #[wasm_bindgen]
    pub fn frame(&mut self) {
        self.nes.frame();
    }

    /// Whether a KIL/JAM opcode has stopped the CPU.
    ///
    /// Frames keep completing, but no further instruction is fetched, so the
    /// picture will not change again.
    #[wasm_bindgen]
    pub fn is_cpu_halted(&self) -> bool {
        self.nes.is_cpu_halted()
    }

    /// When the renderer draws each scanline.
    ///
    /// `true` draws a line at its start (only the state as the line begins
    /// counts), `false` at dot 256 (a write earlier in the line still affects
    /// it). A whole scanline is drawn at once, so this is the only way to
    /// choose which side of a write a line falls on — a per-game compatibility
    /// switch for raster effects, not an accuracy setting.
    #[wasm_bindgen]
    pub fn set_render_timing(&mut self, scanline_start: bool) {
        self.nes.set_render_timing(if scanline_start {
            CoreRenderTiming::ScanlineStart
        } else {
            CoreRenderTiming::ScanlineEnd
        });
    }

    /// Run at most `cycles` CPU cycles, stopping early at a frame boundary.
    ///
    /// Returns the cycles actually run; check `is_frame_ready` afterwards, then
    /// pull the audio with `fill_audio`.
    #[wasm_bindgen]
    pub fn run_cycles(&mut self, cycles: u32) -> u32 {
        self.nes.run_cycles(cycles)
    }

    /// Execute a single CPU instruction, returning the cycles it consumed.
    #[wasm_bindgen]
    pub fn step_instruction(&mut self) -> u8 {
        self.nes.step_instruction()
    }

    /// Drop every queued audio sample, e.g. when loading a cartridge.
    #[wasm_bindgen]
    pub fn clear_audio_samples(&mut self) {
        self.nes.clear_audio_samples();
    }

    /// Frame rate of the loaded cartridge in Hz (NTSC ~60.0985, PAL ~50.0067).
    #[wasm_bindgen]
    pub fn get_frame_rate(&self) -> f64 {
        self.nes.frame_rate()
    }

    /// Current frame as ARGB pixels.
    #[wasm_bindgen]
    pub fn get_frame_buffer(&self) -> Vec<u32> {
        self.nes.frame_buffer().to_vec()
    }

    /// Pointer to the frame buffer, for zero-copy access from JavaScript.
    #[wasm_bindgen]
    pub fn get_frame_buffer_ptr(&self) -> *const u32 {
        self.nes.frame_buffer_ptr()
    }

    /// Frame buffer length in pixels.
    #[wasm_bindgen]
    pub fn get_frame_buffer_len(&self) -> usize {
        self.nes.frame_buffer().len()
    }

    /// Set a button state. `player` is 1 or 2, `button` is a `BUTTON_*` index.
    #[wasm_bindgen]
    pub fn set_button(&mut self, player: u8, button: u8, pressed: bool) {
        self.nes.set_button(player, button, pressed);
    }

    /// Press a button (convenience for `set_button`).
    #[wasm_bindgen]
    pub fn button_down(&mut self, player: u8, button: u8) {
        self.nes.set_button(player, button, true);
    }

    /// Release a button (convenience for `set_button`).
    #[wasm_bindgen]
    pub fn button_up(&mut self, player: u8, button: u8) {
        self.nes.set_button(player, button, false);
    }

    /// Rate the core resamples audio to, in Hz. Size device buffers from this.
    #[wasm_bindgen]
    pub fn get_audio_sample_rate(&self) -> u32 {
        self.nes.audio_sample_rate()
    }

    /// Retarget the audio resampler, e.g. to a 44_100 Hz device.
    ///
    /// Set this before loading a cartridge; queued samples are dropped.
    #[wasm_bindgen]
    pub fn set_audio_sample_rate(&mut self, sample_rate: u32) {
        self.nes.set_sample_rate(sample_rate);
    }

    /// Stereo frames emulated but not yet pulled with `fill_audio`.
    #[wasm_bindgen]
    pub fn get_buffered_audio_frames(&self) -> usize {
        self.nes.buffered_audio_frames()
    }

    /// Copy queued audio into `out`, interleaved left/right, and return how
    /// many floats were written.
    ///
    /// The audio-device callback shape: the caller owns the buffer, asks for a
    /// device period's worth, and gets back a count that is zero when nothing
    /// was produced and less than `out.length` when only part was available.
    /// Samples are already resampled to `get_audio_sample_rate()`.
    #[wasm_bindgen]
    pub fn fill_audio(&mut self, out: &mut [f32]) -> usize {
        self.nes.fill_audio(out)
    }

    /// Reset the console, as if the reset button was pressed.
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.nes.reset();
    }

    /// Whether the loaded cartridge keeps battery-backed save RAM.
    #[wasm_bindgen]
    pub fn has_battery(&self) -> bool {
        self.nes.has_battery()
    }

    /// Snapshot the cartridge's battery RAM so the host can persist it.
    #[wasm_bindgen]
    pub fn get_save_ram(&self) -> Vec<u8> {
        self.nes.save_ram().unwrap_or_default()
    }

    /// Restore a snapshot previously returned by `get_save_ram`.
    #[wasm_bindgen]
    pub fn set_save_ram(&mut self, data: &[u8]) {
        self.nes.load_save_ram(data);
    }

    /// Serialize the whole machine (CPU, PPU, APU, cartridge) into a save state.
    ///
    /// The PRG-ROM image is not included, so a state must be applied to the
    /// same cartridge it was taken from.
    #[wasm_bindgen]
    pub fn save_state(&self) -> Vec<u8> {
        self.nes.save_state()
    }

    /// Restore a state produced by `save_state`.
    #[wasm_bindgen]
    pub fn load_state(&mut self, data: &[u8]) {
        self.nes.load_state(data);
    }

    /// Frames rendered so far.
    #[wasm_bindgen]
    pub fn get_frame_number(&self) -> u64 {
        self.nes.frame_number()
    }

    /// Whether a frame finished since the last `clear_frame_ready`.
    #[wasm_bindgen]
    pub fn is_frame_ready(&self) -> bool {
        self.nes.is_frame_ready()
    }

    /// Clear the frame-ready flag.
    #[wasm_bindgen]
    pub fn clear_frame_ready(&mut self) {
        self.nes.clear_frame_ready();
    }
}

/// Button index for A.
#[wasm_bindgen]
pub fn get_button_a() -> u8 {
    BUTTON_A
}

/// Button index for B.
#[wasm_bindgen]
pub fn get_button_b() -> u8 {
    BUTTON_B
}

/// Button index for Select.
#[wasm_bindgen]
pub fn get_button_select() -> u8 {
    BUTTON_SELECT
}

/// Button index for Start.
#[wasm_bindgen]
pub fn get_button_start() -> u8 {
    BUTTON_START
}

/// Button index for Up.
#[wasm_bindgen]
pub fn get_button_up() -> u8 {
    BUTTON_UP
}

/// Button index for Down.
#[wasm_bindgen]
pub fn get_button_down() -> u8 {
    BUTTON_DOWN
}

/// Button index for Left.
#[wasm_bindgen]
pub fn get_button_left() -> u8 {
    BUTTON_LEFT
}

/// Button index for Right.
#[wasm_bindgen]
pub fn get_button_right() -> u8 {
    BUTTON_RIGHT
}

/// Width of the frame buffer in pixels.
#[wasm_bindgen]
pub fn get_frame_width() -> usize {
    WIDTH
}

/// Height of the frame buffer in pixels.
#[wasm_bindgen]
pub fn get_frame_height() -> usize {
    HEIGHT
}

/// Layout of the pixels returned by `Nes.get_frame_buffer`, e.g. `"argb8888"`.
#[wasm_bindgen]
pub fn get_pixel_format() -> String {
    PIXEL_FORMAT.id().to_string()
}

/// Bit position of each channel inside a pixel, as `[red, green, blue, alpha]`.
#[wasm_bindgen]
pub fn get_pixel_shifts() -> Vec<u32> {
    PIXEL_FORMAT.channel_shifts().to_vec()
}

/// Mapper numbers the core can emulate, ascending.
#[wasm_bindgen]
pub fn get_supported_mappers() -> Vec<u16> {
    supported_mappers().to_vec()
}
