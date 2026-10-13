//! UniFFI bindings for the NES core — the Swift (iOS/macOS) and Kotlin (Android)
//! entry point.
//!
//! This crate is a binding layer, not an emulator: every method forwards to
//! `nes-core` and holds no emulation logic, exactly like `nes-wasm` does for the
//! web. The bindings therefore publish the same contract, and a host that ports
//! between them only has to re-learn the syntax.
//!
//! Personal, non-commercial study project (see `DISCLAIMER.md` in the repository
//! root): no ROM or game files are provided, and there is no Nintendo
//! affiliation.
//!
//! # Shape of the API
//!
//! * [`Nes`] and [`FrameClock`] are UniFFI *objects*: reference-counted handles
//!   the foreign side owns. The machine is meant to live on the host's emulation
//!   thread (see `docs/architecture.md` §10.5); the wrapper still takes a lock,
//!   so a call from the wrong thread is serialized rather than unsafe — and a
//!   panic while holding it does not brick the handle.
//! * [`Config`], [`Region`] and [`NesError`] are value types: a record, an enum
//!   and a switchable error.
//! * Free functions publish what every host needs up front: frame size, pixel
//!   layout, the mapper list and the default sample rate.
//!
//! Panics cannot cross the boundary: UniFFI catches them and re-raises them as
//! an unexpected error on the foreign side, so a corrupt cartridge that trips
//! the CPU is reported instead of aborting the process.
//!
//! # Generating the bindings
//!
//! ```text
//! cargo build -p nes-uniffi                    # cdylib + staticlib
//! cargo run -p nes-uniffi --bin uniffi-bindgen -- \
//!     generate --library target/debug/libnes_uniffi.dylib \
//!     --language swift --out-dir out/swift
//! ```
//!
//! Swap `--language kotlin` for Android, and point `--library` at the artifact
//! of the target you are shipping (an iOS device build produces a `.a`).

use std::sync::{Arc, Mutex, MutexGuard};

use nes_core::{
    Config as CoreConfig, FrameClock as CoreClock, Nes as CoreNes, Region as CoreRegion,
};

uniffi::setup_scaffolding!();

/// The console's TV system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Region {
    /// NTSC: 60.0985 Hz, 262 scanlines per frame.
    Ntsc,
    /// PAL: 50.0067 Hz, 312 scanlines per frame.
    Pal,
}

impl From<CoreRegion> for Region {
    fn from(region: CoreRegion) -> Self {
        match region {
            CoreRegion::Ntsc => Region::Ntsc,
            CoreRegion::Pal => Region::Pal,
        }
    }
}

impl From<Region> for CoreRegion {
    fn from(region: Region) -> Self {
        match region {
            Region::Ntsc => CoreRegion::Ntsc,
            Region::Pal => CoreRegion::Pal,
        }
    }
}

/// Host-side settings, applied when a cartridge is loaded.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Config {
    /// Force a TV system instead of trusting the cartridge header.
    pub region_override: Option<Region>,
    /// Buffer audio for the host to pull with [`Nes::fill_audio`].
    pub enable_audio: bool,
    /// Sample rate the core resamples to, in Hz; `0` keeps the default.
    pub sample_rate: u32,
}

impl Default for Config {
    fn default() -> Self {
        let core = CoreConfig::default();
        Self {
            region_override: core.region_override.map(Region::from),
            enable_audio: core.enable_audio,
            sample_rate: core.sample_rate,
        }
    }
}

impl From<Config> for CoreConfig {
    fn from(config: Config) -> Self {
        CoreConfig {
            region_override: config.region_override.map(CoreRegion::from),
            enable_audio: config.enable_audio,
            sample_rate: config.sample_rate,
        }
    }
}

/// Everything the core can fail at, in a form Swift and Kotlin can switch on.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum NesError {
    /// The image is not a usable iNES file.
    InvalidRomFormat { message: String },
    /// The cartridge uses a mapper the core does not implement yet.
    UnsupportedMapper { mapper: u16 },
    /// A memory access fell outside anything the machine can answer.
    MemoryAccessError { address: u16 },
    /// A host-side I/O operation failed.
    IoError { message: String },
    /// The emulated CPU panicked mid-instruction.
    ///
    /// The core deliberately panics on an undefined opcode instead of quietly
    /// treating it as a no-op, so a corrupt cartridge fails loudly. The binding
    /// catches that panic and hands it back as this error; the machine is left
    /// mid-instruction, so reload the cartridge before running another frame.
    EmulationPanic { message: String },
}

