//! WebAssembly bindings for the NES core.
//!
//! Every method here is a thin pass-through to [`nes_core::Nes`]; the emulator
//! itself lives in the core crate so it can also back native front ends and
//! tests.

use nes_core::nes::{
    Nes, BUTTON_A, BUTTON_B, BUTTON_DOWN, BUTTON_LEFT, BUTTON_RIGHT, BUTTON_SELECT, BUTTON_START,
    BUTTON_UP,
};
use wasm_bindgen::prelude::*;

/// WASM-exposed NES emulator.
#[wasm_bindgen]
pub struct JsNes {
    nes: Nes,
}

impl Default for JsNes {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl JsNes {
    /// Create a new NES emulator instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        #[cfg(feature = "console_error_panic_hook")]
        console_error_panic_hook::set_once();

        Self { nes: Nes::new() }
    }

    /// Load a ROM from byte data.
    ///
    /// Returns an error message when the image is not a usable iNES file.
    #[wasm_bindgen]
    pub fn load_rom(&mut self, data: &[u8]) -> core::result::Result<(), String> {
        self.nes.load_rom(data).map_err(|e| format!("{e:?}"))
    }

    /// Execute one frame of emulation.
    #[wasm_bindgen]
    pub fn frame(&mut self) {
        self.nes.frame();
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

    /// Audio samples captured during the last frame, interleaved left/right.
    #[wasm_bindgen]
    pub fn get_audio_samples(&self) -> Vec<f32> {
        self.nes.audio_samples().to_vec()
    }

    /// Pointer to the audio sample buffer.
    #[wasm_bindgen]
    pub fn get_audio_samples_ptr(&self) -> *const f32 {
        self.nes.audio_samples_ptr()
    }

    /// Number of audio samples captured during the last frame.
    #[wasm_bindgen]
    pub fn get_audio_samples_len(&self) -> usize {
        self.nes.audio_samples().len()
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