impl std::fmt::Display for NesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NesError::InvalidRomFormat { message } => write!(f, "invalid iNES image: {message}"),
            NesError::UnsupportedMapper { mapper } => write!(f, "unsupported mapper {mapper}"),
            NesError::MemoryAccessError { address } => {
                write!(f, "memory access error at {address:#06X}")
            }
            NesError::IoError { message } => write!(f, "host I/O error: {message}"),
            NesError::EmulationPanic { message } => write!(f, "emulation panic: {message}"),
        }
    }
}

impl std::error::Error for NesError {}

impl From<nes_core::NesError> for NesError {
    fn from(error: nes_core::NesError) -> Self {
        match error {
            nes_core::NesError::InvalidRomFormat(message) => NesError::InvalidRomFormat { message },
            nes_core::NesError::UnsupportedMapper(mapper) => NesError::UnsupportedMapper { mapper },
            nes_core::NesError::MemoryAccessError(address) => {
                NesError::MemoryAccessError { address }
            }
            nes_core::NesError::IoError(message) => NesError::IoError { message },
        }
    }
}

/// Run `body`, turning a Rust panic into a catchable error.
///
/// The CPU deliberately panics on an undefined opcode (see [`NesError::EmulationPanic`]).
/// Left alone that panic unwinds into UniFFI's generated wrapper, which reports
/// it as `UniffiInternalError.rustPanic` — and for a method whose Rust signature
/// returns no `Result`, the generated Swift wrapper calls `try!` on that, which
/// traps and takes the whole app down. Catching here keeps the failure catchable
/// and typed, at the price of leaving the machine mid-instruction: the host is
/// told to reload the cartridge.
///
/// This only works while panics unwind; building with `panic = "abort"` turns
/// them into an immediate process abort that no binding can intercept.
fn guard<T>(body: impl FnOnce() -> T) -> Result<T, NesError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).map_err(|payload| {
        NesError::EmulationPanic {
            message: panic_message(payload),
        }
    })
}

/// The message out of a panic payload, if it carried one.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&'static str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "panic without a message".to_string()
    }
}

/// An emulated NES console.
#[derive(uniffi::Object)]
pub struct Nes {
    inner: Mutex<CoreNes>,
}

impl Nes {
    /// Lock the machine, recovering from a poisoned mutex.
    ///
    /// A panic while the lock was held (the CPU panics on an undefined opcode)
    /// leaves it poisoned; refusing every later call would turn one bad frame
    /// into a permanently dead handle. The machine may be mid-instruction, so
    /// the host should reload the cartridge — but it stays usable.
    fn machine(&self) -> MutexGuard<'_, CoreNes> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[uniffi::export]
impl Nes {
    /// A console with no cartridge, using the default [`Config`].
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Self::with_config(Config::default())
    }

    /// A console with no cartridge, using `config`.
    #[uniffi::constructor]
    pub fn with_config(config: Config) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(CoreNes::with_config(config.into())),
        })
    }

    /// Load an iNES cartridge image and reset the console.
    ///
    /// Restore the cartridge's battery RAM with [`Nes::load_save_ram`] before
    /// running the first frame: boot code can read its save that early.
    pub fn load_rom(&self, data: Vec<u8>) -> Result<(), NesError> {
        guard(|| self.machine().load_rom(&data).map_err(NesError::from))?
    }

    /// Reset the console, as if the reset button was pressed.
    pub fn reset(&self) -> Result<(), NesError> {
        guard(|| self.machine().reset())
    }

    /// Run one frame of emulation.
    ///
    /// Audio produced along the way stays queued for [`Nes::fill_audio`].
    ///
    /// Fails with [`NesError::EmulationPanic`] if the cartridge sent the CPU
    /// into an undefined opcode; reload it before the next frame.
    pub fn frame(&self) -> Result<(), NesError> {
        guard(|| self.machine().frame())
    }

    /// Run at most `cycles` CPU cycles, stopping early at a frame boundary.
    ///
    /// Returns the cycles actually run; check [`Nes::is_frame_ready`] afterwards.
    pub fn run_cycles(&self, cycles: u32) -> Result<u32, NesError> {
        guard(|| self.machine().run_cycles(cycles))
    }

    /// Execute a single CPU instruction, returning the cycles it consumed.
    pub fn step_instruction(&self) -> Result<u8, NesError> {
        guard(|| self.machine().step_instruction())
    }

    /// The last completed frame, as `0xAARRGGBB` pixels, row-major.
    ///
    /// Sized [`frame_width`] × [`frame_height`]. On a little-endian host those
    /// bytes are already BGRA in memory, which is what Metal's
    /// `MTLPixelFormatBGRA8Unorm` expects, so the buffer can be uploaded as-is.
    pub fn frame_buffer(&self) -> Vec<u32> {
        self.machine().frame_buffer().to_vec()
    }

    /// Width of one frame in pixels.
    pub fn frame_width(&self) -> u32 {
        nes_core::WIDTH as u32
    }

    /// Height of one frame in pixels.
    pub fn frame_height(&self) -> u32 {
        nes_core::HEIGHT as u32
    }

    /// Frames rendered so far.
    pub fn frame_number(&self) -> u64 {
        self.machine().frame_number()
    }

    /// Whether a frame finished since the last [`Nes::clear_frame_ready`].
    pub fn is_frame_ready(&self) -> bool {
        self.machine().is_frame_ready()
    }

    /// Clear the frame-ready flag.
    pub fn clear_frame_ready(&self) {
        self.machine().clear_frame_ready();
    }

    /// Frame rate of the loaded cartridge in Hz (NTSC ~60.0985, PAL ~50.0067).
    ///
    /// Pace with this rather than a hardcoded 60: hand it to [`FrameClock`].
    pub fn frame_rate(&self) -> f64 {
        self.machine().frame_rate()
    }

    /// TV system of the loaded cartridge.
    pub fn region(&self) -> Region {
        self.machine().region().into()
    }

    /// Set a controller button state; `player` is 1 or 2.
    ///
    /// `button` is one of the `button_*` numbers exported by this crate.
    pub fn set_button(&self, player: u8, button: u8, pressed: bool) {
        self.machine().set_button(player, button, pressed);
    }

    /// Rate the core resamples audio to, in Hz. Size device buffers from this.
    pub fn audio_sample_rate(&self) -> u32 {
        self.machine().audio_sample_rate()
    }

    /// Change the audio sample rate; `0` keeps the current one.
    ///
    /// Queued samples were resampled to the old rate and are dropped. Call this
    /// when the audio route changes, not once per frame.
    pub fn set_sample_rate(&self, sample_rate: u32) {
        self.machine().set_sample_rate(sample_rate);
    }

    /// Stereo frames emulated but not yet pulled with [`Nes::fill_audio`].
    pub fn buffered_audio_frames(&self) -> u64 {
        self.machine().buffered_audio_frames() as u64
    }

    /// Drain up to `max_samples` interleaved stereo samples (left, right, …).
    ///
    /// The audio-device callback shape: ask for a period's worth and get back
    /// what was available — an empty array when nothing was produced, a shorter
    /// one when only part was. The count is always a whole number of stereo
    /// frames, so it is even.
    ///
    /// Call this on the emulation thread and hand the result to the audio
    /// thread through the host's own queue; the core is not a cross-thread
    /// buffer.
    pub fn fill_audio(&self, max_samples: u32) -> Vec<f32> {
        let mut out = vec![0.0f32; max_samples as usize];
        let written = self.machine().fill_audio(&mut out);
        out.truncate(written);
        out
    }

    /// Drop every queued audio sample, e.g. when loading a cartridge.
    pub fn clear_audio_samples(&self) {
        self.machine().clear_audio_samples();
    }

    /// Whether the cartridge keeps battery-backed save RAM.
    pub fn has_battery(&self) -> bool {
        self.machine().has_battery()
    }

    /// Snapshot the cartridge's battery RAM, or `None` when it has none.
    pub fn save_ram(&self) -> Option<Vec<u8>> {
        self.machine().save_ram()
    }

    /// Restore a snapshot produced by [`Nes::save_ram`].
    pub fn load_save_ram(&self, data: Vec<u8>) {
        self.machine().load_save_ram(&data);
    }

    /// Serialize the whole machine (CPU, PPU, APU, cartridge) into a save state.
    ///
    /// The PRG-ROM image is not included, so a state only applies to the same
    /// cartridge it was taken from.
    pub fn save_state(&self) -> Vec<u8> {
        self.machine().save_state()
    }

    /// Restore a state produced by [`Nes::save_state`].
    pub fn load_state(&self, data: Vec<u8>) {
        self.machine().load_state(&data);
    }

    /// Read a byte straight off the system bus; for diagnostics.
    pub fn peek(&self, address: u16) -> u8 {
        self.machine().peek(address)
    }

    /// Address of the core's own frame buffer.
    ///
    /// With [`Nes::frame_buffer_len`] this lets a host hand the picture to the
    /// GPU **without copying it**: wrap the address in
    /// `MTLBuffer(bytesNoCopy:length:options:deallocator:)` on Apple platforms,
    /// or build a `CGDataProvider` over it. The bytes are `0xAARRGGBB`, which on
    /// a little-endian host is `B, G, R, A` in memory — exactly
    /// `MTLPixelFormatBGRA8Unorm`.
    ///
    /// Read the address again on every frame rather than caching it:
    /// [`Nes::load_state`] may resize the buffer. Its *contents* are valid until
    /// the next call that advances or replaces the machine
    /// ([`Nes::frame`], [`Nes::run_cycles`], [`Nes::step_instruction`],
    /// [`Nes::load_rom`], [`Nes::load_state`]).
    ///
    /// Copying is the safer default: [`Nes::frame_buffer`] returns pixels the
    /// host owns, with no lifetime attached.
    pub fn frame_buffer_ptr(&self) -> u64 {
        self.machine().frame_buffer_ptr() as u64
    }

    /// Pixels in the frame buffer: `frame_width * frame_height`.
    pub fn frame_buffer_len(&self) -> u64 {
        self.machine().frame_buffer().len() as u64
    }
}

/// Fixed-timestep pacer shared by every front end.
///
/// Feed it the timestamps your display link already gives you (milliseconds)
/// and it reports how many frames to emulate, absorbing catch-up bursts,
/// dropping what it cannot repay and measuring the achieved frame rate.
#[derive(uniffi::Object)]
pub struct FrameClock {
    inner: Mutex<CoreClock>,
}

impl FrameClock {
    fn lock(&self) -> MutexGuard<'_, CoreClock> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[uniffi::export]
impl FrameClock {
    /// A clock pacing at `frame_rate` Hz.
    #[uniffi::constructor]
    pub fn new(frame_rate: f64) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(CoreClock::new(frame_rate)),
        })
    }

    /// A clock pacing at the console's own rate for `region`.
    #[uniffi::constructor]
    pub fn for_region(region: Region) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(CoreClock::for_region(region.into())),
        })
    }

    /// Frames due now, from a monotonic timestamp in milliseconds.
    ///
    /// Zero means nothing is due yet; the first call only seeds the clock.
    pub fn frames_due(&self, now_ms: f64) -> u32 {
        self.lock().frames_due(now_ms)
    }

    /// Frame rate measured over the last sample window, in Hz (0 until measured).
    pub fn fps(&self) -> f64 {
        self.lock().fps()
    }

    /// The rate this clock paces at, in Hz.
    pub fn frame_rate(&self) -> f64 {
        self.lock().frame_rate()
    }

    /// Retarget the clock, e.g. after loading a PAL cartridge.
    pub fn set_frame_rate(&self, frame_rate: f64) {
        self.lock().set_frame_rate(frame_rate);
    }

    /// Re-seed the clock after loading a ROM, resuming, or returning to the
    /// foreground, so a pause is not emulated as elapsed time.
    pub fn reset(&self) {
        self.lock().reset();
    }
}

/// Width of one frame in pixels.
#[uniffi::export]
pub fn frame_width() -> u32 {
    nes_core::WIDTH as u32
}

/// Height of one frame in pixels.
#[uniffi::export]
pub fn frame_height() -> u32 {
    nes_core::HEIGHT as u32
}

/// Pixel layout of a frame, e.g. `"argb8888"`.
#[uniffi::export]
pub fn pixel_format() -> String {
    nes_core::PIXEL_FORMAT.id().to_string()
}

/// Bit position of each channel in a pixel: `[red, green, blue, alpha]`.
#[uniffi::export]
pub fn pixel_shifts() -> Vec<u32> {
    nes_core::PIXEL_FORMAT.channel_shifts().to_vec()
}

/// Mapper numbers the core can emulate, ascending.
#[uniffi::export]
pub fn supported_mappers() -> Vec<u16> {
    nes_core::supported_mappers().to_vec()
}

/// Sample rate a [`Config`] uses when it leaves `sample_rate` at zero.
#[uniffi::export]
pub fn default_sample_rate() -> u32 {
    nes_core::DEFAULT_SAMPLE_RATE
}

/// A [`Config`] with audio on, the header's region and the default sample rate.
#[uniffi::export]
pub fn default_config() -> Config {
    Config::default()
}

/// Button index for A.
#[uniffi::export]
pub fn button_a() -> u8 {
    nes_core::nes::BUTTON_A
}

/// Button index for B.
#[uniffi::export]
pub fn button_b() -> u8 {
    nes_core::nes::BUTTON_B
}

/// Button index for Select.
#[uniffi::export]
pub fn button_select() -> u8 {
    nes_core::nes::BUTTON_SELECT
}

/// Button index for Start.
#[uniffi::export]
pub fn button_start() -> u8 {
    nes_core::nes::BUTTON_START
}

/// Button index for Up.
#[uniffi::export]
pub fn button_up() -> u8 {
    nes_core::nes::BUTTON_UP
}

/// Button index for Down.
#[uniffi::export]
pub fn button_down() -> u8 {
    nes_core::nes::BUTTON_DOWN
}

/// Button index for Left.
#[uniffi::export]
pub fn button_left() -> u8 {
    nes_core::nes::BUTTON_LEFT
}

/// Button index for Right.
#[uniffi::export]
pub fn button_right() -> u8 {
    nes_core::nes::BUTTON_RIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal cartridge: one PRG bank, CHR-RAM, reset code at `$8000`.
    ///
    /// `body` fills PRG; the reset vector is patched in afterwards so the CPU
    /// always starts at `$8000`.
    fn tiny_rom_with(body: u8) -> Vec<u8> {
        // The header stays zeroed; only the PRG body is filled with `body`.
        let mut rom = vec![0u8; 16];
        rom[0..4].copy_from_slice(b"NES\x1a");
        rom[4] = 1; // one 16 KB PRG bank
        rom[5] = 0; // no CHR-ROM: the mapper backs it with CHR-RAM
        rom.resize(16 + 16 * 1024, body);
        // The reset vector lives in the last six bytes of PRG-ROM.
        rom[16 + 0x3FFC] = 0x00;
        rom[16 + 0x3FFD] = 0x80;
        rom
    }

    /// A cartridge whose reset code spins in a tight `JMP $8000` loop.
    fn tiny_rom() -> Vec<u8> {
        let mut rom = tiny_rom_with(0xEA); // NOP padding
        rom[16] = 0x4C;
        rom[17] = 0x00;
        rom[18] = 0x80;
        rom
    }

    /// A structurally valid cartridge whose PRG is nothing but `0x02`, an
    /// undefined 6502 opcode the core refuses to guess at.
    fn undefined_opcode_rom() -> Vec<u8> {
        tiny_rom_with(0x02)
    }

    #[test]
    fn a_cartridge_runs_and_produces_audio() {
        let nes = Nes::new();
        nes.load_rom(tiny_rom())
            .expect("the tiny cartridge should load");

        assert_eq!(nes.frame_number(), 0);
        nes.frame().expect("frame should run");
        assert_eq!(nes.frame_number(), 1);
        assert!(nes.is_frame_ready());
        assert_eq!(nes.frame_buffer().len(), 256 * 240);
        assert!(
            nes.buffered_audio_frames() > 100,
            "a frame should carry audio"
        );
    }

    #[test]
    fn audio_is_pulled_in_device_sized_chunks() {
        let nes = Nes::new();
        nes.load_rom(tiny_rom()).unwrap();
        nes.set_sample_rate(44_100);
        nes.frame().expect("frame should run");

        assert_eq!(nes.fill_audio(100).len(), 100, "a short request is filled");
        assert!(nes.fill_audio(4096).len() > 100);
        assert_eq!(nes.fill_audio(4096).len(), 0, "the queue drains dry");
    }

    #[test]
    fn a_malformed_cartridge_is_an_error_not_a_panic() {
        let nes = Nes::new();
        let mut rom = tiny_rom();
        rom[4] = 0; // claim zero PRG banks
        assert!(matches!(
            nes.load_rom(rom),
            Err(NesError::InvalidRomFormat { .. })
        ));
    }

    #[test]
    fn an_undefined_opcode_is_an_error_not_a_crash() {
        // This is the case that would otherwise unwind into UniFFI's `try!`
        // wrapper and take the host process down.
        let nes = Nes::new();
        nes.load_rom(undefined_opcode_rom())
            .expect("the header itself is valid");

        match nes.frame() {
            Err(NesError::EmulationPanic { message }) => {
                assert!(
                    message.contains("Invalid opcode"),
                    "the panic message should survive, got {message:?}"
                );
            }
            other => panic!("expected an emulation panic, got {other:?}"),
        }
    }

    #[test]
    fn the_console_survives_an_emulation_panic() {
        let nes = Nes::new();
        nes.load_rom(undefined_opcode_rom()).unwrap();
        assert!(nes.frame().is_err());

        // The panic poisons the mutex; the wrapper recovers, so the host can
        // simply reload the cartridge and carry on instead of rebuilding.
        nes.load_rom(tiny_rom()).expect("reloading should work");
        nes.frame().expect("the fresh cartridge should run");
        assert_eq!(nes.frame_number(), 1);
    }

    #[test]
    fn the_published_contract_matches_the_core() {
        assert_eq!(frame_width(), nes_core::WIDTH as u32);
        assert_eq!(frame_height(), nes_core::HEIGHT as u32);
        assert_eq!(pixel_format(), nes_core::PIXEL_FORMAT.id());
        assert_eq!(pixel_shifts(), nes_core::PIXEL_FORMAT.channel_shifts());
        assert_eq!(supported_mappers(), nes_core::supported_mappers());
        assert_eq!(default_sample_rate(), nes_core::DEFAULT_SAMPLE_RATE);
        assert!(default_config().enable_audio);
    }

    #[test]
    fn the_clock_releases_frames_from_timestamps() {
        let clock = FrameClock::new(100.0); // a frame every 10 ms
        assert_eq!(clock.frames_due(0.0), 0, "the first call only seeds");
        assert_eq!(clock.frames_due(10.0), 1);
        assert_eq!(clock.frames_due(13.0), 0);
        assert_eq!(clock.frames_due(33.0), 2);
    }

    #[test]
    fn the_frame_buffer_can_be_read_without_copying() {
        let nes = Nes::new();
        nes.load_rom(tiny_rom()).unwrap();
        nes.frame().expect("frame should run");

        let copied = nes.frame_buffer();
        let len = nes.frame_buffer_len() as usize;
        assert_eq!(len, copied.len(), "the pointer and the copy must agree");
        assert!(nes.frame_buffer_ptr() != 0, "a loaded console has a buffer");

        // Sound here because nothing advances the machine in between: no Rust
        // code runs concurrently, and the buffer is 256x240 of `u32`.
        let through_ptr =
            unsafe { std::slice::from_raw_parts(nes.frame_buffer_ptr() as *const u32, len) };
        assert_eq!(through_ptr, copied.as_slice());
    }
}
