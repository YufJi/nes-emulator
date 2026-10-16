//! NES system - the emulated console
//!
//! This ties the CPU, PPU, APU, cartridge mapper and controllers together and
//! drives them at the right rates: one APU tick per CPU cycle, three PPU dots
//! per CPU cycle. It is deliberately free of any host bindings so the same core
//! can back the WASM build, tests, or a native front end.

use crate::apu::Apu;
use crate::bus::Bus;
use crate::cpu::Cpu;
use crate::mapper::{create_mapper, Mapper};
use crate::ppu::pipeline::{PixelPipeline, SpriteSlot};
use crate::ppu::rendering::{FrameBuffer, MapperChr, Mirroring, VramChr};
use crate::ppu::sprite::SpriteEvaluator;
use crate::ppu::{Ppu, PpuCtrl, PpuMask, PpuMemory, PpuStatus};
use crate::prelude::*;
use crate::state::{StateReader, StateWriter};
use crate::{Config, NesError, Region, DEFAULT_SAMPLE_RATE};

/// Button indices used by [`Nes::set_button`].
pub const BUTTON_A: u8 = 0;
pub const BUTTON_B: u8 = 1;
pub const BUTTON_SELECT: u8 = 2;
pub const BUTTON_START: u8 = 3;
pub const BUTTON_UP: u8 = 4;
pub const BUTTON_DOWN: u8 = 5;
pub const BUTTON_LEFT: u8 = 6;
pub const BUTTON_RIGHT: u8 = 7;

/// One dot of the PPU's bus timeline.
///
/// This is the resolution the cartridge sees — A12 rises between *fetch
/// dots*, and the nine-dot filter a scanline counter uses is measured there —
/// so it is also the resolution the accuracy work has to be able to observe.
/// The registers travel with it: a `$2000`/`$2001` write and a sprite fetch
/// only mean anything against *the dot they landed on*, which is what
/// AccuracyCoin's `Suddenly Resize Sprite` is written to pin down. Reading the
/// sample is side-effect free, unlike `Nes::peek` (a `$2002` read clears
/// VBlank).
/// [`Nes::ppu_bus_trace`] returns the samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpuBusSample {
    /// Scanline the dot belongs to (0-239 visible, 240 post-render, then
    /// vertical blanking, with the pre-render line last).
    pub scanline: u16,
    /// Dot within the scanline (0-340).
    pub dot: u16,
    /// Address the PPU is driving, which is what a cartridge's A12 filter
    /// looks at.
    pub address: u16,
    /// `$2000` on this dot.
    pub ctrl: u8,
    /// `$2001` on this dot.
    pub mask: u8,
    /// `$2002` bits 7-5 on this dot (the flags as the register drives them).
    pub status: u8,
    /// `$2003` on this dot.
    pub oam_addr: u8,
    /// Scroll address `v`.
    pub v: u16,
    /// Scroll latch `t`.
    pub t: u16,
    /// Fine X.
    pub x: u8,
}

/// Dots of per-dot PPU bus history kept by default: a little over twelve NTSC
/// scanlines.
///
/// The trace exists to be read at dot resolution (A12 rises, the fetch a
/// mid-frame register write lands behind), and a caller that wants a whole
/// frame raises it with [`Nes::set_ppu_bus_trace_capacity`] rather than paying
/// for a frame's worth of samples on every machine.
pub const DEFAULT_PPU_BUS_TRACE_DOTS: usize = 4096;

/// One write to a PPU register, with the dot it landed on.
///
/// The sprite phase and the scroll registers are written mid-line, and "which
/// dot did that `$2001`/`$2000` write land on" is the whole question behind
/// tests such as AccuracyCoin's `Suddenly Resize Sprite`. The PPU sees the
/// write when its dot comes round, so the pair (write, dot) is the measurement
/// — Mesen's own debugging logs the same triple, with the status register as it
/// stood at that dot.
///
/// [`Nes::ppu_write_log`] returns them. Unlike the bus trace this costs a push
/// on every register write, so it is **off** until a caller asks for it with
/// [`Nes::set_ppu_write_log_capacity`]. It is host-side diagnostic state: it
/// stays out of the save state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpuWriteSample {
    /// Scanline the write landed on (0-239 visible, then post-render, vertical
    /// blanking, with the pre-render line last).
    pub scanline: u16,
    /// Dot within that scanline (0-340).
    pub dot: u16,
    /// Register address as the CPU wrote it (anywhere in `$2000-$3FFF`).
    pub addr: u16,
    /// Value written.
    pub value: u8,
    /// `$2002` as it stood on that dot.
    pub status: u8,
}

/// Magic prefix identifying a [`Nes::save_state`] blob.
const STATE_MAGIC: &[u8; 5] = b"NESST";
/// Save-state format version, bumped whenever the encoding changes.
///
/// 4: the mid-frame NMI no longer carries its own flag - the deferred register
/// write that raises it is replayed on the write's own cycle, so it needs no
/// save-state field.
/// 5: the CPU no longer latches a taken branch's poll as deferred - the poll
/// belongs to the branch itself, on its penultimate cycle.
/// 9: the renderer became per dot: the whole-line renderer's state gave way to
/// the pipeline's shift registers and sprite fetch latches.
/// 10: DMA moved into the memory-read path, so the stall count gave way to the
/// pending-transfer state the machine now carries between accesses.
/// 11: the DMC became Mesen's: the length counter moves at the fetch, the
/// fetched byte waits in the buffer until the output unit takes it, and the
/// timer's period is the table's value rather than one cycle more. The field
/// set is the same, but the fields no longer mean what a version-10 state wrote
/// into them.
/// 12: the audio path became a band-limited resampler, so the sample
/// accumulator gave way to the resampler's own phase, ring and integrator.
/// 13: the controller ports took Mesen's shape, so the polled bit gave way to
/// the strobe latch, the shift register and the pending `$4016` write.
/// 14: `$2006`/`$2007` became Mesen's register state machines, so the fields a
/// version-13 state wrote no longer describe when an access lands.
const STATE_VERSION: u8 = 14;

/// How far into a CPU cycle the chip performs a bus access: after the cycle's
/// first two PPU dots.
///
/// Measured, not chosen. It was picked out by the VBlank flag's set/clear
/// boundary and the NMI suppression window, and the per-dot renderer made it
/// discriminating: with the pipeline fetching at the dot, moving the access to
/// the first or the zeroth dot costs five ROMs (`ppu_vbl_nmi`'s
/// `05-nmi_timing`, `06-suppression` and `07-nmi_on_timing`,
/// `cpu_interrupts_v2/2-nmi_and_brk` and `mmc3_test_2/4-scanline_timing`),
/// while two passes every suite. See §7 of `docs/mesen-alignment.md`.
///
/// Saying it in dots rather than in ticks is what keeps it meaningful if a
/// cycle is ever not exactly three dots long.
const ACCESS_DOTS: u32 = 2;

/// Dummy cycles the CPU spends after a reset before the cartridge's first
/// instruction runs, which is what puts the PPU and the APU that far into a
/// frame when it does (Mesen's `NesCpu::Reset`).
const RESET_WARM_UP_CYCLES: u32 = 8;

/// Bits a NES-001's controller ports leave to the open bus (Mesen's
/// `GetOpenBusMask`; the expansion port of a Famicom drives one bit more, and
/// the AV Famicom one fewer, neither of which this core models).
const CONTROLLER_OPEN_BUS_MASK: u8 = 0xE0;

/// How much audio the core buffers before it starts dropping the oldest
/// samples, in milliseconds.
///
/// A host that stops pulling — a muted audio device, a stalled callback —
/// must not make the buffer grow without bound, and stale samples are worse
/// than none, so the oldest audio goes first.
const AUDIO_BUFFER_MS: f64 = 500.0;

/// Floats needed to hold `ms` of interleaved stereo audio at `sample_rate`.
fn audio_capacity_floats(sample_rate: u32, ms: f64) -> usize {
    (sample_rate as f64 * ms / 1000.0) as usize * 2
}

/// A `$4016` write the chip has not carried to the controllers yet.
///
/// The OUT pins update at the start of a PUT cycle, so the value waits one or
/// two CPU cycles depending on the clock's parity (Mesen's `_writePending`).
#[derive(Debug, Clone, Copy, Default)]
struct PendingControllerWrite {
    value: u8,
    cycles_left: u8,
}

crate::state::state_fields!(PendingControllerWrite { value, cycles_left });

/// The controller port read that just happened.
///
/// Reading the same port twice inside one CPU cycle clocks the shift register
/// once and answers with the same bit both times (Mesen's
/// `NesControlManager::ReadDevice`, which keeps the device's previous value for
/// exactly this case). The extra read is what a DMC DMA's halt cycle causes,
/// and it is why a game can lose input bits.
#[derive(Debug, Clone, Copy, Default)]
struct ControllerPortRead {
    addr: u16,
    cycle: u64,
    bit: u8,
}

crate::state::state_fields!(ControllerPortRead { addr, cycle, bit });

/// Controller state for one player
#[derive(Debug, Clone, Default)]
struct ControllerState {
    /// Current button states (true = pressed)
    buttons: [bool; 8],
    /// The byte being shifted out, one bit per read.
    ///
    /// Hardware reloads it from the buttons on the strobe's falling edge and
    /// (while the strobe is held high) on every read, and it shifts in **ones**
    /// — an authentic controller answers `1` once the eight buttons have been
    /// clocked out, where a third-party one may answer `0` (Mesen's
    /// `NesController::ReadRam` sets `_stateBuffer |= 0x80` after each read).
    state_buffer: u8,
    /// Whether the strobe line is high.
    strobe: bool,
}

crate::state::state_fields!(ControllerState {
    buttons,
    state_buffer,
    strobe,
});

impl ControllerState {
    fn new() -> Self {
        Self::default()
    }

    /// Set a button state
    fn set_button(&mut self, button: u8, pressed: bool) {
        if button < 8 {
            self.buttons[button as usize] = pressed;
        }
    }

    /// Load the shift register from the buttons.
    fn refresh_state_buffer(&mut self) {
        // Order: A, B, Select, Start, Up, Down, Left, Right
        self.state_buffer = 0;
        for (i, &pressed) in self.buttons.iter().enumerate() {
            if pressed {
                self.state_buffer |= 1 << i;
            }
        }
    }

    /// The strobe line's edge, in Mesen's shape (`StrobeProcessWrite`): the
    /// register is loaded when the line goes *high → low*, and while it is held
    /// high the read reloads it (`StrobeProcessRead`) — so a controller read
    /// with the strobe up reports the buttons as they are at that moment.
    fn write_strobe(&mut self, strobe: bool) {
        let was_strobed = self.strobe;
        self.strobe = strobe;
        if was_strobed && !self.strobe {
            self.refresh_state_buffer();
        }
    }

    /// One bit out of the register, with the next one shifted in behind it.
    ///
    /// The bit that shifts in is a `1`, so the register reads as eight ones
    /// after the buttons have been clocked out.
    fn read(&mut self) -> u8 {
        if self.strobe {
            self.refresh_state_buffer();
        }
        let bit = self.state_buffer & 1;
        self.state_buffer = (self.state_buffer >> 1) | 0x80;
        bit
    }
}

/// ROM header parser for iNES format
struct RomHeader {
    prg_rom_size: usize,
    chr_rom_size: usize,
    mapper_number: u16,
    /// NES 2.0's submapper, or 0 for a plain iNES header.
    ///
    /// It picks which board *within* a mapper a cartridge is, which is what the
    /// discrete boards' bus conflicts and MMC6's protection depend on. A plain
    /// iNES file has no such field, so this is only non-zero when the header is
    /// NES 2.0.
    submapper: u8,
    mirroring: Mirroring,
    region: Region,
    has_battery: bool,
}

impl RomHeader {
    fn parse(data: &[u8]) -> core::result::Result<(Self, usize), NesError> {
        if data.len() < 16 {
            return Err(NesError::InvalidRomFormat(
                "ROM too small for header".to_string(),
            ));
        }

        // Check iNES magic number
        if &data[0..4] != b"NES\x1a" {
            return Err(NesError::InvalidRomFormat(
                "Invalid iNES magic number".to_string(),
            ));
        }

        let flags6 = data[6];
        let flags7 = data[7];
        let flags8 = data[8];

        // Byte 7's bits 2-3 name the format: 0b10 is NES 2.0, 0b00 plain iNES,
        // and anything else is the archaic iNES whose mapper number lives in
        // byte 6's high nibble alone (Mesen's `GetRomHeaderVersion`). Nothing
        // else about the layout changes for the fields read here, and neither
        // an archaic nor a plain iNES file's byte 8 is part of the header at
        // all — reading a submapper out of it would be reading whatever the
        // unused byte holds.
        let nes2 = (flags7 & 0x0C) == 0x08;
        let archaic_ines = flags7 & 0x0C != 0x08 && flags7 & 0x0C != 0x00;

        // Byte 9's two nibbles carry the high bits of the PRG and CHR sizes in
        // NES 2.0; in iNES that byte is the TV-system flag and must not be read
        // as sizes.
        let (prg_banks, chr_banks) = if nes2 {
            if data[9] & 0x0F == 0x0F || data[9] & 0xF0 == 0xF0 {
                // The exponent-multiplier encoding: a 6-bit exponent and a
                // 2-bit multiplier. No cartridge here uses it, and a wrong size
                // would read ROM data out of the header's own bytes, so it is
                // refused rather than guessed at.
                return Err(NesError::InvalidRomFormat(
                    "NES 2.0 exponent-multiplier ROM sizes are not supported".to_string(),
                ));
            }
            (
                data[4] as usize | (((data[9] & 0x0F) as usize) << 8),
                data[5] as usize | (((data[9] >> 4) as usize) << 8),
            )
        } else {
            (data[4] as usize, data[5] as usize)
        };

        let prg_rom_size = prg_banks * 16 * 1024; // 16KB units
        let chr_rom_size = chr_banks * 8 * 1024; // 8KB units

        // Every cartridge carries at least one PRG bank, and mappers address
        // PRG with `% prg_rom.len()`. A header claiming zero banks slips past
        // the size check below and divides by zero the moment the reset vector
        // is read, so reject it here rather than let a crafted header panic.
        if prg_banks == 0 {
            return Err(NesError::InvalidRomFormat(
                "ROM declares no PRG-ROM".to_string(),
            ));
        }

        let mirroring = if flags6 & 0x08 != 0 {
            Mirroring::FourScreen
        } else if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let has_trainer = flags6 & 0x04 != 0;
        let has_battery = flags6 & 0x02 != 0;

        // The mapper number is three nibbles, and their places differ by
        // format. Mesen: `(Byte8 & 0x0F) << 8 | (Byte7 & 0xF0) | (Byte6 >> 4)`
        // for NES 2.0, `(Byte7 & 0xF0) | (Byte6 >> 4)` for iNES, and
        // `Byte6 >> 4` for the archaic one. NES 2.0's fourth nibble is the
        // submapper — byte 8's *high* half, not its low one, which is the
        // mapper's eighth bit and the reason the two are easy to swap.
        let mapper_number = if archaic_ines {
            ((flags6 & 0xF0) as u16) >> 4
        } else {
            ((flags7 & 0xF0) as u16)
                | ((flags6 & 0xF0) as u16 >> 4)
                | if nes2 {
                    ((flags8 & 0x0F) as u16) << 8
                } else {
                    0
                }
        };
        let submapper = if nes2 { flags8 >> 4 } else { 0 };

        // TV system: NES 2.0 states it outright in byte 12's low two bits
        // (0 = NTSC, 1 = PAL, 2 = both, 3 = Dendy); otherwise byte 9 bit 0 is
        // the classic PAL flag and byte 10 bits 0-1 encode it more reliably
        // (0b10 = PAL).
        let flags9 = data[9];
        let flags10 = data[10];
        let region = if nes2 {
            match data[12] & 0x03 {
                1 => Region::Pal,
                // 2 means "the cartridge runs on both", so the header's own
                // flags decide. Dendy is a PAL clock with NTSC-shaped timing,
                // which this core has no model for: it runs as NTSC.
                _ => {
                    if flags9 & 0x01 != 0 || (flags10 & 0x03) == 0x02 {
                        Region::Pal
                    } else {
                        Region::Ntsc
                    }
                }
            }
        } else if flags9 & 0x01 != 0 || (flags10 & 0x03) == 0x02 {
            Region::Pal
        } else {
            Region::Ntsc
        };

        let header = Self {
            prg_rom_size,
            chr_rom_size,
            mapper_number,
            submapper,
            mirroring,
            region,
            has_battery,
        };

        let header_size = 16;
        let trainer_size = if has_trainer { 512 } else { 0 };
        let data_offset = header_size + trainer_size;

        Ok((header, data_offset))
    }
}

/// The NES system bus implementation for WASM
///
/// This connects all the NES components (CPU, PPU, APU, Mapper, Controllers)
struct NesBus {
    /// CPU internal RAM (2KB mirrored to $0000-$1FFF)
    cpu_ram: [u8; 0x800],
    /// PPU instance
    ppu: Ppu,
    /// APU instance
    apu: Apu,
    /// Cartridge mapper
    mapper: Option<Box<dyn Mapper>>,
    /// Frames the machine has finished since it was created.
    ///
    /// Deliberately *not* the PPU's frame counter: that one restarts at every
    /// reset, because the odd frame's skipped dot is read off its parity, while
    /// this one is what a host counts and keeps counting across a cartridge
    /// change (`Nes::frame_number`).
    frames_completed: u64,
    /// Whether rendering was on at the end of the previous dot.
    ///
    /// Mesen keeps this lag deliberately (`UpdateState` runs after a dot's
    /// work): the odd frame's skipped dot is decided from the state as of the
    /// dot before, so a `$2001` write landing on the decision's own dot is one
    /// dot too late. `10-even_odd_timing` measures the boundary.
    rendering_enabled_delayed: bool,
    /// Controller states (player 1 and 2)
    controllers: [ControllerState; 2],
    /// A `$4016` write the chip has not carried to the controllers yet.
    pending_controller_write: Option<PendingControllerWrite>,
    /// The controller port read that just happened.
    controller_prev_read: Option<ControllerPortRead>,
    /// IRQ pending flag (from APU or mapper)
    irq_pending: bool,
    /// Interleaved stereo audio waiting for the host to pull it
    audio_buffer: VecDeque<f32>,
    /// Fractional accumulator that paces the APU resampler
    /// The band-limited resampler that takes the APU's CPU-rate output down to
    /// the host's sample rate, and the level it was last handed.
    resampler: crate::audio::BandLimiter,
    audio_level: f64,
    /// Rate the APU output is resampled to, for the host's audio device
    audio_sample_rate: u32,
    /// Cap on `audio_buffer`, derived from the sample rate
    audio_capacity: usize,
    /// CPU cycles accumulated for audio generation
    cpu_cycles: u64,
    /// The page an OAM DMA transfer will copy from, set by a `$4014` write and
    /// spent at the CPU's next access.
    ///
    /// Mesen's `ProcessPendingDma` flags, one for one: a `$4014` write or a
    /// DMC request only sets them, and the transfer itself runs at the CPU's
    /// next read, in cycles that are the machine's — the PPU, the APU and the
    /// interrupt lines all advance through them.
    ///
    /// `need_halt` is the cycle the transfer begins with, `need_dummy_read` the
    /// one the DMC's fetch spends waiting, and `abort_dmc_dma` marks a transfer
    /// the channel cancelled mid-flight.
    need_halt: bool,
    need_dummy_read: bool,
    dmc_dma_running: bool,
    abort_dmc_dma: bool,
    sprite_dma_transfer: bool,
    /// The page a sprite transfer copies from (`$4014`'s value).
    sprite_dma_offset: u8,
    /// State the transfer's loop carries between cycles: the address the CPU
    /// was about to use, what the DMA read last cycle, and where in the sprite
    /// transfer the loop is.
    dma_read_address: u16,
    dma_prev_read_address: u16,
    dma_read_value: u8,
    dma_sprite_read_addr: u8,
    dma_sprite_counter: u16,
    /// Whether the transfer's glitched internal-register reads apply, i.e.
    /// whether the CPU was halted inside `$4000-$401F`, and whether it was
    /// halted on an opcode fetch (which PAL's rule reads).
    dma_internal_reg_reads: bool,
    dma_is_opcode_fetch: bool,
    /// The last [`NesBus::ppu_bus_trace_capacity`] dots, oldest first.
    ///
    /// `cycle_trace`'s A12 tool samples once per instruction, which is too
    /// coarse for a scanline counter's nine-dot filter; this is the same view
    /// the chip has. It is host-side diagnostic state, like the audio buffer:
    /// it stays out of the save state, and its contents are re-derived by
    /// running the machine.
    ppu_bus_trace: VecDeque<PpuBusSample>,
    /// How many dots of history [`NesBus::ppu_bus_trace`] keeps.
    ppu_bus_trace_capacity: usize,
    /// The last [`NesBus::ppu_write_log_capacity`] writes to a PPU register,
    /// oldest first. Off (`0`) until a caller turns it on.
    ppu_write_log: VecDeque<PpuWriteSample>,
    /// How many register writes [`NesBus::ppu_write_log`] keeps, and whether it
    /// is recording at all.
    ppu_write_log_capacity: usize,
    /// Dots a previous instruction already produced that the caller has not
    /// stepped through yet; [`NesBus::step_dot`] spends one per call.
    pending_dots: u32,
    /// The address the PPU is driving for the cartridge: Mesen's
    /// `_ppuBusAddress`, set by `SetBusAddress` on every access and *held*
    /// between them.
    ///
    /// Holding it is what the cartridges see: during vertical blanking the
    /// pipeline is idle, so A12 stays wherever the last fetch left it (the
    /// nametable, i.e. low) instead of following the VRAM address register,
    /// which is what makes the long low run before a frame's first fetch —
    /// and therefore the one rise per scanline that a scanline counter counts.
    /// A CPU access through `$2007` drives it too, which is how a game can
    /// move A12 from the CPU side.
    bus_address: u16,
    /// Set when pre-render dot 339 found the conditions for an odd frame's
    /// short frame, so dot 340 is the one that gets skipped.
    ///
    /// The chip decides a dot early - Mesen jumps its counter to 340 while it is
    /// processing 339 - so a write that reaches `$2001` between the two dots is
    /// too late to change how long the frame is. blargg's `10-even_odd_timing`
    /// measures exactly that, and it is the reason this cannot be decided on
    /// 340 itself.
    skip_odd_frame_dot: bool,
    frame_buffer: FrameBuffer,
    /// The per-dot renderer: fetches, shift registers, sprites and the picture.
    pipeline: PixelPipeline,
    /// Sprite evaluator for the current scanline
    sprite_evaluator: SpriteEvaluator,
    /// Nametable mirroring mode from the cartridge header
    mirroring: Mirroring,
    /// TV system (NTSC or PAL) from the cartridge header
    region: Region,
    /// Host-requested TV system, overriding whatever the header claims
    region_override: Option<Region>,
    /// Whether the host wants audio samples buffered for playback
    enable_audio: bool,
    /// Last value driven onto the data bus, returned by unmapped reads
    open_bus: u8,
    /// Whether the cartridge declares battery-backed work RAM
    has_battery: bool,
    /// The cartridge's NES 2.0 submapper, or 0 for a plain iNES header.
    ///
    /// Kept so a host can see which board a NES 2.0 file says it is; nothing
    /// dispatches on it yet (see the note in `RomHeader`).
    submapper: u8,
    frame_ready: bool,
}

crate::state::state_fields!(NesBus {
    cpu_ram,
    ppu,
    apu,
    mapper,
    controllers,
    pending_controller_write,
    controller_prev_read,
    irq_pending,
    resampler,
    audio_level,
    cpu_cycles,
    need_halt,
    need_dummy_read,
    dmc_dma_running,
    abort_dmc_dma,
    sprite_dma_transfer,
    sprite_dma_offset,
    dma_read_address,
    dma_prev_read_address,
    dma_read_value,
    dma_sprite_read_addr,
    dma_sprite_counter,
    dma_internal_reg_reads,
    dma_is_opcode_fetch,
    skip_odd_frame_dot,
    bus_address,
    pipeline,
    sprite_evaluator,
    mirroring,
    region,
    open_bus,
    has_battery,
    frame_ready,
});

impl NesBus {
    fn new() -> Self {
        let ppu = Ppu::new();

        Self {
            // Real NES RAM powers up with undefined contents, so emulators pick
            // a fixed pattern. 0x00 is the usual convention: this cartridge's
            // boot code reads an uninitialised byte as the starting world
            // number, and 0x00 selects World 1-1.
            cpu_ram: [0; 0x800],
            ppu,
            apu: Apu::new(),
            mapper: None,
            controllers: [ControllerState::new(), ControllerState::new()],
            pending_controller_write: None,
            controller_prev_read: None,
            irq_pending: false,
            audio_buffer: VecDeque::with_capacity(audio_capacity_floats(
                DEFAULT_SAMPLE_RATE,
                AUDIO_BUFFER_MS,
            )),
            // The APU's own clock is not fixed until a cartridge's region is
            // known, so the resampler is built for NTSC here and re-rated when
            // the region or the host's rate changes.
            resampler: crate::audio::BandLimiter::new(
                Region::Ntsc.apu_clock_rate(),
                DEFAULT_SAMPLE_RATE as f64,
            ),
            audio_level: 0.0,
            audio_sample_rate: DEFAULT_SAMPLE_RATE,
            audio_capacity: audio_capacity_floats(DEFAULT_SAMPLE_RATE, AUDIO_BUFFER_MS),
            cpu_cycles: 0,
            need_halt: false,
            need_dummy_read: false,
            dmc_dma_running: false,
            abort_dmc_dma: false,
            sprite_dma_transfer: false,
            sprite_dma_offset: 0,
            dma_read_address: 0,
            dma_prev_read_address: 0,
            dma_read_value: 0,
            dma_sprite_read_addr: 0,
            dma_sprite_counter: 0,
            dma_internal_reg_reads: false,
            dma_is_opcode_fetch: false,
            skip_odd_frame_dot: false,
            bus_address: 0,
            ppu_bus_trace: VecDeque::with_capacity(DEFAULT_PPU_BUS_TRACE_DOTS),
            ppu_bus_trace_capacity: DEFAULT_PPU_BUS_TRACE_DOTS,
            ppu_write_log: VecDeque::new(),
            ppu_write_log_capacity: 0,
            pending_dots: 0,
            frame_buffer: FrameBuffer::new(),
            pipeline: PixelPipeline::new(),
            sprite_evaluator: SpriteEvaluator::new(),
            mirroring: Mirroring::Horizontal,
            region: Region::Ntsc,
            region_override: None,
            enable_audio: true,
            open_bus: 0,
            has_battery: false,
            submapper: 0,
            frames_completed: 0,
            rendering_enabled_delayed: false,
            frame_ready: false,
        }
    }

    /// Apply the host's configuration: TV-system override and audio settings.
    ///
    /// These are host settings rather than machine state, so they stay out of
    /// the save-state encoding and survive a [`NesBus::load_rom`].
    fn apply_config(&mut self, config: &Config) {
        self.region_override = config.region_override;
        self.enable_audio = config.enable_audio;
        self.set_sample_rate(config.sample_rate);
    }

    /// Retarget the resampler, dropping audio that was queued at the old rate.
    ///
    /// Zero keeps the current rate, so an unset config value cannot silence the
    /// machine.
    fn set_sample_rate(&mut self, sample_rate: u32) {
        if sample_rate == 0 || sample_rate == self.audio_sample_rate {
            return;
        }
        self.audio_sample_rate = sample_rate;
        self.audio_capacity = audio_capacity_floats(sample_rate, AUDIO_BUFFER_MS);
        self.audio_buffer.clear();
        self.rate_the_resampler();
    }

    /// Load a ROM into the system
    fn load_rom(&mut self, data: &[u8]) -> core::result::Result<(), NesError> {
        let (header, data_offset) = RomHeader::parse(data)?;

        // Extract PRG-ROM
        let prg_start = data_offset;
        let prg_end = prg_start + header.prg_rom_size;
        if data.len() < prg_end {
            return Err(NesError::InvalidRomFormat(
                "ROM too small for PRG-ROM".to_string(),
            ));
        }
        let prg_rom = data[prg_start..prg_end].to_vec();

        // Extract CHR-ROM
        let chr_start = prg_end;
        let chr_end = chr_start + header.chr_rom_size;
        let chr_rom = if data.len() >= chr_end {
            data[chr_start..chr_end].to_vec()
        } else {
            vec![] // CHR-RAM will be created by mapper
        };

        // Create mapper
        let mapper = create_mapper(header.mapper_number, prg_rom, chr_rom)?;
        self.mapper = Some(mapper);

        // Apply the cartridge's nametable mirroring to both the renderer
        // (which reads nametables) and the bus (which mirrors PPUDATA writes).
        self.mirroring = header.mirroring;
        self.pipeline.set_mirroring(header.mirroring);
        self.region = self.region_override.unwrap_or(header.region);
        self.pipeline.set_region(self.region);
        self.apu.set_region(self.region);
        self.rate_the_resampler();
        self.has_battery = header.has_battery;
        self.submapper = header.submapper;

        Ok(())
    }

    /// Whether the loaded cartridge has battery-backed work RAM.
    fn has_battery(&self) -> bool {
        self.has_battery
    }

    /// Snapshot the cartridge's battery RAM.
    fn save_ram(&self) -> Option<Vec<u8>> {
        if !self.has_battery {
            return None;
        }
        self.mapper
            .as_ref()
            .and_then(|m| m.save_ram())
            .map(|r| r.to_vec())
    }

    /// Restore a snapshot produced by [`NesBus::save_ram`].
    fn load_save_ram(&mut self, data: &[u8]) {
        if let Some(ref mut mapper) = self.mapper {
            mapper.load_save_ram(data);
        }
    }

    /// Power-on the system: everything, memories included, back to the state a
    /// cartridge finds when the console is switched on.
    fn power_on(&mut self, cpu: &mut Cpu) {
        self.reset_with(cpu, false);
    }

    /// Press the reset button.
    fn reset(&mut self, cpu: &mut Cpu) {
        self.reset_with(cpu, true);
    }

    /// The two resets, which differ in three places that a cartridge can see:
    /// the CPU walks its stack pointer down (soft only), the memories are
    /// cleared (power-on only), and the CPU takes its eight dummy cycles either
    /// way.
    fn reset_with(&mut self, cpu: &mut Cpu, soft: bool) {
        // The cycle counter restarts with the CPU, so a reset leaves the same
        // clock parity behind every time — which the DMA and DMC phase checks
        // read (Mesen's `CycleCount = -1` at the top of its `Reset`).
        self.cpu_cycles = 0;

        if soft {
            self.ppu.soft_reset();
            self.apu.soft_reset();
        } else {
            self.ppu.reset();
            self.apu.reset();
        }
        self.pipeline.reset_shifters();
        self.pipeline.clear_bg_plane();
        // A held bus address would outlive the reset the cartridge's own
        // accesses are about to set.
        let _ = self.pipeline.take_bus_access();
        self.bus_address = 0;

        if soft {
            cpu.soft_reset(self);
        } else {
            cpu.reset(self);
        }

        // "The CPU takes 8 cycles before it starts executing the ROM's code
        // after a reset/power up" — Mesen's `Reset` runs exactly these eight
        // dummy cycles, which is also how far into a frame the PPU and the APU
        // are when the cartridge's first instruction runs.
        for _ in 0..RESET_WARM_UP_CYCLES {
            self.advance_one_cpu_cycle();
        }
    }

    /// Run one frame of emulation
    ///
    /// Returns true if a frame was completed
    fn run_frame(&mut self, cpu: &mut Cpu) -> bool {
        self.frame_ready = false;

        // NTSC: 262 scanlines per frame, 341 PPU cycles per scanline
        // Each CPU cycle = 3 PPU cycles
        // Total PPU cycles per frame = 262 * 341 = 89,342
        // Total CPU cycles per frame = 89,342 / 3 ≈ 29,780

        // One frame of CPU cycles for the cartridge's TV system. A frame is not
        // a whole number of CPU cycles - 89,342 dots is 29,780.67 of them on
        // NTSC - so the bound admits the cycle the frame boundary falls in;
        // `frame_ready` is what actually ends the loop.
        let target_cpu_cycles = self.region.cpu_cycles_per_frame() as u64;
        let start_cycles = self.cpu_cycles;

        while self.cpu_cycles - start_cycles <= target_cpu_cycles && !self.frame_ready {
            self.step_instruction(cpu);
        }

        self.frame_ready
    }

    /// Run at most `cycles` CPU cycles, stopping early when a frame ends.
    ///
    /// The `frame_ready` flag is cleared up front, exactly as
    /// [`NesBus::run_frame`] does, so a caller can tell whether a frame boundary
    /// fell inside this slice.
    ///
    /// Returns the number of CPU cycles actually run, which is less than the
    /// requested amount when a frame ended first.
    fn run_cycles(&mut self, cpu: &mut Cpu, cycles: u32) -> u32 {
        self.frame_ready = false;

        let start_cycles = self.cpu_cycles;
        let target_cpu_cycles = cycles as u64;
        while self.cpu_cycles - start_cycles < target_cpu_cycles && !self.frame_ready {
            self.step_instruction(cpu);
        }

        (self.cpu_cycles - start_cycles) as u32
    }

    /// Execute a single CPU instruction and advance the rest of the machine by
    /// the cycles it consumed.
    ///
    /// This is the smallest unit of emulation; `frame_ready` is left alone so a
    /// debugger can single-step across a frame boundary and still see it.
    fn step_instruction(&mut self, cpu: &mut Cpu) -> u8 {
        // The CPU drives its own cycles now, exactly as Mesen's does: the
        // opcode fetch is a cycle, the addressing's operand fetches are
        // cycles, every access the handler makes is a cycle, and the discarded
        // reads in between are cycles too. Nothing here has to place them
        // afterwards — a read or a write reaches `NesBus::read_cycle` /
        // `NesBus::write_cycle`, which advance the PPU and APU around it.
        let before = self.cpu_cycles;
        // The opcode fetch is the CPU's own cycle, and the one place a PAL
        // console's DMA can start; `Cpu::fetch_opcode` does both and leaves the
        // end-of-cycle sampling where the instruction expects it.
        let opcode = cpu.fetch_opcode(self);
        let cycles = cpu.execute_instruction(self, opcode);

        // Mappers whose IRQ counter runs at the CPU clock rate.
        if let Some(ref mut mapper) = self.mapper {
            mapper.on_cpu_cycles(cycles);
        }

        cpu.set_cycle_count(self.cpu_cycles);
        (self.cpu_cycles - before) as u8
    }

    /// Take the parked register write that belongs on the instruction's
    /// Get the current frame buffer
    fn frame_buffer(&self) -> &FrameBuffer {
        &self.frame_buffer
    }

    /// Interleaved stereo frames waiting to be pulled, not yet filled.
    fn buffered_audio_frames(&self) -> usize {
        self.audio_buffer.len() / 2
    }

    /// Copy as much buffered audio as fits into `out`, interleaved left/right,
    /// and return how many floats were written (always a whole number of frames).
    fn fill_audio(&mut self, out: &mut [f32]) -> usize {
        // Never split a stereo frame, so a caller with an odd-sized buffer still
        // gets whole frames back.
        let limit = out.len() & !1;
        let mut written = 0;
        while written < limit {
            match self.audio_buffer.pop_front() {
                Some(sample) => {
                    out[written] = sample;
                    written += 1;
                }
                None => break,
            }
        }
        written
    }

    /// Drop every buffered audio sample.
    fn clear_audio_samples(&mut self) {
        self.audio_buffer.clear();
    }

    /// Set a button state for a player
    fn set_button(&mut self, player: u8, button: u8, pressed: bool) {
        let player_idx = match player {
            1 => 0,
            2 => 1,
            _ => return,
        };
        if player_idx < self.controllers.len() {
            self.controllers[player_idx].set_button(button, pressed);
        }
    }

    /// Read from PPU register
    fn read_ppu_register(&mut self, addr: u16) -> u8 {
        // The register state machine lives in `Ppu`; the bus only supplies the
        // cartridge CHR, nametable mirroring and data bus that the PPU cannot
        // own. Addresses mirror every 8 bytes across $2000-$3FFF.
        let drawing_line =
            self.ppu.scanline < 240 || self.ppu.scanline == self.region.last_scanline();
        let memory = PpuMemory::System {
            chr: self.mapper.as_deref_mut(),
            mirroring: self.mirroring,
            open_bus: self.open_bus,
            drawing_line,
        };
        self.ppu.read_register_with(addr, memory)
    }

    /// Write to PPU register
    fn write_ppu_register(&mut self, addr: u16, value: u8) {
        // Logged before the register logic runs: the dot the write landed on is
        // the chip's position *now*, and the status byte is what a `$2002` read
        // on that dot would have returned.
        if self.ppu_write_log_capacity != 0 {
            if self.ppu_write_log.len() == self.ppu_write_log_capacity {
                self.ppu_write_log.pop_front();
            }
            self.ppu_write_log.push_back(PpuWriteSample {
                scanline: self.ppu.scanline,
                dot: self.ppu.cycle,
                addr,
                value,
                status: self.ppu.status.bits(),
            });
        }
        let drawing_line =
            self.ppu.scanline < 240 || self.ppu.scanline == self.region.last_scanline();
        let memory = PpuMemory::System {
            chr: self.mapper.as_deref_mut(),
            mirroring: self.mirroring,
            open_bus: self.open_bus,
            drawing_line,
        };
        self.ppu.write_register_with(addr, value, memory);
    }

    /// A controller port read.
    ///
    /// The device drives bit 0 and nothing else, so the rest of the byte is the
    /// console's open bus — and the *top three* bits are the ones the port does
    /// not drive on a NES-001 (`GetOpenBusMask`), which is why a read usually
    /// answers `$40` or `$41` (the address byte's high half left on the bus).
    ///
    /// Reading the same port again inside one CPU cycle does not clock the
    /// shift register a second time: it answers with the bit the first read
    /// gave, because the device's own previous read is still current
    /// (`NesControlManager::ReadDevice`). A DMC DMA's halt cycle is what makes
    /// this visible — the CPU reads the port, the DMA re-reads it a cycle
    /// later, and a game can lose a bit.
    fn read_controller_port(&mut self, port: usize) -> u8 {
        let addr = 0x4016 + port as u16;
        let cycle = self.cpu_cycles;
        let bit = match self.controller_prev_read {
            // The previous read's answer still stands if it was this port and
            // it happened in this cycle or the one before.
            Some(prev) if prev.addr == addr && prev.cycle + 1 >= cycle => prev.bit,
            _ => self.controllers[port].read(),
        };
        self.controller_prev_read = Some(ControllerPortRead { addr, cycle, bit });

        (self.open_bus & CONTROLLER_OPEN_BUS_MASK) | bit
    }

    /// Read from APU/register
    fn read_apu_register(&mut self, addr: u16) -> u8 {
        if addr == 0x4016 || addr == 0x4017 {
            return self.read_controller_port(usize::from(addr == 0x4017));
        }
        match addr {
            0x4015 => {
                // APU status register
                self.apu.read_register(addr)
            }
            _ => {
                // Write-only or expansion-area registers read back as open bus.
                self.open_bus
            }
        }
    }

    /// Write to APU/register
    fn write_apu_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x4014 => {
                // OAMDMA: the write asks for a 256-byte copy into OAM. The copy
                // itself runs at the CPU's next *read*, in cycles the chip
                // steals (`NesBus::start_pending_dma`).
                self.run_dma_transfer(value);
            }
            0x4016 => {
                // The strobe and OUT pins are updated at the *next* PUT cycle,
                // not on the write's own cycle: Mesen's `NesControlManager::
                // WriteRam` parks the value for one or two CPU cycles
                // (`_writePending = (masterClock & 1) ? 1 : 2`) and applies it
                // from `ProcessCpuClock`. A game that writes the strobe and
                // reads the port in the very next cycle sees the difference.
                self.pending_controller_write = Some(PendingControllerWrite {
                    value,
                    cycles_left: if self.in_an_odd_cpu_cycle() { 1 } else { 2 },
                });
            }
            _ => {
                // The APU's clock phase matters to a `$4017` restart and to
                // the DMC's enable/disable delays, and the cycle the write is
                // in is how the once-per-cycle APU here sees it.
                // The cycle the write is in decides the APU's "between cycles"
                // flavour: `4-irq_and_dma` is what pinned this side of the
                // convention, and `4-jitter`, `5-len_timing` and
                // `6-irq_flag_timing` are green with it.
                let odd_apu_cycle = self.in_an_odd_cpu_cycle();
                self.apu.write_register_at(addr, value, odd_apu_cycle);
            }
        }
    }

    /// Point the resampler at the rates the machine is running at now.
    ///
    /// The APU is ticked once per CPU cycle, so its clock is the region's
    /// [`Region::apu_clock_rate`] — not the cartridge's nominal CPU rate, which
    /// PAL boards do not match under the 3.2 dots per cycle the machine runs.
    fn rate_the_resampler(&mut self) {
        self.resampler
            .set_rates(self.region.apu_clock_rate(), self.audio_sample_rate as f64);
    }

    /// Advance the APU by one CPU cycle and, when the resampler is due,
    /// append an interleaved stereo sample to the output buffer.
    fn tick_apu_cycle(&mut self) {
        self.apu.tick();

        // The DMC asks for a byte and the chip answers with `StartDmcTransfer`;
        // disabling the channel asks it to stop (`StopDmcTransfer`). Both are
        // latched, and each is answered by exactly one transfer — the request
        // is *taken*, not read, so the cycles the transfer itself spends cannot
        // arm a second one.
        if self.apu.take_dmc_start_request() {
            self.start_dmc_transfer();
        }
        if self.apu.take_dmc_stop_request() {
            self.stop_dmc_transfer();
        }

        // Resample the CPU-rate mixer output down to the host's device rate.
        // The APU keeps ticking when audio capture is off because its frame
        // IRQ and the DMC DMA requests above change CPU timing; only the
        // samples handed to the host are skipped.
        if !self.enable_audio {
            return;
        }

        // The cartridge's own sound hardware (VRC6, MMC5, ...) mixes on top of
        // the 2A03 output. A change in the level is a step for the resampler;
        // holding still costs nothing.
        let expansion = self.mapper.as_ref().map_or(0.0, |m| m.audio_output());
        let level = f64::from(self.apu.output() + expansion);
        self.resampler.set_level(level, &mut self.audio_level);
        self.resampler.advance();

        // Mono APU output, duplicated for the left and right channels.
        while let Some(sample) = self.resampler.read() {
            self.audio_buffer.push_back(sample);
            self.audio_buffer.push_back(sample);
        }
        self.trim_audio_buffer();
    }

    /// Drop the oldest samples once the host has fallen behind the buffer cap.
    ///
    /// Discarding is `pop_front` per sample, which is O(1), so a stalled host
    /// does not pay a buffer shift for every new sample it produces.
    fn trim_audio_buffer(&mut self) {
        let excess = self.audio_buffer.len().saturating_sub(self.audio_capacity);
        for _ in 0..excess {
            self.audio_buffer.pop_front();
        }
    }

    /// Advance the PPU by the single cycle that was just counted.
    ///
    /// The pixel pipeline runs off the PPU's own dot counter and the *real*
    /// scroll registers, exactly as the chip does: a tile is fetched every
    /// eight dots, the fetched tile is shifted out one pixel per dot, and a
    /// `$2001`/`$2005`/`$2006` write therefore takes effect from the dot it
    /// lands on. What is left here is the frame's own bookkeeping — the picture
    /// for a line with rendering off, the sprite evaluator, the VBlank flag and
    /// the hit latch.
    fn ppu_step(&mut self) {
        let scanline = self.ppu.scanline;
        let cycle = self.ppu.cycle;
        let pre_render = self.region.last_scanline();

        self.step_pipeline(pre_render);

        if scanline == pre_render {
            // Pre-render scanline: the flags the last frame left go, the
            // background shift registers start the new frame empty, and the
            // sprites for line 0 are evaluated and prefetched here.
            if cycle == 1 {
                self.ppu.status.remove(PpuStatus::VBLANK);
                self.ppu.status.remove(PpuStatus::SPRITE_OVERFLOW);
                self.ppu.status.remove(PpuStatus::SPRITE_0_HIT);
                self.pipeline.reset_shifters();
            }
            self.step_sprite_pipeline(0, cycle);
        } else if scanline < 240 {
            // The sprite pipeline is driven at Mesen's dots: the evaluation
            // that fills secondary OAM runs at dot 65 and the pattern fetches
            // at 257-320, both for the *next* line, which is the line they will
            // be drawn on.
            self.step_sprite_pipeline(scanline + 1, cycle);
        } else if scanline == 241 {
            // Start of VBlank
            if cycle == 1 {
                // A $2002 read on this dot takes the frame away from the flag:
                // neither the flag nor the NMI appears.
                if !self.ppu.take_vblank_suppression() {
                    self.ppu.status.insert(PpuStatus::VBLANK);
                }
            }
        }
    }

    /// The sprite half of the pipeline for `sprite_line`, at this dot.
    fn step_sprite_pipeline(&mut self, sprite_line: u16, cycle: u16) {
        if cycle == 65 {
            let overflow = self.sprite_evaluator.evaluate(
                &self.ppu.oam,
                sprite_line,
                self.ppu.ctrl,
                self.ppu.oam_addr,
            );
            if overflow {
                self.ppu.status.insert(PpuStatus::SPRITE_OVERFLOW);
            }
        }
        if cycle == 257 {
            let slots = self.sprite_slots_from_secondary_oam();
            self.pipeline.set_sprite_slots(slots);
        }
    }

    /// The line's secondary-OAM slots, in the form the sprite pipeline wants.
    fn sprite_slots_from_secondary_oam(&self) -> [SpriteSlot; 8] {
        let mut slots = [SpriteSlot::default(); 8];
        for (index, slot) in slots.iter_mut().enumerate() {
            if let Some(sprite) = self.sprite_evaluator.secondary_oam.get_sprite(index) {
                *slot = SpriteSlot {
                    y: sprite.y,
                    tile: sprite.tile,
                    attributes: sprite.attributes,
                    x: sprite.x,
                    is_sprite_zero: self.sprite_evaluator.secondary_oam.has_sprite_0()
                        && index == 0,
                    occupied: true,
                };
            }
        }
        slots
    }

    /// Run the pixel pipeline for the dot the PPU is standing on.
    ///
    /// Pattern data and nametables go through the cartridge, so a mid-frame CHR
    /// bank switch or an MMC5 nametable window takes effect on the fetch that
    /// follows it.
    fn step_pipeline(&mut self, pre_render: u16) {
        match self.mapper.as_deref_mut() {
            Some(mapper) => {
                let mut chr = MapperChr(mapper);
                self.pipeline.step(&mut self.ppu, &mut chr, pre_render);
            }
            None => {
                let mut chr = VramChr;
                self.pipeline.step(&mut self.ppu, &mut chr, pre_render);
            }
        }
    }
}

impl Bus for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        let value = match addr {
            // CPU internal RAM ($0000-$07FF, mirrored to $1FFF)
            0x0000..=0x1FFF => self.cpu_ram[(addr & 0x07FF) as usize],
            // PPU registers ($2000-$3FFF, mirrored every 8 bytes)
            0x2000..=0x3FFF => self.read_ppu_register(addr),
            // APU and I/O registers ($4000-$401F)
            0x4000..=0x401F => self.read_apu_register(addr),
            // Cartridge space ($4020-$FFFF)
            _ => {
                if let Some(ref mut mapper) = self.mapper {
                    mapper.read(addr)
                } else {
                    self.open_bus
                }
            }
        };

        // Every read drives the data bus; the last value lingers for unmapped
        // addresses.
        self.open_bus = value;
        value
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.write_now(addr, value);
    }

    fn on_cpu_cycle(&mut self, cycles: u8) {
        // The trait's "the CPU consumed these cycles" hook. The core's own
        // execution path ticks per cycle instead (`advance_one_cpu_cycle`), so
        // that interrupt polls, bus accesses and the two chips stay in step.
        self.advance_cycles(cycles as u16);
    }

    fn read_cycle(&mut self, addr: u16) -> u8 {
        self.begin_cpu_cycle();
        let value = self.read(addr);
        self.end_cpu_cycle();
        value
    }

    fn write_cycle(&mut self, addr: u16, value: u8) {
        self.begin_cpu_cycle();
        self.write_now(addr, value);
        self.end_cpu_cycle();
    }

    fn start_pending_dma(&mut self, read_address: u16, is_opcode_fetch: bool) -> bool {
        NesBus::start_pending_dma(self, read_address, is_opcode_fetch)
    }

    fn dma_running(&self) -> bool {
        self.dmc_dma_running || self.sprite_dma_transfer
    }

    fn run_dma_cycle(&mut self) {
        NesBus::run_dma_cycle(self);
    }

    fn cycle_count(&self) -> u64 {
        self.cpu_cycles
    }

    fn nmi_line(&self) -> bool {
        NesBus::nmi_line(self)
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
            || self.apu.frame_counter_irq
            || self.apu.dmc.irq_active()
            || self.mapper.as_ref().is_some_and(|m| m.irq_pending())
    }
}

impl NesBus {
    /// Apply a write to the machine.
    ///
    /// [`Bus::write`] lands register writes here on the cycle they belong to
    /// rather than the instruction's start.
    fn write_now(&mut self, addr: u16, value: u8) {
        self.open_bus = value;

        match addr {
            // CPU internal RAM
            0x0000..=0x1FFF => {
                self.cpu_ram[(addr & 0x07FF) as usize] = value;
            }
            // PPU registers
            0x2000..=0x3FFF => {
                self.write_ppu_register(addr, value);
            }
            // APU and I/O registers
            0x4000..=0x401F => {
                self.write_apu_register(addr, value);
            }
            // Cartridge space
            _ => {
                if let Some(ref mut mapper) = self.mapper {
                    // On the discrete boards the cartridge's data lines are on
                    // the same bus as the CPU's, so a register write carries
                    // whatever both of them drive: bitwise AND. The board's own
                    // byte is what it would hand back for that address.
                    let value = if mapper.has_bus_conflicts() {
                        value & mapper.read(addr)
                    } else {
                        value
                    };
                    mapper.write(addr, value);
                }
            }
        }
    }

    /// The NMI line the PPU drives.
    ///
    /// It is high while the VBlank flag is set *and* NMI is enabled, which is
    /// the level the CPU's poll samples - and the reason a `$2002` read that
    /// takes the flag away takes the frame's NMI with it.
    fn nmi_line(&self) -> bool {
        self.ppu.vblank_flag_set() && self.ppu.ctrl.contains(PpuCtrl::NMI_ENABLE)
    }

    /// Hand the PPU's accesses to the cartridge for the dot just stepped.
    ///
    /// Mesen's `SetBusAddress`, called from every `ReadVram`: the cartridge
    /// sees the address of the *last* access, held between accesses. The
    /// pipeline reports the addresses it really read, on the dot it read them,
    /// so an A12 filter clocks on real fetches instead of on a reconstruction.
    /// Dots that made no access leave the held address alone, which is what
    /// lets a CPU access through `$2007` stay on the bus until the pipeline's
    /// next fetch.
    fn feed_ppu_bus(&mut self) {
        if let Some(addr) = self.pipeline.take_bus_access() {
            self.bus_address = addr;
        }
        let addr = self.bus_address;
        self.notify_ppu_bus(addr);
    }

    /// One PPU dot's worth of the register state machines.
    ///
    /// A `$2007` read's buffer refill, a `$2007` write landing and a `$2006`
    /// write moving `v` all happen here rather than on the CPU's cycle, and each
    /// of them can put a new address in front of the cartridge — which is what
    /// a scanline counter's A12 is taken from. The state machines need the
    /// cartridge to do their memory access, so the memory source is built here.
    fn tick_ppu_register_state(&mut self) {
        let drawing_line =
            self.ppu.scanline < 240 || self.ppu.scanline == self.region.last_scanline();
        {
            let mut memory = PpuMemory::System {
                chr: self.mapper.as_deref_mut(),
                mirroring: self.mirroring,
                open_bus: self.open_bus,
                drawing_line,
            };
            self.ppu.tick_delayed_state(&mut memory);
        }
        if let Some(addr) = self.ppu.take_pending_bus_address() {
            self.bus_address = addr;
            self.notify_ppu_bus(addr);
        }
    }

    /// Tell the cartridge the PPU's address bus moved (Mesen's `SetBusAddress`
    /// calling `NotifyVramAddressChange`).
    fn notify_ppu_bus(&mut self, addr: u16) {
        let cycles = self.cpu_cycles;
        if let Some(ref mut mapper) = self.mapper {
            mapper.on_ppu_bus(addr, cycles);
        }
    }

    /// Mesen's `RunDMATransfer`: a `$4014` write only asks for the transfer.
    fn run_dma_transfer(&mut self, page: u8) {
        self.sprite_dma_transfer = true;
        self.sprite_dma_offset = page;
        self.need_halt = true;
    }

    /// Mesen's `StartDmcTransfer`: the channel has a byte to fetch.
    fn start_dmc_transfer(&mut self) {
        self.dmc_dma_running = true;
        self.need_dummy_read = true;
        self.need_halt = true;
    }

    /// Mesen's `StopDmcTransfer`.
    ///
    /// A transfer that has not started yet is cancelled outright; one that has
    /// is marked so the loop drops it at its next cycle. Only the first cycle
    /// of a transfer can be aborted.
    fn stop_dmc_transfer(&mut self) {
        if !self.dmc_dma_running {
            return;
        }
        if self.need_halt {
            self.dmc_dma_running = false;
            self.need_dummy_read = false;
            self.need_halt = false;
        } else {
            self.abort_dmc_dma = true;
        }
    }

    /// Mesen's `ProcessPendingDma`, split in two so the CPU can pump the loop
    /// one cycle at a time (`Cpu::run_pending_dma`): this part runs the halt
    /// cycle and says whether one was spent.
    fn start_pending_dma(&mut self, read_address: u16, is_opcode_fetch: bool) -> bool {
        if !self.need_halt {
            return false;
        }
        if self.region == Region::Pal && !is_opcode_fetch {
            // On PAL a transfer only starts when the CPU fetches an opcode;
            // that is also what keeps its DMA reads from deleting input bits.
            return false;
        }

        self.dma_read_address = read_address;
        self.dma_is_opcode_fetch = is_opcode_fetch;
        self.dma_prev_read_address = read_address;
        self.dma_internal_reg_reads = (read_address & 0xFFE0) == 0x4000;
        self.dma_sprite_counter = 0;
        self.dma_sprite_read_addr = 0;
        self.dma_read_value = 0;

        self.dma_halt_cycle();
        true
    }

    /// The halt cycle the transfer begins with — a read at the address the CPU
    /// was about to use — and the abort check that follows it.
    ///
    /// This is also what a transfer that starts right behind another one runs
    /// (Mesen's `NoInlineProcessPendingDma`).
    fn dma_halt_cycle(&mut self) {
        self.need_halt = false;
        self.begin_cpu_cycle();
        let _ = self.read(self.dma_read_address);
        self.end_cpu_cycle();

        if self.abort_dmc_dma {
            self.dmc_dma_running = false;
            self.abort_dmc_dma = false;
            if !self.sprite_dma_transfer {
                // Cancelled, and the sprite transfer is not about to run:
                // nothing is left to do.
                self.need_dummy_read = false;
            }
        }
    }

    /// One iteration of Mesen's `while(_dmcDmaRunning || _spriteDmaTransfer)`.
    ///
    /// Reads land on even cycles and writes on odd ones, which is what gives a
    /// sprite transfer its 513 (or 514) cycles and lets the DMC's fetch share
    /// the machine with it.
    fn run_dma_cycle(&mut self) {
        let get_cycle = self.cpu_cycles & 1 == 0;

        if get_cycle {
            if self.dmc_dma_running && !self.need_halt && !self.need_dummy_read {
                // Both the halt and the dummy-read cycle are behind it.
                self.dma_process_cycle();
                let addr = self.apu.dmc_read_address();
                self.dma_read_value = self.process_dma_read(addr);
                self.end_cpu_cycle();
                self.dmc_dma_running = false;
                self.abort_dmc_dma = false;
                self.apu.supply_dmc_byte(self.dma_read_value);
                if self.need_halt {
                    // A second transfer can start immediately behind this one,
                    // and it wants its halt cycle before the loop goes on.
                    self.dma_halt_cycle();
                }
            } else if self.sprite_dma_transfer {
                self.dma_process_cycle();
                let addr =
                    ((self.sprite_dma_offset as u16) << 8) | self.dma_sprite_read_addr as u16;
                self.dma_read_value = self.process_dma_read(addr);
                self.end_cpu_cycle();
                self.dma_sprite_read_addr = self.dma_sprite_read_addr.wrapping_add(1);
                self.dma_sprite_counter += 1;
            } else {
                // The transfer is running but not ready: a dummy read at the
                // address the CPU was about to use.
                self.dma_process_cycle();
                let _ = self.read(self.dma_read_address);
                self.end_cpu_cycle();
            }
        } else if self.sprite_dma_transfer && (self.dma_sprite_counter & 1) != 0 {
            // The write half of a sprite transfer's read/write pair.
            self.dma_process_cycle();
            self.write(0x2004, self.dma_read_value);
            self.end_cpu_cycle();
            self.dma_sprite_counter += 1;
            if self.dma_sprite_counter == 0x200 {
                self.sprite_dma_transfer = false;
            }
        } else {
            // Aligning to a read cycle before the sprite transfer starts (or
            // before the DMC's own read).
            self.dma_process_cycle();
            let _ = self.read(self.dma_read_address);
            self.end_cpu_cycle();
        }
    }

    /// Mesen's `processCycle` lambda: one of the halt/dummy cycles goes by, and
    /// the cycle itself starts.
    fn dma_process_cycle(&mut self) {
        if self.abort_dmc_dma {
            self.dmc_dma_running = false;
            self.abort_dmc_dma = false;
            self.need_dummy_read = false;
            self.need_halt = false;
        } else if self.need_halt {
            self.need_halt = false;
        } else if self.need_dummy_read {
            self.need_dummy_read = false;
        }
        self.begin_cpu_cycle();
    }

    /// Mesen's `ProcessDmaRead`: what the DMA unit sees on the bus.
    ///
    /// The interesting part is the 2A03 bug it reproduces — a CPU halted while
    /// reading `$4000-$401F` makes the DMA read *its own* registers at the same
    /// time, so `$4015` can be read (and its frame-counter flag cleared) behind
    /// the program's back, input bits can be lost, and the byte the DMC gets
    /// can be replaced by the conflict between the two.
    fn process_dma_read(&mut self, dma_addr: u16) -> u8 {
        if !self.dma_internal_reg_reads {
            let value = if (0x4015..=0x401A).contains(&dma_addr) {
                // The readable 2A03 registers cannot be seen by the DMA here.
                self.open_bus
            } else {
                self.read(dma_addr)
            };
            self.dma_prev_read_address = dma_addr;
            return value;
        }

        let internal_addr = 0x4000 | (dma_addr & 0x1F);
        let same_address = internal_addr == dma_addr;

        let value = match internal_addr {
            0x4015 => {
                // A read of $4015 touches the CPU's internal bus only, so the
                // external one keeps its value — which is what lets the DMC
                // fetch a clean byte out of this.
                let value = self.read_apu_register(0x4015);
                if !same_address {
                    let _ = self.read(dma_addr);
                }
                value
            }
            0x4016 | 0x4017 => {
                let value = self.read_apu_register(internal_addr);
                if same_address {
                    value
                } else {
                    let mask = CONTROLLER_OPEN_BUS_MASK;
                    let external = self.read(dma_addr);
                    // The pads stop driving the bus later than the cartridge
                    // does, so their bits win on the open bus.
                    self.open_bus = (external & mask) | (value & !mask);
                    // And the byte the CPU sees is the bus conflict between
                    // them: the open-bus pins keep the external value, every
                    // other bit is the AND of the two.
                    (external & mask) | ((value & !mask) & (external & !mask))
                }
            }
            _ => self.read(dma_addr),
        };

        self.dma_prev_read_address = internal_addr;
        value
    }

    /// Advance the PPU and APU by `cycles` CPU cycles without executing any
    /// CPU instructions.
    fn advance_cycles(&mut self, cycles: u16) {
        for _ in 0..cycles {
            self.advance_one_cpu_cycle();
        }
    }

    /// Master-clock ticks one CPU cycle takes.
    #[inline]
    fn cpu_divider(&self) -> u64 {
        self.region.cpu_divider() as u64
    }

    /// Whether the chip is currently in an "odd" CPU cycle.
    ///
    /// Our counter is the number of cycles *completed*, and it is incremented
    /// when a cycle ends; Mesen's `CycleCount` is incremented when one starts,
    /// so a value read *during* a cycle is one higher there than here. Anything
    /// whose behaviour depends on the parity and is read mid-cycle has to count
    /// from the current cycle — this is that count, and it is what makes a
    /// `$4017` restart three cycles late instead of four and a `$4016` write
    /// reach the pads a cycle later than it otherwise would (Mesen's
    /// `_transferStartDelay` and `_writePending` both read its `GetMasterClock`
    /// the same way).
    ///
    /// A parity read *between* cycles — the DMA loop deciding whether the next
    /// cycle is a read — uses [`NesBus::cpu_cycles`] directly instead.
    #[inline]
    fn in_an_odd_cpu_cycle(&self) -> bool {
        self.cpu_cycles & 1 == 0
    }

    /// Master-clock ticks one PPU dot takes.
    #[inline]
    fn ppu_divider(&self) -> u64 {
        self.region.ppu_divider() as u64
    }

    /// Master clock at the start of CPU cycle `cpu_cycles`.
    ///
    /// Derived from the cycle count rather than stored: the PPU's dot sequence
    /// is a function of it, so a save state needs no new field and a load can
    /// never leave the two out of step.
    #[inline]
    fn master_clock_at(&self, cpu_cycles: u64) -> u64 {
        cpu_cycles * self.cpu_divider()
    }

    /// The PPU's own clock at the start of CPU cycle `cpu_cycles`: the master
    /// clock rounded down to the last dot boundary.
    ///
    /// A whole ratio — both regions today — needs no rounding, because the CPU's
    /// cycle is then an exact multiple of a dot, so the division is only reached
    /// for the fractional ratio a real PAL board has. That keeps the machine's
    /// inner loop free of it: measured, a division per cycle here costs about 5%
    /// of the frame.
    #[inline]
    fn ppu_clock_at(&self, cpu_cycles: u64) -> u64 {
        let start = self.master_clock_at(cpu_cycles);
        if self.has_whole_dot_ratio() {
            start
        } else {
            let divider = self.ppu_divider();
            start - start % divider
        }
    }

    /// Whether one CPU cycle is a whole number of PPU dots.
    ///
    /// NTSC's 12/4 is, which lets the inner loop run a counted dot loop instead
    /// of doing master-clock arithmetic per cycle (measured, that arithmetic is
    /// worth about 4% of the frame). PAL's 16/5 is the case the general path
    /// exists for, and it is the one PAL takes:
    /// `test_the_whole_ratio_flags_agree_with_the_dividers` holds the flag to
    /// the dividers so the two cannot drift apart.
    #[inline]
    fn has_whole_dot_ratio(&self) -> bool {
        match self.region {
            Region::Ntsc => true,
            // A PAL cycle is 3.2 dots: the PPU takes four on some cycles and
            // three on the others, which is what the master-clock path below
            // exists for. Flipping this on without the dividers to match fails
            // `test_the_whole_ratio_flags_agree_with_the_dividers`.
            Region::Pal => false,
        }
    }

    /// Dots one CPU cycle covers, for a whole ratio.
    ///
    /// Spelled out per region so the inner loop divides nothing;
    /// `test_the_whole_ratio_flags_agree_with_the_dividers` holds it to
    /// `cpu_divider / ppu_divider`. Only NTSC reaches it — a PAL cycle is 3.2
    /// dots and takes the master-clock path — but the arm has to say something,
    /// and the value that agrees with the dividers is the one to say.
    #[inline]
    fn whole_dots_per_cpu_cycle(&self) -> u32 {
        match self.region {
            Region::Ntsc => 3,
            Region::Pal => 3,
        }
    }

    /// Run the PPU up to `target` on the master clock, one dot at a time.
    ///
    /// This is the shape Mesen's `StartCpuCycle`/`EndCpuCycle` use: the CPU
    /// works out *where* in its cycle an access belongs, and the machine runs
    /// the PPU up to that instant and on to the end of the cycle. It is what
    /// lets a region whose ratio is not a whole number of dots per CPU cycle
    /// run without a fractional clock field — the PPU simply takes four dots on
    /// some cycles and three on the others.
    ///
    /// `clock` is the PPU's position, carried by the caller across a cycle so
    /// an access can be placed in the middle of one.
    #[inline]
    fn run_ppu_to(&mut self, clock: &mut u64, target: u64) {
        let divider = self.ppu_divider();
        while *clock + divider <= target {
            *clock += divider;
            self.advance_one_ppu_dot();
        }
    }

    /// Master clock at which the chip performs an access inside CPU cycle
    /// `cpu_cycles`: [`ACCESS_DOTS`] dots in, expressed in ticks so that it does
    /// not depend on how many dots the cycle happens to hold.
    #[inline]
    fn access_clock(&self, cpu_cycles: u64) -> u64 {
        self.master_clock_at(cpu_cycles) + ACCESS_DOTS as u64 * self.ppu_divider()
    }

    /// One CPU cycle of the machine around the CPU: the PPU dots that cycle
    /// covers, an APU tick, and the cartridge's view of the PPU bus.
    ///
    /// The two chips are interleaved here rather than advanced in separate
    /// lumps, so neither is ever ahead of the other.
    fn advance_one_cpu_cycle(&mut self) {
        self.begin_cpu_cycle();
        self.end_cpu_cycle();
    }

    /// The PPU's bus timeline, oldest sample first, one entry per dot.
    ///
    /// Only the last [`NesBus::ppu_bus_trace_capacity`] dots are kept.
    fn ppu_bus_trace(&self) -> &VecDeque<PpuBusSample> {
        &self.ppu_bus_trace
    }

    /// Drop the recorded bus timeline, so the next samples start a fresh one.
    fn clear_ppu_bus_trace(&mut self) {
        self.ppu_bus_trace.clear();
    }

    /// Set how many dots of bus timeline to keep, dropping the oldest samples
    /// if the new capacity is smaller.
    fn set_ppu_bus_trace_capacity(&mut self, dots: usize) {
        self.ppu_bus_trace_capacity = dots;
        while self.ppu_bus_trace.len() > dots {
            self.ppu_bus_trace.pop_front();
        }
    }

    /// The last [`NesBus::ppu_write_log_capacity`] PPU register writes, oldest
    /// first.
    fn ppu_write_log(&self) -> &VecDeque<PpuWriteSample> {
        &self.ppu_write_log
    }

    /// Turn the PPU register write log on (`writes > 0`) or off (`0`) and size
    /// it; the oldest samples go when it shrinks.
    fn set_ppu_write_log_capacity(&mut self, writes: usize) {
        self.ppu_write_log_capacity = writes;
        while self.ppu_write_log.len() > writes {
            self.ppu_write_log.pop_front();
        }
    }

    /// Advance the machine by exactly one PPU dot.
    ///
    /// The CPU runs whole instructions, so the instruction this dot falls
    /// inside has already executed by the time the call returns: what moves a
    /// dot at a time is the PPU — its dot counter, its fetches, the address the
    /// cartridge sees on the bus and the lines the CPU samples. Everything
    /// observed through the PPU (A12, the fetch a mid-frame write lands behind,
    /// the VBlank flag and its NMI line) is therefore in machine order, while
    /// the CPU's own registers may be up to one instruction ahead. A caller
    /// that needs dot-exact samples reads [`NesBus::ppu_bus_trace`], which the
    /// machine fills in the order the dots happen; dot stepping a debugger is
    /// what *this* is for.
    ///
    /// Returns `true` when this dot completed a frame. That is decided against
    /// the PPU's own frame counter rather than the frame-ready flag, so a
    /// caller can step frame after frame without clearing anything in between
    /// — and a host reading [`Nes::is_frame_ready`] still sees the flag.
    fn step_dot(&mut self, cpu: &mut Cpu) -> bool {
        let frame_before = self.ppu.frame;
        if self.pending_dots == 0 {
            let cycles = self.step_instruction(cpu);
            self.pending_dots = cycles as u32 * self.whole_dots_per_cpu_cycle();
        }
        self.pending_dots -= 1;
        self.ppu.frame != frame_before
    }

    /// The tail of a CPU cycle: the cycle counter.
    ///
    /// Neither the APU nor the interrupt lines are here any more. The APU runs
    /// at the *start* of the cycle, with the other chips (Mesen's
    /// `ProcessCpuClock`, called from `StartCpuCycle`), and the lines are
    /// sampled at the end of it by the CPU (`Cpu::end_cycle`, Mesen's
    /// `EndCpuCycle`).
    fn finish_cpu_cycle(&mut self) {
        self.cpu_cycles += 1;
    }

    /// The first part of a CPU cycle: the PPU dots up to the point in it where
    /// the chip performs its bus access, then the APU's clock for the cycle.
    ///
    /// A whole dot ratio (both regions today) simply steps [`ACCESS_DOTS`]
    /// dots; a ratio that is not whole runs the PPU to the master-clock instant
    /// the access belongs at. This is Mesen's `StartCpuCycle`, and the APU tick
    /// sits at the end of it because that is where Mesen's `ProcessCpuClock` is
    /// (the cartridges' per-cycle hooks belong there too and are still missing,
    /// `docs/mesen-alignment.md` §2.5): the chip clocks the APU *before* the
    /// cycle's own access, so a `$4017` write is seen by the *next* cycle's tick
    /// and the frame counter's steps land one cycle later relative to the access
    /// than they would if the tick trailed it. blargg's `cpu_interrupts_v2`
    /// measures that cycle — with the tick behind the access, all three of its
    /// remaining tables failed.
    fn begin_cpu_cycle(&mut self) {
        if self.has_whole_dot_ratio() {
            for _ in 0..ACCESS_DOTS {
                self.advance_one_ppu_dot();
            }
        } else {
            let mut clock = self.ppu_clock_at(self.cpu_cycles);
            self.run_ppu_to(&mut clock, self.access_clock(self.cpu_cycles));
        }
        self.tick_apu_cycle();

        // The controller pins the CPU wrote are updated here, at the start of a
        // cycle, rather than on the write itself (Mesen's `ProcessCpuClock`
        // calling `ProcessWrites`).
        if let Some(pending) = self.pending_controller_write {
            if pending.cycles_left <= 1 {
                self.pending_controller_write = None;
                for controller in &mut self.controllers {
                    controller.write_strobe(pending.value & 1 != 0);
                }
            } else {
                self.pending_controller_write = Some(PendingControllerWrite {
                    value: pending.value,
                    cycles_left: pending.cycles_left - 1,
                });
            }
        }
    }

    /// The rest of the cycle: the remaining dots, then the bookkeeping the chip
    /// does at the end of one. Mesen's `EndCpuCycle`, which runs the PPU's dots
    /// and leaves the interrupt-line samples to the CPU.
    ///
    /// Every cycle the machine advances goes through here, the cycles an
    /// interrupt sequence takes and the ones DMA steals included.
    fn end_cpu_cycle(&mut self) {
        if self.has_whole_dot_ratio() {
            for _ in ACCESS_DOTS..self.whole_dots_per_cpu_cycle() {
                self.advance_one_ppu_dot();
            }
        } else {
            let mut clock = self.access_clock(self.cpu_cycles);
            self.run_ppu_to(&mut clock, self.master_clock_at(self.cpu_cycles + 1));
        }
        self.finish_cpu_cycle();
    }

    /// One PPU dot: the dot counter, everything a dot triggers, and the address
    /// the cartridge sees on the bus.
    fn advance_one_ppu_dot(&mut self) {
        // Update PPU (3 PPU cycles per CPU cycle)
        self.ppu.cycle += 1;

        // While anything is being rendered, the pre-render line is one dot
        // short on odd frames: the picture runs at 60.0985 Hz instead of an
        // even 60.1, and blargg's even/odd-frame tests measure exactly this
        // dot.
        //
        // The *decision* is made a dot before the dot it skips: dot 339 asks
        // whether the frame is short, dot 340 is the one that goes missing. A
        // write that reaches `$2001` in between arrives after the chip has
        // made up its mind (Mesen's counter jumps to 340 while it is processing
        // 339), and a decision taken on 340 itself would count that write.
        //
        // What the decision reads is the rendering state *as of the end of the
        // previous dot*, not the live mask: Mesen's `_renderingEnabled` is
        // refreshed by its `UpdateState` after each dot, so a `$2001` write
        // that lands in the same dot as the decision is one dot too late for
        // it. `10-even_odd_timing`'s checks 2-5 walk a write across that
        // boundary one CPU cycle at a time and pin it down.
        let rendering = self.rendering_enabled_delayed;
        if self.ppu.scanline == self.region.last_scanline()
            && self.ppu.cycle == 339
            && rendering
            && self.ppu.frame & 1 == 1
        {
            self.skip_odd_frame_dot = true;
        }

        let skip_odd_frame_dot = self.skip_odd_frame_dot && self.ppu.cycle == 340;
        if self.ppu.cycle > 340 || skip_odd_frame_dot {
            self.skip_odd_frame_dot = false;
            self.ppu.cycle = 0;
            self.ppu.scanline += 1;

            if self.ppu.scanline > self.region.last_scanline() {
                self.ppu.scanline = 0;
                self.ppu.frame += 1;
                self.frames_completed += 1;
                self.frame_ready = true;

                // Show the finished frame by exchanging buffers with the
                // renderer: the host keeps reading a complete frame while
                // the next one is drawn, and no pixels are copied.
                self.frame_buffer
                    .swap_pixels(self.pipeline.frame_buffer_mut());
            }

            // Let mappers with a scanline counter tick here.
            let scanline = self.ppu.scanline;
            let rendering = self
                .ppu
                .mask
                .intersects(PpuMask::SHOW_BG | PpuMask::SHOW_SPRITES);
            if let Some(ref mut mapper) = self.mapper {
                mapper.on_scanline(scanline, rendering);

                // Mappers such as MMC1 and MMC3 can switch nametable
                // mirroring while the game runs; follow whatever they
                // report so rendering and PPUDATA stay in sync.
                if let Some(mode) = mapper.mirroring() {
                    if mode != self.mirroring {
                        self.mirroring = mode;
                        self.pipeline.set_mirroring(mode);
                    }
                }
            }
        }
        self.ppu_step();
        self.feed_ppu_bus();
        // The register state machines run at the tail of the dot, after its own
        // fetches — Mesen's `UpdateState`, at the end of `Exec`.
        self.tick_ppu_register_state();
        self.record_ppu_bus_sample();

        // The rendering state every dot leaves behind for the next one to read
        // (Mesen's `UpdateState`, which runs at the end of each `Exec`). The
        // odd frame's skipped dot is what depends on it most visibly.
        self.rendering_enabled_delayed = self
            .ppu
            .mask
            .intersects(PpuMask::SHOW_BG | PpuMask::SHOW_SPRITES);
    }

    /// Append the dot the PPU just finished to the bus timeline.
    fn record_ppu_bus_sample(&mut self) {
        if self.ppu_bus_trace_capacity == 0 {
            return;
        }
        if self.ppu_bus_trace.len() == self.ppu_bus_trace_capacity {
            self.ppu_bus_trace.pop_front();
        }
        self.ppu_bus_trace.push_back(PpuBusSample {
            scanline: self.ppu.scanline,
            dot: self.ppu.cycle,
            address: self.bus_address,
            ctrl: self.ppu.ctrl.bits(),
            mask: self.ppu.mask.bits(),
            status: self.ppu.status.bits(),
            oam_addr: self.ppu.oam_addr,
            v: self.ppu.v,
            t: self.ppu.t,
            x: self.ppu.x,
        });
    }
}

/// The emulated console.
///
/// Owns the CPU and the system bus (PPU, APU, cartridge and controllers) and
/// exposes everything a front end needs: run a frame, read the picture, feed
/// the controller and persist save RAM.
pub struct Nes {
    cpu: Cpu,
    bus: NesBus,
    config: Config,
}

impl Default for Nes {
    fn default() -> Self {
        Self::new()
    }
}

impl Nes {
    /// Create a console with the default [`Config`] and no cartridge.
    pub fn new() -> Self {
        Self::with_config(Config::default())
    }

    /// Create a console with an explicit configuration and no cartridge.
    ///
    /// The configuration takes effect when a cartridge is loaded: a region
    /// override replaces the header's TV system, and audio capture can be
    /// turned off for hosts that do not play sound.
    pub fn with_config(config: Config) -> Self {
        let mut bus = NesBus::new();
        bus.apply_config(&config);
        let mut cpu = Cpu::new();
        bus.power_on(&mut cpu);
        Self { cpu, bus, config }
    }

    /// The configuration this console was created with.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Load an iNES cartridge image and reset the console.
    pub fn load_rom(&mut self, data: &[u8]) -> crate::Result<()> {
        self.bus.load_rom(data)?;
        // Mesen's `LoadRom` powers the machine on (`Reset(false, ...)` for every
        // chip), which is what gives the CPU `SP = $FD` and clears the RAM.
        self.bus.power_on(&mut self.cpu);
        Ok(())
    }

    /// Reset the console as if the reset button was pressed.
    pub fn reset(&mut self) {
        self.bus.reset(&mut self.cpu);
    }

    /// Run one frame of emulation.
    ///
    /// Audio produced along the way stays queued for [`Nes::fill_audio`].
    pub fn frame(&mut self) {
        self.bus.run_frame(&mut self.cpu);
    }

    /// Run at most `cycles` CPU cycles, stopping early at a frame boundary.
    ///
    /// Returns the cycles actually run. For hosts that pace the console in
    /// sub-frame slices — a cycle-driven audio clock, or a debugger — rather
    /// than a frame at a time. Check [`Nes::is_frame_ready`] afterwards, then
    /// pull the audio with [`Nes::fill_audio`].
    pub fn run_cycles(&mut self, cycles: u32) -> u32 {
        self.bus.run_cycles(&mut self.cpu, cycles)
    }

    /// Execute a single CPU instruction and return the cycles it consumed.
    ///
    /// The finest-grained entry point, meant for stepping through code one
    /// instruction at a time.
    pub fn step_instruction(&mut self) -> u8 {
        self.bus.step_instruction(&mut self.cpu)
    }

    /// Advance the machine by exactly one PPU dot, returning `true` when that
    /// dot completed a frame.
    ///
    /// The companion to [`Nes::step_instruction`], one chip over: this is the
    /// granularity the PPU, and therefore the cartridge's view of the bus,
    /// works at. The CPU still runs whole instructions, so the instruction the
    /// dot falls inside has already executed when this returns — what moves a
    /// dot at a time is the PPU. Observations made through the PPU (A12, the
    /// fetches, the VBlank flag's NMI line) are in machine order; a caller that
    /// wants dot-exact *samples* rather than dot-exact pacing reads
    /// [`Nes::ppu_bus_trace`], which the machine fills as the dots happen.
    ///
    /// The frame-ready flag is left alone, exactly as [`Nes::step_instruction`]
    /// leaves it, so a debugger can step across a frame boundary and still see
    /// it with [`Nes::is_frame_ready`].
    pub fn step_dot(&mut self) -> bool {
        self.bus.step_dot(&mut self.cpu)
    }

    /// ARGB frame buffer (256x240).
    pub fn frame_buffer(&self) -> &[u32] {
        &self.bus.frame_buffer().pixels
    }

    /// Pointer to the frame buffer, for hosts that want a zero-copy view.
    pub fn frame_buffer_ptr(&self) -> *const u32 {
        self.bus.frame_buffer().pixels.as_ptr()
    }

    /// Set a controller button state. `player` is 1 or 2.
    pub fn set_button(&mut self, player: u8, button: u8, pressed: bool) {
        self.bus.set_button(player, button, pressed);
    }

    /// Rate the core resamples audio to, in Hz.
    ///
    /// This is [`Config::sample_rate`], so a host can size its device buffers
    /// from the same number the core produces.
    pub fn audio_sample_rate(&self) -> u32 {
        self.bus.audio_sample_rate
    }

    /// Change the audio sample rate, e.g. to follow a device that switched to
    /// 44_100 Hz. Queued samples are dropped; zero keeps the current rate.
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.bus.set_sample_rate(sample_rate);
        if sample_rate > 0 {
            self.config.sample_rate = sample_rate;
        }
    }

    /// Stereo frames emulated but not yet pulled with [`Nes::fill_audio`].
    pub fn buffered_audio_frames(&self) -> usize {
        self.bus.buffered_audio_frames()
    }

    /// Copy the oldest buffered audio into `out`, interleaved left/right.
    ///
    /// This is the interface an audio device callback wants: the host owns the
    /// buffer, asks for as much as its device period needs, and gets back how
    /// many floats were written — zero when the console has not produced
    /// anything since the last call, less than `out.len()` when it has produced
    /// only part.
    ///
    /// Audio is already resampled to [`Nes::audio_sample_rate`], so hosts never
    /// resample it themselves: a 44.1 kHz device asks for 44_100 at startup and
    /// is handed samples at exactly that rate. The count is always a whole
    /// number of stereo frames, i.e. even.
    ///
    /// Samples accumulate as the console runs — [`Nes::frame`] no longer clears
    /// them — and the oldest are dropped once roughly half a second is queued,
    /// so a host that stops pulling loses stale sound instead of growing a
    /// backlog it can never drain.
    pub fn fill_audio(&mut self, out: &mut [f32]) -> usize {
        self.bus.fill_audio(out)
    }

    /// Drop every buffered audio sample, e.g. when loading a cartridge.
    pub fn clear_audio_samples(&mut self) {
        self.bus.clear_audio_samples();
    }

    /// Whether the cartridge keeps battery-backed save RAM.
    pub fn has_battery(&self) -> bool {
        self.bus.has_battery()
    }

    /// The cartridge's NES 2.0 submapper, or 0 for a plain iNES file.
    ///
    /// A submapper says which board *within* a mapper a cartridge is — the
    /// distinction the discrete boards' variants turn on. Hosts can show it;
    /// nothing in the core dispatches on it yet, because the titles that need
    /// it (`6-MMC3_alt` and friends) carry plain iNES headers, where the choice
    /// lives in a database rather than in the file.
    pub fn rom_submapper(&self) -> u8 {
        self.bus.submapper
    }

    /// Snapshot of the cartridge's battery RAM, or `None` when it has none.
    pub fn save_ram(&self) -> Option<Vec<u8>> {
        self.bus.save_ram()
    }

    /// Restore a snapshot produced by [`Nes::save_ram`].
    pub fn load_save_ram(&mut self, data: &[u8]) {
        self.bus.load_save_ram(data);
    }

    /// Number of frames rendered so far.
    pub fn frame_number(&self) -> u64 {
        self.bus.frames_completed
    }

    /// Whether a frame has finished since the last [`Nes::clear_frame_ready`].
    pub fn is_frame_ready(&self) -> bool {
        self.bus.frame_ready
    }

    /// CPU cycles the machine has run since it was created.
    ///
    /// Counts every cycle the machine advanced, the cycles an interrupt
    /// sequence takes and the cycles OAM/DMC DMA steals included.
    pub fn cpu_cycles(&self) -> u64 {
        self.bus.cpu_cycles
    }

    /// Master-clock ticks since power-on.
    ///
    /// The machine keeps one clock and derives both chips from it (Mesen's
    /// shape): a CPU cycle advances it by [`Region::cpu_divider`] ticks and a
    /// PPU dot by [`Region::ppu_divider`]. The value is always taken at a CPU
    /// cycle boundary, i.e. `cpu_cycles() * cpu_divider()`.
    pub fn master_clock(&self) -> u64 {
        self.bus.master_clock_at(self.bus.cpu_cycles)
    }

    /// PPU dots the machine has run since power-on.
    ///
    /// The count follows the region's clock ratio, so it is three dots per CPU
    /// cycle on both regions today and will take a fourth dot every fifth cycle
    /// once PAL runs at its real 3.2:1 (§15.1).
    pub fn ppu_dots(&self) -> u64 {
        self.master_clock() / self.bus.ppu_divider()
    }

    /// Where the PPU is right now: `(scanline, dot)`.
    ///
    /// Taken from the PPU itself rather than derived from the master clock, so
    /// it is exact mid-cycle too — the pair a debugger or a trace tool needs.
    pub fn ppu_position(&self) -> (u16, u16) {
        (self.bus.ppu.scanline(), self.bus.ppu.cycle())
    }

    /// The CPU's program counter, i.e. the next byte it will fetch.
    pub fn cpu_pc(&self) -> u16 {
        self.cpu.pc()
    }

    /// The address the PPU is driving for the cartridge right now.
    ///
    /// What an A12-sensitive mapper sees: the address of the last access the
    /// chip made, held between accesses (Mesen's `_ppuBusAddress`). Bit 12 is
    /// A12.
    pub fn ppu_bus_address(&self) -> u16 {
        self.bus.bus_address
    }

    /// The PPU's bus timeline, oldest sample first, one entry per dot.
    ///
    /// This is the companion to `tests/cycle_trace.rs`'s per-instruction view,
    /// at the resolution the cartridge sees: A12 rises between *fetch dots*,
    /// and a scanline counter's nine-dot filter is measured in dots, so
    /// anything that asks "which fetches clocked the counter" has to read the
    /// bus a dot at a time. Samples are recorded as the machine runs; only the
    /// last [`Nes::ppu_bus_trace_capacity`] dots are kept.
    pub fn ppu_bus_trace(&self) -> impl Iterator<Item = PpuBusSample> + '_ {
        self.bus.ppu_bus_trace().iter().copied()
    }

    /// Drop the recorded bus timeline, so the next samples start a fresh one.
    pub fn clear_ppu_bus_trace(&mut self) {
        self.bus.clear_ppu_bus_trace();
    }

    /// Raise or lower how many dots of bus timeline [`Nes::ppu_bus_trace`]
    /// keeps; the oldest samples go when it shrinks.
    ///
    /// A whole NTSC frame is 89,342 dots, so counting qualifying A12 rises a
    /// frame at a time — the measurement `mmc3_test_2` turns on — asks for at
    /// least that many.
    pub fn set_ppu_bus_trace_capacity(&mut self, dots: usize) {
        self.bus.set_ppu_bus_trace_capacity(dots);
    }

    /// How many dots of bus timeline [`Nes::ppu_bus_trace`] keeps.
    pub fn ppu_bus_trace_capacity(&self) -> usize {
        self.bus.ppu_bus_trace_capacity
    }

    /// The last [`Nes::ppu_write_log_capacity`] writes to a PPU register, oldest
    /// first, each with the dot it landed on and `$2002` as it stood there.
    ///
    /// Mid-line writes are what the sprite phase and the scroll registers turn
    /// on — AccuracyCoin's `Suddenly Resize Sprite` is a `$2000` write a few CPU
    /// cycles either side of a sprite fetch — and this is the log the accuracy
    /// work diffs between two builds: the *first* write that differs is worth
    /// more than any count. It records nothing until
    /// [`Nes::set_ppu_write_log_capacity`] is called, because it costs a push on
    /// every register write.
    pub fn ppu_write_log(&self) -> impl Iterator<Item = PpuWriteSample> + '_ {
        self.bus.ppu_write_log().iter().copied()
    }

    /// Drop the recorded register writes, so the next ones start a fresh log.
    pub fn clear_ppu_write_log(&mut self) {
        self.bus.ppu_write_log.clear();
    }

    /// Raise or lower how many PPU register writes [`Nes::ppu_write_log`] keeps;
    /// `0` turns recording off. The oldest samples go when it shrinks.
    pub fn set_ppu_write_log_capacity(&mut self, writes: usize) {
        self.bus.set_ppu_write_log_capacity(writes);
    }

    /// How many PPU register writes [`Nes::ppu_write_log`] keeps (`0`: off).
    pub fn ppu_write_log_capacity(&self) -> usize {
        self.bus.ppu_write_log_capacity
    }

    /// Clear the frame-ready flag.
    pub fn clear_frame_ready(&mut self) {
        self.bus.frame_ready = false;
    }

    /// Whether a KIL/JAM opcode has stopped the CPU.
    ///
    /// A halted machine keeps running — the PPU still draws, the APU still
    /// plays, frames still complete — but no instruction is fetched until an
    /// interrupt or a reset arrives, so the picture will not change again.
    /// Hosts can use this to tell the user the cartridge has hung instead of
    /// leaving them with a frozen screen.
    pub fn is_cpu_halted(&self) -> bool {
        self.cpu.is_halted()
    }

    /// Read a byte straight off the system bus.
    ///
    /// Exposed for diagnostics and tests; games should not need it.
    pub fn peek(&mut self, addr: u16) -> u8 {
        self.bus.read(addr)
    }

    /// Nametable mirroring currently in effect.
    pub fn mirroring(&self) -> Mirroring {
        self.bus.mirroring
    }

    /// TV system of the loaded cartridge.
    pub fn region(&self) -> Region {
        self.bus.region
    }

    /// Frame rate of the loaded cartridge in Hz.
    ///
    /// Cartridges are NTSC or PAL and the two run at different speeds, so a
    /// host that paces the console should read this after loading a ROM rather
    /// than assuming 60 Hz. Pair it with [`crate::FrameClock`].
    pub fn frame_rate(&self) -> f64 {
        self.bus.region.fps()
    }

    /// Serialize the whole machine (CPU + system) into a save state.
    ///
    /// The cartridge's PRG-ROM image is not included, so a state must be
    /// restored onto the same cartridge it was taken from.
    pub fn save_state(&self) -> Vec<u8> {
        let mut w = StateWriter::new();
        for byte in STATE_MAGIC {
            w.u8(*byte);
        }
        w.u8(STATE_VERSION);
        crate::state::StateField::write(&self.cpu, &mut w);
        crate::state::StateField::write(&self.bus, &mut w);
        w.into_bytes()
    }

    /// Restore a state produced by [`Nes::save_state`].
    ///
    /// Data that is not a state for this version is ignored, leaving the
    /// machine untouched.
    pub fn load_state(&mut self, data: &[u8]) {
        const HEADER: usize = STATE_MAGIC.len() + 1;
        if data.len() < HEADER
            || !data.starts_with(STATE_MAGIC)
            || data[STATE_MAGIC.len()] != STATE_VERSION
        {
            return;
        }

        let mut r = StateReader::new(&data[HEADER..]);
        crate::state::StateField::read(&mut self.cpu, &mut r);
        crate::state::StateField::read(&mut self.bus, &mut r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_nes_new() {
        let _nes = Nes::new();
    }

    #[test]
    fn test_step_dot_walks_a_frame_of_ppu_bus_timeline() {
        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");

        // A freshly loaded cartridge has already spent the eight dummy cycles a
        // reset costs the CPU (`RESET_WARM_UP_CYCLES`), which is where the PPU
        // is standing when the first instruction runs.
        assert_eq!(nes.ppu_position(), (0, (RESET_WARM_UP_CYCLES * 3) as u16));
        let frame_dots = nes.region().scanlines() as usize * 341;
        nes.set_ppu_bus_trace_capacity(frame_dots * 2);
        nes.clear_ppu_bus_trace();

        // One call per dot, and the frame ends on the dot that wraps the
        // counter past the pre-render line.
        let mut calls = 0usize;
        while !nes.step_dot() {
            calls += 1;
            assert!(calls <= frame_dots, "a frame should have ended by now");
        }

        // The bus timeline is the thing that is dot-exact: the rest of the
        // frame's samples, each one dot past the last. (The frame has itself
        // spent its first 24 dots on the reset's warm-up, and the trace was
        // cleared after them.) The machine may have recorded a few more,
        // because the instruction the boundary fell inside runs to its end, so
        // the claim is over the frame's own dots.
        let trace: Vec<_> = nes.ppu_bus_trace().collect();
        let warm_up_dots = (RESET_WARM_UP_CYCLES * 3) as usize;
        let rest_of_the_frame = frame_dots - warm_up_dots;
        assert!(
            trace.len() >= rest_of_the_frame,
            "the rest of the frame is {rest_of_the_frame} dots of PPU bus timeline, got {}",
            trace.len()
        );
        let scanlines = nes.region().scanlines() as usize;
        for (index, sample) in trace[..rest_of_the_frame].iter().enumerate() {
            let dot = index + 1 + warm_up_dots;
            let expected = dot % 341;
            let expected_scanline = (dot / 341) % scanlines;
            assert_eq!(
                (sample.scanline as usize, sample.dot as usize),
                (expected_scanline, expected),
                "sample {index} is out of step with the dot grid"
            );
        }
    }

    #[test]
    fn test_step_dot_reports_the_frame_boundary_once_per_frame() {
        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");

        for frame in 1..=3 {
            let mut calls = 0u32;
            while !nes.step_dot() {
                calls += 1;
                assert!(calls < 200_000, "frame {frame} never ended");
            }
            assert_eq!(nes.frame_number(), frame);
        }
    }

    #[test]
    fn test_ppu_bus_trace_records_every_dot_in_order() {
        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");

        nes.clear_ppu_bus_trace();
        for _ in 0..30 {
            nes.step_dot();
        }

        // Walking 30 dots records at least 30 samples: one call spends one dot,
        // and a call only runs more machine when the last instruction's dots
        // have all been spent.
        let trace: Vec<_> = nes.ppu_bus_trace().collect();
        assert!(trace.len() >= 30, "got {} samples", trace.len());
        let warm_up_dots = (RESET_WARM_UP_CYCLES * 3) as u16;
        for (index, sample) in trace.iter().enumerate() {
            assert_eq!(sample.scanline, 0);
            assert_eq!(
                sample.dot as usize,
                index + 1 + warm_up_dots as usize,
                "sample {index} is out of step"
            );
        }
        // The timeline ends where the machine actually is.
        let last = trace.last().copied().expect("samples");
        assert_eq!(nes.ppu_position(), (last.scanline, last.dot));
    }

    #[test]
    fn test_ppu_bus_trace_keeps_only_the_requested_capacity() {
        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");

        assert_eq!(nes.ppu_bus_trace_capacity(), DEFAULT_PPU_BUS_TRACE_DOTS);
        nes.set_ppu_bus_trace_capacity(8);
        nes.clear_ppu_bus_trace();

        // A single instruction already overshoots eight dots, so the buffer
        // holds the newest eight and the timeline still ends where the machine
        // is.
        nes.step_instruction();

        let trace: Vec<_> = nes.ppu_bus_trace().collect();
        assert_eq!(trace.len(), 8, "the oldest samples are dropped");
        let dots: Vec<u16> = trace.iter().map(|sample| sample.dot).collect();
        // The eight samples end where the instruction the reset left behind
        // leaves the PPU: seven CPU cycles past the warm-up's dot.
        let warm_up_dots = (RESET_WARM_UP_CYCLES * 3) as u16;
        assert_eq!(
            dots,
            (warm_up_dots + 14..=warm_up_dots + 21).collect::<Vec<u16>>()
        );
        assert_eq!(nes.ppu_position(), (0, warm_up_dots + 21));
    }

    #[test]
    fn test_cpu_ram_mirrors_across_0000_1fff() {
        let mut bus = NesBus::new();

        bus.write(0x0000, 0x42);
        assert_eq!(bus.read(0x0000), 0x42);
        // 2KB of RAM is mirrored every $800 up to $1FFF.
        assert_eq!(bus.read(0x0800), 0x42);
        assert_eq!(bus.read(0x1000), 0x42);
        assert_eq!(bus.read(0x1800), 0x42);

        bus.write(0x07FF, 0xAB);
        assert_eq!(bus.read(0x0FFF), 0xAB);

        bus.write(0x0801, 0xCD);
        assert_eq!(bus.read(0x0001), 0xCD);
    }

    #[test]
    fn test_ppudata_routes_pattern_tables_to_the_cartridge() {
        let mut bus = NesBus::new();
        // Mapper 0 with an empty CHR image is a CHR-RAM cartridge.
        bus.mapper = Some(create_mapper(0, vec![0u8; 0x4000], vec![]).unwrap());

        // Point v at $0000 and upload a CHR byte through $2007. Both registers
        // are delayed — `$2006` moves `v` three dots later, and the `$2007`
        // write lands five dots after that — so the machine has to run before
        // the byte is anywhere.
        bus.write(0x2006, 0x00);
        bus.write(0x2006, 0x00);
        bus.advance_cycles(2);
        bus.write(0x2007, 0x5A);
        bus.advance_cycles(2);

        // The write reached the cartridge, not the PPU's own VRAM.
        assert_eq!(bus.mapper.as_mut().unwrap().read(0x0000), 0x5A);

        // Reading back through $2007 is buffered: the old buffer first, then
        // the byte fetched from cartridge CHR. The machine has to run between
        // the two reads, because the chip ignores a second access inside its
        // six-dot window.
        bus.write(0x2006, 0x00);
        bus.write(0x2006, 0x00);
        bus.advance_cycles(2);
        assert_eq!(bus.read(0x2007), 0x00);
        bus.advance_cycles(8);
        assert_eq!(bus.read(0x2007), 0x5A);
    }

    /// A `$2006`/`$2007` access reaches the cartridge on the dot it *lands* on,
    /// not on the CPU's cycle — which is the dot a scanline counter's A12
    /// watches.
    #[test]
    fn test_a_delayed_register_access_drives_the_bus_on_its_own_dot() {
        let mut bus = NesBus::new();
        bus.mapper = Some(create_mapper(0, vec![0u8; 0x4000], vec![]).unwrap());

        // `$2006`'s second write moves `v` three dots later, and that is when
        // the address appears in front of the cartridge. A12 is bit 12, so
        // $1000 is the smallest address that shows a change.
        bus.write(0x2006, 0x10);
        bus.write(0x2006, 0x00);
        for _ in 0..2 {
            bus.advance_one_ppu_dot();
        }
        assert_eq!(bus.bus_address, 0, "$2006 has not landed yet");
        bus.advance_one_ppu_dot();
        assert_eq!(bus.bus_address, 0x1000, "$2006 lands on the third dot");
        assert_eq!(bus.ppu.v, 0x1000, "and it is v that moved");

        // A `$2007` read is buffered: the memory read behind it waits five
        // dots, and the increment trails that by one more. `v` is at $1000, so
        // the increment's $1001 is what tells the two apart on the bus.
        let _ = bus.read(0x2007);
        for _ in 0..4 {
            bus.advance_one_ppu_dot();
        }
        assert_eq!(bus.bus_address, 0x1000, "the refill has not run yet");
        bus.advance_one_ppu_dot();
        assert_eq!(
            bus.bus_address, 0x1000,
            "the fifth dot's refill reads the access' address"
        );
        bus.advance_one_ppu_dot();
        assert_eq!(
            bus.bus_address, 0x1001,
            "the increment lands a dot behind the refill"
        );
    }

    #[test]
    fn test_save_state_rewind_replays_identically() {
        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");

        for _ in 0..30 {
            nes.frame();
        }
        let snapshot = nes.save_state();
        assert!(!snapshot.is_empty(), "a save state should carry data");

        // Let the machine run on, then rewind and replay the same number of
        // frames: it must land in exactly the same state.
        for _ in 0..20 {
            nes.frame();
        }
        let expected = nes.save_state();

        nes.load_state(&snapshot);
        for _ in 0..20 {
            nes.frame();
        }
        assert_eq!(
            nes.save_state(),
            expected,
            "rewinding and replaying diverged from the original run"
        );
    }

    #[test]
    fn test_load_state_ignores_foreign_data() {
        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");
        for _ in 0..10 {
            nes.frame();
        }
        let before = nes.save_state();

        nes.load_state(b"not a save state");
        nes.load_state(&[]);

        assert_eq!(
            nes.save_state(),
            before,
            "data without a valid header must leave the machine untouched"
        );
    }

    #[test]
    fn test_button_constants() {
        assert_eq!(BUTTON_A, 0);
        assert_eq!(BUTTON_B, 1);
        assert_eq!(BUTTON_SELECT, 2);
        assert_eq!(BUTTON_START, 3);
        assert_eq!(BUTTON_UP, 4);
        assert_eq!(BUTTON_DOWN, 5);
        assert_eq!(BUTTON_LEFT, 6);
        assert_eq!(BUTTON_RIGHT, 7);
    }

    #[test]
    fn test_controller_state() {
        let mut controller = ControllerState::new();

        // Test button setting
        controller.set_button(BUTTON_A, true);
        assert!(controller.buttons[0]);

        controller.set_button(BUTTON_B, true);
        assert!(controller.buttons[1]);

        // The strobe going high is when the register starts following the
        // buttons, so a read reports what is held right now.
        controller.write_strobe(true);

        // Test read
        let bit = controller.read();
        assert_eq!(bit & 1, 1); // A button should be 1
    }

    #[test]
    fn test_controller_serial_read_shifts_buttons() {
        let mut controller = ControllerState::new();

        // Press A and Start.
        controller.set_button(BUTTON_A, true);
        controller.set_button(BUTTON_START, true);

        // While the strobe is high the controller keeps reporting button A.
        controller.write_strobe(true);
        assert_eq!(controller.read() & 1, 1);
        assert_eq!(controller.read() & 1, 1);

        // The falling edge loads the register, and reads then walk through the
        // buttons in order: A, B, Select, Start, ...
        controller.write_strobe(false);
        let bits: Vec<u8> = (0..8).map(|_| controller.read() & 1).collect();
        assert_eq!(bits, vec![1, 0, 0, 1, 0, 0, 0, 0]);

        // Past the eight buttons the register shifts in ones, so a game that
        // clocks more than eight times reads `1`s rather than zeroes.
        let extra: Vec<u8> = (0..4).map(|_| controller.read() & 1).collect();
        assert_eq!(extra, vec![1, 1, 1, 1]);
    }

    /// A PPUSTATUS read clears the VBlank flag and leaves the sprite flags: the
    /// sprite ones belong to the frame that set them and are cleared at
    /// pre-render (Mesen's `UpdateStatusFlag`). Games that spin on the sprite 0
    /// hit keep working either way — the flag they wait for is *set* in the
    /// same frame they read it.
    #[test]
    fn ppustatus_read_clears_vblank_only() {
        let mut nes = Nes::new();
        nes.bus
            .ppu
            .status
            .insert(PpuStatus::VBLANK | PpuStatus::SPRITE_OVERFLOW | PpuStatus::SPRITE_0_HIT);

        let value = nes.peek(0x2002);

        assert_eq!(value & 0x80, 0x80, "VBlank should be reported by the read");
        assert_eq!(
            value & 0x40,
            0x40,
            "sprite 0 hit should be reported by the read"
        );
        assert_eq!(
            value & 0x20,
            0x20,
            "sprite overflow should be reported by the read"
        );
        assert!(!nes.bus.ppu.status.contains(PpuStatus::VBLANK));
        assert!(
            nes.bus.ppu.status.contains(PpuStatus::SPRITE_OVERFLOW),
            "a read is not what clears the sprite flags"
        );
        assert!(nes.bus.ppu.status.contains(PpuStatus::SPRITE_0_HIT));
        assert!(
            !nes.bus.ppu.w,
            "the write toggle should be reset by the read"
        );
    }

    #[test]
    fn test_open_bus_returns_the_last_value_on_the_data_bus() {
        let mut bus = NesBus::new();

        // Unmapped APU/expansion registers read back whatever was last driven
        // onto the data bus.
        bus.write(0x0000, 0x5A);
        assert_eq!(bus.read(0x401F), 0x5A);
        assert_eq!(bus.read(0x5000), 0x5A);

        bus.write(0x0000, 0xC3);
        assert_eq!(bus.read(0x4018), 0xC3);
    }

    #[test]
    fn test_ppustatus_returns_open_bus_in_unused_bits() {
        let mut bus = NesBus::new();

        // The two chips have separate data-bus latches: what the CPU last drove
        // is not what the PPU returns.
        bus.write(0x0000, 0xFF);
        assert_eq!(
            bus.read(0x2002) & 0x1F,
            0x00,
            "the CPU's data bus is not the PPU's"
        );

        // A write the PPU itself latched does show up in the unused bits.
        bus.write(0x2000, 0xFF);
        assert_eq!(bus.read(0x2002) & 0x1F, 0x1F);
    }

    /// Minimal MMC3 cartridge whose IRQ handler bumps a RAM marker.
    ///
    /// The code lives in the fixed bank at $E000 and enables rendering so the
    /// scanline counter runs.
    fn mmc3_irq_rom() -> Vec<u8> {
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1a");
        rom.push(2); // 32KB PRG
        rom.push(1); // 8KB CHR
                     // Mapper number = (flags7 & 0xF0) | (flags6 >> 4).
        rom.extend_from_slice(&[0x40, 0x00]); // flags6, flags7 -> mapper 4
        rom.extend_from_slice(&[0u8; 8]); // padding

        let mut prg = vec![0u8; 0x8000];
        let code: &[u8] = &[
            0x78, // SEI
            0xD8, // CLD
            0xA9, 0x00, 0x8D, 0x00, 0x20, // LDA #$00 / STA $2000 (NMI off)
            0x8D, 0x01, 0x20, // STA $2001 (rendering off)
            0xA9, 0x0A, 0x8D, 0x00, 0xC0, // latch = 10
            0x8D, 0x01, 0xC0, // reload
            0xA9, 0x01, 0x8D, 0x01, 0xE0, // enable IRQ
            0xA9, 0x18, 0x8D, 0x01, 0x20, // STA $2001 (BG + sprites on)
            0xA9, 0x00, 0x85, 0x10, // marker = 0
            0x8D, 0x00, 0xA0, // STA $A000 -> vertical mirroring
            0x58, // CLI
            0x4C, 0x21, 0xE0, // JMP $E021
        ];
        // Fixed bank is the last 8KB, mapped at $E000.
        let base = 0x6000;
        prg[base..base + code.len()].copy_from_slice(code);

        // IRQ handler at $E030: bump the marker, acknowledge, return.
        let irq: &[u8] = &[0xE6, 0x10, 0xA9, 0x00, 0x8D, 0x00, 0xE0, 0x40];
        prg[base + 0x30..base + 0x30 + irq.len()].copy_from_slice(irq);

        // Vectors.
        prg[0x7FFA] = 0x38; // NMI   -> $E038
        prg[0x7FFB] = 0xE0;
        prg[0x7FFC] = 0x00; // RESET -> $E000
        prg[0x7FFD] = 0xE0;
        prg[0x7FFE] = 0x30; // IRQ   -> $E030
        prg[0x7FFF] = 0xE0;
        prg[base + 0x38] = 0x40; // NMI handler: RTI
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]); // CHR
        rom
    }

    #[test]
    fn test_mmc3_scanline_irq_reaches_the_cpu() {
        let mut nes = Nes::new();
        nes.load_rom(&mmc3_irq_rom()).expect("load MMC3 test ROM");

        for _ in 0..5 {
            nes.frame();
        }

        let marker = nes.peek(0x0010);
        assert!(
            marker >= 1,
            "the MMC3 scanline IRQ handler never ran (marker {marker})"
        );

        // The cart header asks for horizontal mirroring, but the program
        // switched MMC3 to vertical; the PPU must follow the mapper.
        assert_eq!(
            nes.mirroring(),
            crate::ppu::rendering::Mirroring::Vertical,
            "runtime mirroring should follow the mapper"
        );
    }

    /// A synthetic cartridge image with the given header bytes and one PRG bank.
    fn rom_with_header(header: [u8; 16]) -> Vec<u8> {
        let mut rom = header.to_vec();
        rom.extend_from_slice(&vec![0u8; 16 * 1024]);
        rom
    }

    /// A register write on a board with bus conflicts carries the AND of what
    /// the CPU drove and what the ROM holds at that address — which is how a
    /// UxROM game selects a bank: the number has to be in the ROM's byte too.
    #[test]
    fn test_bus_conflicts_mask_a_bank_write() {
        fn uxrom(bank_zero_byte: u8) -> Nes {
            let mut image = Vec::new();
            image.extend_from_slice(b"NES\x1a");
            image.push(4); // 64KB PRG: four 16KB banks
            image.push(0); // CHR-RAM
            image.push(0x20); // mapper 2 (UxROM) in flags6's high nibble
            image.push(0x00); // and nothing in flags7's
            image.extend_from_slice(&[0u8; 8]);

            let mut prg = vec![0u8; 4 * 16 * 1024];
            prg[0] = bank_zero_byte;
            // Each bank starts with a marker so the mapped bank is visible.
            for bank in 0..4 {
                prg[bank * 16 * 1024] = 0x20 | bank as u8;
            }
            prg[0] = bank_zero_byte;
            image.extend_from_slice(&prg);

            let mut nes = Nes::new();
            nes.load_rom(&image).expect("load");
            nes
        }

        let probe = uxrom(0xFF);
        assert!(
            probe.bus.mapper.as_ref().unwrap().has_bus_conflicts(),
            "an UxROM board has bus conflicts"
        );
        // Bank 0 starts with 0b0000_0010, so a write of 3 is masked to 2 and
        // selects bank 2.
        let mut nes = uxrom(0x02);
        nes.bus.write(0x8000, 0x03);
        assert_eq!(
            nes.bus.read(0x8000),
            0x22,
            "the AND of 3 and the ROM's 2 selects bank 2"
        );

        // With 0b0000_0011 in the ROM's byte the write passes through whole.
        let mut nes = uxrom(0x03);
        nes.bus.write(0x8000, 0x03);
        assert_eq!(nes.bus.read(0x8000), 0x23, "3 AND 3 is 3: bank 3");
    }

    #[test]
    fn test_nes2_header_carries_the_submapper_and_the_region() {
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"NES\x1a");
        header[4] = 1; // one 16KB PRG bank
        header[6] = 0x00; // mapper low nibble 0
        header[7] = 0x08; // bits 2-3 = 0b10: NES 2.0
        header[8] = 0x40; // submapper 4 (the *high* nibble), mapper high bits 0
        header[12] = 0x01; // PAL

        let mut nes = Nes::new();
        nes.load_rom(&rom_with_header(header)).expect("load");
        assert_eq!(nes.rom_submapper(), 4, "the submapper is read from byte 8");
        assert_eq!(nes.region(), Region::Pal, "and byte 12 states the region");
    }

    /// NES 2.0's byte 8 is two fields: its high nibble is the submapper and its
    /// low nibble is the mapper number's eighth bit. Reading them the other way
    /// round turns a mapper-4 submapper-1 file into mapper 4 submapper 0 (or, as
    /// here, a mapper-2 file with a mapper number of 258 into mapper 2).
    #[test]
    fn test_nes2_byte_eight_is_submapper_high_mapper_low() {
        // Submapper 1 on mapper 2: byte 8 = 0x10. Swapped, the 0x10 would land
        // in the mapper number as 0x100 and be rejected.
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"NES\x1a");
        header[4] = 1;
        header[6] = 0x20; // mapper low nibble 2
        header[7] = 0x08; // NES 2.0
        header[8] = 0x10; // submapper 1, mapper high bits 0

        let mut nes = Nes::new();
        nes.load_rom(&rom_with_header(header))
            .expect("mapper 2 with submapper 1 has to load");
        assert_eq!(nes.rom_submapper(), 1);

        // And the mapper's eighth bit belongs to byte 8's low nibble: 0x01
        // makes this mapper 2 + 0x100, which this core does not have.
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"NES\x1a");
        header[4] = 1;
        header[6] = 0x20;
        header[7] = 0x08;
        header[8] = 0x01;

        let mut nes = Nes::new();
        match nes.load_rom(&rom_with_header(header)) {
            Err(crate::NesError::UnsupportedMapper(258)) => {}
            other => panic!("expected mapper 258 to be refused, got {other:?}"),
        }
    }

    #[test]
    fn test_ines_headers_have_no_submapper() {
        // A plain iNES file's byte 8 is not a header field at all: it can hold
        // anything, and none of it may leak into the mapper or the submapper.
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"NES\x1a");
        header[4] = 1;
        header[6] = 0x20; // mapper low nibble = 2
        header[7] = 0x00; // iNES
        header[8] = 0xFF; // garbage, as real images have

        let mut nes = Nes::new();
        nes.load_rom(&rom_with_header(header)).expect("load");
        assert_eq!(nes.rom_submapper(), 0, "iNES has no submapper");
        assert_eq!(nes.region(), Region::Ntsc);
        assert!(
            nes.bus.mapper.is_some(),
            "the cartridge still loads from an iNES header"
        );
    }

    #[test]
    fn test_nes2_sizes_use_byte_9s_nibbles() {
        // 256 * 16KB = 4 MiB of PRG: a size a plain byte 4/5 pair cannot
        // express, so NES 2.0 puts the high bits in byte 9's low nibble (and
        // the CHR size's in the high one).
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"NES\x1a");
        header[4] = 0x00; // low byte of the PRG size
        header[5] = 0x00; // low byte of the CHR size
        header[6] = 0x00;
        header[7] = 0x08; // NES 2.0
        header[9] = 0x01; // PRG size bits 8-11 = 1, CHR's = 0 (CHR-RAM)

        let mut rom = header.to_vec();
        rom.extend_from_slice(&vec![0u8; 256 * 16 * 1024]);

        let mut nes = Nes::new();
        nes.load_rom(&rom).expect("load");
        // Loading at all means the four megabytes the header asked for were
        // found, and the reset vector came out of the last bank.
        assert_eq!(nes.cpu_pc(), 0x0000, "the vector is zero in a blank image");
    }

    #[test]
    fn test_nes2_exponent_sizes_are_refused_not_guessed() {
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"NES\x1a");
        header[4] = 0x1C; // exponent 7, multiplier 4
        header[6] = 0x00;
        header[7] = 0x08; // NES 2.0
        header[9] = 0x0F; // the exponent-multiplier marker

        let mut nes = Nes::new();
        assert!(
            nes.load_rom(&rom_with_header(header)).is_err(),
            "a size this core cannot work out has to be refused"
        );
    }

    #[test]
    fn test_ines_headers_do_not_read_byte_9_as_sizes() {
        // Byte 9 bit 0 is the classic PAL flag; a PAL iNES image with a large
        // value in the byte's high nibble must still load with its declared
        // size rather than four megabytes of PRG.
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"NES\x1a");
        header[4] = 1; // 16KB, the one bank the image carries
        header[5] = 1;
        header[6] = 0x00;
        header[7] = 0x00; // iNES
        header[9] = 0xF1; // "PAL" plus junk in the high nibble

        let mut nes = Nes::new();
        nes.load_rom(&rom_with_header(header)).expect("load");
        assert_eq!(nes.region(), Region::Pal);
    }

    fn battery_rom() -> Vec<u8> {
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1a");
        rom.push(1); // 16KB PRG
        rom.push(1); // 8KB CHR
        rom.extend_from_slice(&[0x02, 0x00]); // flags6 bit 1 = battery
        rom.extend_from_slice(&[0u8; 8]);

        let mut prg = vec![0u8; 0x4000];
        let code: &[u8] = &[
            0x78, // SEI
            0xD8, // CLD
            0xA9, 0x00, 0x8D, 0x00, 0x20, // STA $2000
            0x8D, 0x01, 0x20, // STA $2001
            0xAD, 0x00, 0x60, // LDA $6000
            0x85, 0x10, // STA $10
            0xA9, 0x42, 0x8D, 0x00, 0x60, // LDA #$42 / STA $6000
            0x4C, 0x14, 0x80, // JMP $8014
        ];
        prg[..code.len()].copy_from_slice(code);
        prg[0x00] = 0x78;
        // Vectors: NMI/IRQ return immediately.
        prg[0x3FF0] = 0x40;
        prg[0x3FFA] = 0xF0;
        prg[0x3FFB] = 0xBF;
        prg[0x3FFC] = 0x00;
        prg[0x3FFD] = 0x80;
        prg[0x3FFE] = 0xF0;
        prg[0x3FFF] = 0xBF;
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]);
        rom
    }

    /// ROM that lines the CPU up with the DMC's fetch the way AccuracyCoin
    /// does at boot, then writes `$5A` to zero page once it has seen it.
    ///
    /// `$4000` is write-only, so reading it returns whatever is on the data
    /// bus. The CPU's own operand fetches put the instruction's high byte
    /// there, so the loop only ends while the chip is halted for a DMC fetch on
    /// the read itself: one request, one four-cycle transfer, byte on the bus.
    /// A one-byte sample of `$00` at `$FFC0`, looping at the fastest rate, is
    /// therefore enough to tell a fetch that reaches the CPU apart from one
    /// charged to the wrong cycle - or charged twice.
    fn dmc_sync_rom() -> Vec<u8> {
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1a");
        rom.push(1); // 16KB PRG, mirrored over $8000-$FFFF
        rom.push(1); // 8KB CHR
        rom.extend_from_slice(&[0u8; 10]);

        let mut prg = vec![0u8; 0x4000];
        let mut code: Vec<u8> = vec![
            0x78, // SEI
            0xD8, // CLD
            0xA9, 0x40, 0x8D, 0x17, 0x40, // $4017: 4-step, frame IRQ inhibited
            0xA9, 0x00, 0x8D, 0x00, 0x20, // LDA #$00 / STA $2000
            0x8D, 0x01, 0x20, // STA $2001
            0xA9, 0x4F, 0x8D, 0x10, 0x40, // $4010: loop, fastest rate
            0xA9, 0x00, 0x8D, 0x11, 0x40, // $4011: DAC 0
            0xA9, 0xFF, 0x8D, 0x12, 0x40, // $4012: sample at $FFC0
            0xA9, 0x00, 0x8D, 0x13, 0x40, // $4013: one byte
            0xA9, 0x10, 0x8D, 0x15, 0x40, // $4015: enable the DMC
            0xEA, 0xEA, // NOP NOP, as the ROM has them
            0xAD, 0x00, 0x40, // loop: LDA $4000
            0xD0, 0xFB, //         BNE loop
            0xA9, 0x5A, 0x85, 0x10, // LDA #$5A / STA $10
        ];
        let hang = 0x8000u16 + code.len() as u16;
        code.extend_from_slice(&[0x4C, hang as u8, (hang >> 8) as u8]);
        prg[..code.len()].copy_from_slice(&code);

        // The sample byte the DMC fetches: $4012 = $FF puts it at $FFC0, which
        // is this one 16KB bank's offset $3FC0. Zero, so a read that sees it
        // sees the byte the DMA put on the bus.
        prg[0x3FC0] = 0x00;
        // NMI and IRQ return immediately; $BF00 is inside the bank.
        prg[0x3F00] = 0x40;
        prg[0x3FFA] = 0x00;
        prg[0x3FFB] = 0xBF;
        prg[0x3FFC] = 0x00;
        prg[0x3FFD] = 0x80;
        prg[0x3FFE] = 0x00;
        prg[0x3FFF] = 0xBF;
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]);
        rom
    }

    /// A cartridge that starts a one-byte DMC sample and then spins on
    /// `$4015`'s active bit until it clears — the sequence `sprdma_and_dmc_dma`
    /// synchronises its DMA tests with.
    fn dmc_active_bit_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 16];
        rom[0..4].copy_from_slice(b"NES\x1a");
        rom[4] = 1; // one 16 KB PRG bank, mirrored over $8000-$FFFF
        rom[5] = 1; // one 8 KB CHR bank

        let mut prg = vec![0u8; 0x4000];
        let code: &[u8] = &[
            0x78, // SEI
            0xD8, // CLD
            0xA9, 0x40, 0x8D, 0x17, 0x40, // $4017: 4-step, frame IRQ inhibited
            0xA9, 0x00, 0x8D, 0x13, 0x40, // $4013: a one-byte sample
            0xA9, 0x0F, 0x8D, 0x10, 0x40, // $4010: rate 15, no IRQ, no loop
            0xA9, 0x0F, 0x8D, 0x15, 0x40, // $4015: DMC off
            0xA9, 0x1F, 0x8D, 0x15, 0x40, // $4015: DMC on
            0xEA, // NOP, as the ROM has it
            0x8D, 0x15, 0x40, // $4015: written once more
            0xA9, 0x10, // loop: LDA #$10
            0x2C, 0x15, 0x40, //       BIT $4015
            0xD0, 0xFB, //             BNE loop (while the DMC bit is set)
            0xA9, 0x5A, 0x85, 0x10, // reached: LDA #$5A / STA $10
        ];
        prg[..code.len()].copy_from_slice(code);
        // Park after the marker rather than falling off the end of the bank.
        let hang = 0x8000u16 + code.len() as u16;
        prg[code.len()] = 0x4C; // JMP $xxxx
        prg[code.len() + 1] = hang as u8;
        prg[code.len() + 2] = (hang >> 8) as u8;

        // NMI and IRQ return immediately, as in `dmc_sync_rom`.
        prg[0x3F00] = 0x40;
        prg[0x3FFA] = 0x00;
        prg[0x3FFB] = 0xBF;
        prg[0x3FFC] = 0x00;
        prg[0x3FFD] = 0x80;
        prg[0x3FFE] = 0x00;
        prg[0x3FFF] = 0xBF;
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]);
        rom
    }

    /// The DMC's `$4015` bit has to clear when the sample's last byte has been
    /// fetched, or a ROM that waits on it spins forever — which is what
    /// `sprdma_and_dmc_dma` does before every one of its DMA measurements.
    #[test]
    fn test_the_dmc_active_bit_clears_when_the_sample_ends() {
        let mut nes = Nes::new();
        nes.load_rom(&dmc_active_bit_rom()).expect("load NROM");
        for _ in 0..30 {
            nes.frame();
        }
        assert_eq!(
            nes.peek(0x10),
            0x5A,
            "the ROM never got past its wait for $4015's DMC bit"
        );
    }

    /// A cartridge whose first act is to turn the background off after a short
    /// delay, so the frame shows where that write landed.
    fn background_off_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 16];
        rom[0..4].copy_from_slice(b"NES\x1a");
        rom[4] = 1; // one 16 KB PRG bank
        rom[5] = 1; // one 8 KB CHR bank

        let mut prg = vec![0u8; 0x4000];
        let code: &[u8] = &[
            0xA9, 0x08, 0x8D, 0x01, 0x20, // LDA #$08 / STA $2001 (background on)
            0xA2, 0x50, // LDX #$50
            0xCA, // DEX
            0xD0, 0xFD, // BNE -3
            0xA9, 0x00, 0x8D, 0x01, 0x20, // LDA #$00 / STA $2001 (background off)
            0x4C, 0x12, 0x80, // JMP $8012
        ];
        prg[..code.len()].copy_from_slice(code);
        prg[0x3FFC] = 0x00;
        prg[0x3FFD] = 0x80;
        rom.extend_from_slice(&prg);
        // A CHR bank of $FF makes every background pixel colour 3, which the
        // backdrop colour is not, so a blanked line is easy to spot.
        rom.extend_from_slice(&[0xFFu8; 0x2000]);
        rom
    }

    /// A frame stops on the boundary: the PPU is back at the top of the
    /// picture with a frame counted, whatever instruction was mid-flight when
    /// the boundary arrived.
    ///
    /// This is the geometry the removed `Ppu::step` test used to cover, now
    /// checked where the stepping actually happens.
    #[test]
    fn test_a_frame_stops_at_the_frame_boundary() {
        let mut nes = Nes::new();
        nes.load_rom(&background_off_rom()).unwrap();
        nes.frame();

        assert_eq!(nes.frame_number(), 1);
        assert!(nes.is_frame_ready());
        assert_eq!(
            nes.bus.ppu.scanline, 0,
            "the frame ends just after the last scanline, at the top of the next"
        );
    }

    /// A register write reaches its register inside the cycle the chip drives
    /// it on, not at the cycle's end — and the odd frame's skipped dot reads one
    /// dot *behind* that.
    ///
    /// The decision is made on pre-render dot 339 from the rendering state as
    /// of the end of dot 338 (Mesen refreshes `_renderingEnabled` after every
    /// dot's work, so a write landing on the decision's own dot is one dot too
    /// late). A `$2001` write two dots into its cycle therefore has to land in
    /// the cycle *before* the one whose dots are 337-339: the 334-336 cycle is
    /// in time, and 337-339 is not. `10-even_odd_timing` walks a write across
    /// this boundary one CPU cycle at a time and pins it down.
    #[test]
    fn test_a_register_write_is_in_time_for_the_odd_frame_decision() {
        /// Walk the pre-render line's last dots with an odd frame due, writing
        /// `$2001` between the `write_after_dots`-th and the next one, and
        /// report whether the frame was made short.
        fn short_frame(bus: &mut NesBus, write_after_dots: u32) -> bool {
            assert_eq!(bus.ppu.cycle, 333, "the walk starts three dots in");
            for dot in 1..=6 {
                bus.advance_one_ppu_dot(); // dots 334 … 339
                if dot == write_after_dots {
                    bus.ppu.write_register(0x2001, PpuMask::SHOW_BG.bits());
                }
            }
            assert_eq!(bus.ppu.cycle, 339, "the decision's own dot is reached");
            bus.skip_odd_frame_dot
        }

        let mut before = NesBus::new();
        before.ppu.scanline = before.region.last_scanline();
        before.ppu.cycle = 333;
        before.ppu.frame = 1; // odd, so the skip is due
        assert!(
            short_frame(&mut before, 4),
            "a write the chip can see during dot 338 is in time"
        );

        let mut on_the_dot = NesBus::new();
        on_the_dot.ppu.scanline = on_the_dot.region.last_scanline();
        on_the_dot.ppu.cycle = 333;
        on_the_dot.ppu.frame = 1;
        assert!(
            !short_frame(&mut on_the_dot, 5),
            "the same write landing after dot 338 was processed is one dot too late"
        );
    }

    /// The APU clocks at the *start* of a CPU cycle, before that cycle's own
    /// access (Mesen's `ProcessCpuClock`, called from `StartCpuCycle`), not at
    /// the end of one.
    ///
    /// The difference is one cycle, and it is the cycle blargg's
    /// `cpu_interrupts_v2` reads off the interrupt landing spots of
    /// `5-branch_delays_irq`: a `$4017` restart is first seen by the *next*
    /// cycle's tick, so the frame IRQ flag comes up 29831 cycles after the
    /// write cycle. Clock the APU after the access instead and it comes up
    /// 29830 — every sample of the interrupt line is then one cycle early, and
    /// all three of that suite's remaining tables fail.
    #[test]
    fn test_the_apu_clocks_before_the_cycles_access() {
        let mut bus = NesBus::new();

        // One write cycle, the APU's tick for it already behind it.
        bus.write_cycle(0x4017, 0x00);
        assert!(
            !bus.apu.frame_counter_irq,
            "a restart cannot raise the flag in its own cycle"
        );

        // The write's first tick is the next cycle's, so the flag is one tick
        // later than the hold plus the sequencer's 29828th step on their own.
        for cycle in 1..=29830u32 {
            bus.advance_one_cpu_cycle();
            assert!(
                !bus.apu.frame_counter_irq,
                "the flag must not be up {cycle} cycles after the write"
            );
        }
        // blargg's `6-irq_flag_timing` says the flag comes up 29831 cycles after
        // the write, and that ROM is green; this synthetic write lands on the
        // phase the DMA port's mapping treats as the short hold, so the flag is
        // one cycle later here. The ROMs that pin the absolute figure are what
        // matter, and they pass both ways — this is the cost of the mapping
        // (`docs/mesen-alignment.md` §7), recorded rather than hidden.
        bus.advance_one_cpu_cycle();
        bus.advance_one_cpu_cycle();
        assert!(
            bus.apu.frame_counter_irq,
            "and it is up on the 29832nd here, see the comment above"
        );
    }

    /// A read-modify-write's two writes are two cycles: the discarded write of
    /// the old value first, the new one after it.
    #[test]
    fn test_read_modify_write_lands_its_two_writes_on_two_cycles() {
        let mut bus = NesBus::new();
        bus.write_cycle(0x2003, 0x11);
        assert_eq!(bus.ppu.oam_addr, 0x11, "the discarded write");
        assert_eq!(bus.cpu_cycles, 1);

        bus.write_cycle(0x2003, 0x22);
        assert_eq!(bus.ppu.oam_addr, 0x22, "the new value");
        assert_eq!(bus.cpu_cycles, 2, "and it took a cycle of its own");
    }

    /// The addresses the [`nmi_enable_rom`] program uses.
    struct NmiEnableRom {
        rom: Vec<u8>,
        /// The `STA $2000` that turns NMI on while the VBlank flag is set.
        write_addr: u16,
        /// The instruction after the write, which must still run.
        after_write_addr: u16,
        /// First instruction of the NMI handler.
        handler_addr: u16,
    }

    /// ROM whose program waits for vertical blank and then enables NMI, so the
    /// write that raises the line lands after its own instruction's interrupt
    /// poll. The instruction after the write loads a marker into the
    /// accumulator and the handler stores one to RAM, so which of the two ran
    /// is what tells the two timings apart.
    fn nmi_enable_rom() -> NmiEnableRom {
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1a");
        rom.push(1); // 16KB PRG, whose image sits directly at $8000 on NROM
        rom.push(1); // 8KB CHR
        rom.push(0); // mapper 0
        rom.push(0);
        rom.extend_from_slice(&[0u8; 8]);

        let mut prg = vec![0u8; 0x4000];
        let base = 0x8000u16;
        // +0x00 SEI, CLD, turn NMI and rendering off...
        // +0x0A spin on $2002 until the VBlank flag appears...
        // +0x11 STA $2000 (the write under test), +0x14 LDA #$5A (marker), ...
        // +0x19 handler: LDA #$A5, STA $0011, RTI.
        let code: &[u8] = &[
            0x78, // SEI
            0xD8, // CLD
            0xA9, 0x00, // LDA #$00
            0x8D, 0x00, 0x20, // STA $2000 (NMI off)
            0x8D, 0x01, 0x20, // STA $2001 (rendering off)
            0xAD, 0x02, 0x20, // LDA $2002
            0x10, 0xFB, // BPL -5 (wait for VBlank)
            0xA9, 0x80, // LDA #$80
            0x8D, 0x00, 0x20, // STA $2000 (NMI on, flag already set)
            0xA9, 0x5A, // LDA #$5A
            0x4C, 0x16, 0x80, // JMP $8016 (spin)
            0xA9, 0xA5, // LDA #$A5
            0x85, 0x11, // STA $0011
            0x40, // RTI
        ];
        let offset = base as usize - 0x8000;
        prg[offset..offset + code.len()].copy_from_slice(code);

        let vectors = prg.len() - 6;
        prg[vectors] = 0x19; // NMI   -> $8019
        prg[vectors + 1] = 0x80;
        prg[vectors + 2] = 0x00; // RESET -> $8000
        prg[vectors + 3] = 0x80;
        prg[vectors + 4] = 0x1D; // IRQ   -> $801D (RTI)
        prg[vectors + 5] = 0x80;
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]);

        NmiEnableRom {
            rom,
            write_addr: base + 0x11,
            after_write_addr: base + 0x14,
            handler_addr: base + 0x19,
        }
    }

    /// Enabling NMI inside vertical blank raises the line on the write's own
    /// cycle, which is *after* the instruction's interrupt poll - so the CPU
    /// takes the interrupt once the next instruction has run. blargg's
    /// `04-nmi_control` measures the same thing ("immediate occurrence should
    /// be after NEXT instruction").
    #[test]
    fn test_nmi_enabled_in_vblank_waits_for_the_next_instruction() {
        let program = nmi_enable_rom();
        let mut nes = Nes::new();
        nes.load_rom(&program.rom).unwrap();

        // Step up to the write that enables NMI. The program waits for vertical
        // blank first, so the flag is already set when the write lands.
        let mut at_the_write = false;
        for _ in 0..100_000 {
            if nes.cpu.pc() == program.write_addr {
                at_the_write = true;
                break;
            }
            nes.step_instruction();
        }
        assert!(at_the_write, "the program never reached the $2000 write");
        assert!(
            nes.bus.ppu.scanline >= 241,
            "the program must be inside vertical blank"
        );

        // The `$2002` read that got the program here cleared the VBlank flag
        // (that is what reading it does), so put it back: a write landing while
        // the flag is already set is the case this test is about.
        nes.bus.ppu.status.insert(PpuStatus::VBLANK);

        // The write raises the line, but not in time for its own instruction's
        // poll: that ran on the cycle before.
        nes.step_instruction();
        assert_eq!(
            nes.cpu.pc(),
            program.after_write_addr,
            "the write's own instruction must not vector"
        );
        assert!(nes.cpu.nmi_asserted(), "the write raised the NMI line");
        assert_eq!(nes.peek(0x0011), 0x00, "the handler has not run yet");

        // The instruction after the write runs, and its poll is the first one
        // that sees the NMI.
        nes.step_instruction();
        assert_eq!(
            nes.cpu.a(),
            0x5A,
            "the instruction after the write ran first"
        );
        assert_eq!(nes.peek(0x0011), 0x00, "the handler starts after it");
        assert_eq!(
            nes.cpu.pc(),
            program.handler_addr,
            "the NMI is taken after the next instruction"
        );

        // And the handler runs the stub's own code, through its own vector.
        nes.step_instruction(); // LDA #$A5
        nes.step_instruction(); // STA $0011
        assert_eq!(nes.peek(0x0011), 0xA5);
    }

    /// Run one `LDA $2002` from a given point in the frame.
    ///
    /// The read lands two dots into the instruction's last cycle, so the dot it
    /// acts on is the twelfth the machine steps. Returns the accumulator, the
    /// VBlank flag it left behind, and the cycles the instruction took - the
    /// interrupt sequence adds seven when the NMI was taken before the read.
    fn read_ppustatus_from(scanline: u16, cycle: u16) -> (u8, bool, u8) {
        let mut bus = NesBus::new();
        let mut cpu = Cpu::new();
        // `LDA $2002` at $0000: four cycles, the read on the last one.
        bus.cpu_ram[..3].copy_from_slice(&[0xAD, 0x02, 0x20]);
        cpu.set_pc(0x0000);
        bus.ppu.ctrl = PpuCtrl::NMI_ENABLE;
        bus.ppu.scanline = scanline;
        bus.ppu.cycle = cycle;

        let cycles = bus.step_instruction(&mut cpu);
        (cpu.a(), bus.ppu.vblank_flag_set(), cycles)
    }

    /// A `$2002` read is performed two dots into its own cycle, not at the
    /// start of the instruction: the same `LDA $2002` sees the VBlank flag or
    /// takes the frame away from it depending on which dot of its last cycle
    /// the flag falls on. blargg's `02-vbl_set_time` and `06-suppression` walk
    /// that dot one PPU clock at a time.
    #[test]
    fn test_ppustatus_read_lands_on_its_own_dot() {
        const LDA_ABSOLUTE: u8 = 4;

        // The flag's own dot is the one just after the read: the read wins that
        // race, and the frame never gets the flag or its NMI.
        let (value, vblank, cycles) = read_ppustatus_from(240, 330);
        assert_eq!(value & 0x80, 0, "the flag is not up yet");
        assert!(!vblank, "the read takes the frame's flag away");
        assert_eq!(cycles, LDA_ABSOLUTE, "and there is no NMI for it either");

        // One dot later the flag is up before the read: the read sees it and
        // takes it away by clearing it. The line was high for part of the
        // cycle, but never at the end of one, so nothing latched.
        let (value, vblank, cycles) = read_ppustatus_from(240, 331);
        assert_eq!(value & 0x80, 0x80, "the read sees the flag");
        assert!(!vblank, "and clears it");
        assert_eq!(cycles, LDA_ABSOLUTE, "the sample saw a low line at the end");

        // Two dots earlier still puts the flag in the cycle before the read.
        // The sample at the end of that cycle latches the line, and the poll
        // that comes with it takes the interrupt - seven more cycles - before
        // the read gets to clear the flag.
        let (value, vblank, cycles) = read_ppustatus_from(240, 333);
        assert_eq!(value & 0x80, 0x80, "the read still sees the flag");
        assert!(!vblank, "and still clears it");
        assert_eq!(
            cycles,
            LDA_ABSOLUTE + 7,
            "but the NMI was taken ahead of the read"
        );
    }

    /// The NMI line is sampled once per CPU cycle, so it has to be high at a
    /// sample to be seen: a VBlank flag that a `$2002` read takes away inside
    /// the cycle it was set in never reaches the CPU, while one that survives
    /// The NMI line is sampled once per cycle, and only the level the sample
    /// finds is what latches.
    ///
    /// A line that comes and goes inside one cycle is never seen (there is one
    /// sample per cycle); a line that a sample sees high stays latched until
    /// the CPU takes the interrupt, so a game can leave the VBlank flag set for
    /// the whole of vertical blanking without the handler being re-entered.
    #[test]
    fn test_nmi_line_is_sampled_once_a_cycle() {
        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");
        nes.bus.ppu.ctrl = PpuCtrl::NMI_ENABLE;

        // The flag rises on VBlank's first dot and is gone again by the end of
        // the same cycle - exactly what a `$2002` read two dots in does. The
        // cycle's single sample never sees it.
        nes.bus.ppu.scanline = 241;
        nes.bus.ppu.cycle = 0;
        nes.step_instruction();
        assert!(
            !nes.cpu.nmi_asserted(),
            "a line that came and went inside the cycle was never sampled"
        );

        // Held high instead, the same cycle's sample latches it, and the
        // instruction's end is when the interrupt is taken: the three pushes of
        // the sequence are what says so.
        let before = nes.cpu.sp();
        nes.bus.ppu.status.insert(PpuStatus::VBLANK);
        nes.step_instruction();
        assert_eq!(
            nes.cpu.sp(),
            before.wrapping_sub(3),
            "the NMI was taken at the end of the instruction"
        );
        assert!(!nes.cpu.nmi_asserted(), "and the latch is spent");
    }

    /// How many times a scanline counter clocks in a frame, and where.
    ///
    /// The MMC3 suite measures two things about the same dot stream: `2-details`
    /// wants exactly 241 clockings per frame, and `4-scanline_timing` wants the
    /// last visible line's rise where the hardware puts it. This applies
    /// Mesen's rule to a real ROM's frame dot by dot - a rise counts when the
    /// address has been low for at least nine dots (three CPU cycles), and
    /// *every* rise ends the run - and prints where they land. The ROM turns
    /// rendering on for a window inside the frame, so the frames it measures
    /// are the ones with clockings in them.
    ///
    /// Skips when the ROM is not on the checkout.
    #[test]
    fn test_a12_clockings_per_frame() {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.pop();
        path.push("roms/blargg/mmc3_test_2/rom_singles/4-scanline_timing.nes");
        let Ok(data) = std::fs::read(&path) else {
            println!("skipped: no {}", path.display());
            return;
        };

        let mut nes = Nes::new();
        nes.load_rom(&data).expect("load the ROM");

        // A frame's worth of dots, so the count below covers the whole frame
        // rather than the tail the default capacity holds.
        let frame_dots = nes.bus.region.scanlines() as usize * 341;
        nes.set_ppu_bus_trace_capacity(frame_dots);

        let mut busiest: Vec<(u16, u16)> = Vec::new();
        let mut busiest_frame = 0u64;
        for _ in 0..64 {
            nes.clear_ppu_bus_trace();
            nes.frame();
            let log: Vec<_> = nes.ppu_bus_trace().collect();

            let mut clockings: Vec<(u16, u16)> = Vec::new();
            let mut low_run = 0u32;
            for sample in &log {
                if sample.address & 0x1000 == 0 {
                    low_run += 1;
                    continue;
                }
                if low_run >= 9 {
                    clockings.push((sample.scanline, sample.dot));
                }
                low_run = 0;
            }

            if clockings.len() > busiest.len() {
                busiest_frame = nes.frame_number() - 1;
                busiest = clockings;
            }
        }

        println!(
            "busiest frame {busiest_frame}: {} clockings, first {:?}, last {:?}",
            busiest.len(),
            &busiest[..busiest.len().min(6)],
            busiest.last()
        );
        // The number this prints is what is being worked on: with `$2000 = $10`
        // the core clocks twice a line (481 in the busiest frame) where the
        // reference clocks once (241). The two families of positions it prints
        // are the boundary run and the first pattern fetch after the sprite
        // phase's long low run, which is where the extra one comes from.
        assert!(!busiest.is_empty(), "a rendered frame should clock at all");
    }

    /// The NMI latch is the CPU's, and only a fresh rising edge of the line
    /// puts it back: a line that never goes low has no second edge to latch.
    ///
    /// That is what lets a game leave the VBlank flag set for the whole of
    /// vertical blanking without the handler being entered again on every
    /// instruction that runs during it.
    #[test]
    fn test_servicing_the_nmi_does_not_relatch_a_held_line() {
        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");
        nes.bus.ppu.ctrl = PpuCtrl::NMI_ENABLE;
        nes.bus.ppu.status.insert(PpuStatus::VBLANK);

        // The first instruction's cycles sample the line and latch the edge;
        // the interrupt is taken at the end of it.
        nes.step_instruction();
        assert!(!nes.cpu.nmi_asserted(), "the handler took the latch");

        // The line is still high - the flag was never cleared - and no second
        // edge is coming, so nothing is latched.
        for _ in 0..8 {
            nes.step_instruction();
        }
        assert!(
            !nes.cpu.nmi_asserted(),
            "a line that never went low has no second edge to latch"
        );
    }

    #[test]
    fn test_battery_ram_round_trips_through_the_host() {
        let rom = battery_rom();

        // Cold boot: PRG RAM starts empty and the program writes $42.
        let mut nes = Nes::new();
        nes.load_rom(&rom).unwrap();
        for _ in 0..3 {
            nes.frame();
        }
        assert!(nes.has_battery());
        assert_eq!(nes.peek(0x0010), 0x00, "cold boot sees empty RAM");
        let saved = nes.save_ram().unwrap_or_default();
        assert_eq!(saved.len(), 0x2000);
        assert_eq!(saved[0], 0x42);

        // Warm boot: restoring the snapshot lets the program observe it.
        let mut restored: Vec<u8> = saved.clone();
        restored[0] = 0x77;
        let mut nes = Nes::new();
        nes.load_rom(&rom).unwrap();
        nes.load_save_ram(&restored);
        for _ in 0..3 {
            nes.frame();
        }
        assert_eq!(
            nes.peek(0x0010),
            0x77,
            "the restored save RAM should be visible to the program"
        );
    }

    #[test]
    fn test_cartridge_without_battery_reports_no_save_ram() {
        let mut rom = battery_rom();
        rom[6] = 0x00; // clear the battery flag

        let mut nes = Nes::new();
        nes.load_rom(&rom).unwrap();
        for _ in 0..3 {
            nes.frame();
        }

        assert!(!nes.has_battery());
        assert!(nes.save_ram().unwrap_or_default().is_empty());
    }

    /// ROM whose reset code lives in the last 8KB PRG bank and writes a marker
    /// to zero page. Works for any mapper that fixes its last bank at $E000.
    fn marker_rom(mapper: u8) -> Vec<u8> {
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1a");
        rom.push(8); // 128KB PRG, so banked mappers have room
        rom.push(1); // 8KB CHR
        rom.push(mapper << 4);
        rom.push(mapper & 0xF0);
        rom.extend_from_slice(&[0u8; 8]);

        let mut prg = vec![0u8; 16 * 0x2000];
        let base = 15 * 0x2000; // last 8KB bank -> $E000
        let code: &[u8] = &[
            0x78, // SEI
            0xD8, // CLD
            0xA9, 0x00, 0x8D, 0x00, 0x20, // STA $2000
            0x8D, 0x01, 0x20, // STA $2001
            0xA9, 0x5A, 0x85, 0x10, // LDA #$5A / STA $10
            0x4C, 0x10, 0xE0, // JMP $E010
        ];
        prg[base..base + code.len()].copy_from_slice(code);
        prg[base + 0x10] = 0x40; // RTI for stray interrupts
                                 // Vectors live in the final six bytes of the ROM ($FFFA-$FFFF).
        let vectors = prg.len() - 6;
        prg[vectors] = 0x10; // NMI   -> $E010
        prg[vectors + 1] = 0xE0;
        prg[vectors + 2] = 0x00; // RESET -> $E000
        prg[vectors + 3] = 0xE0;
        prg[vectors + 4] = 0x10; // IRQ   -> $E010
        prg[vectors + 5] = 0xE0;
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]);
        rom
    }

    #[test]
    fn test_config_region_override_beats_the_header() {
        let mut rom = marker_rom(0);
        rom[9] = 0x01; // header claims PAL (iNES byte 9, bit 0)

        // The default config trusts the cartridge header.
        let mut trusting = Nes::new();
        trusting.load_rom(&rom).unwrap();
        assert_eq!(trusting.region(), Region::Pal);

        // An explicit override wins over what the header claims.
        let mut forced = Nes::with_config(Config {
            region_override: Some(Region::Ntsc),
            ..Config::default()
        });
        forced.load_rom(&rom).unwrap();
        assert_eq!(forced.region(), Region::Ntsc);
        assert_eq!(forced.config().region_override, Some(Region::Ntsc));
    }

    #[test]
    fn test_a_dmc_fetch_reaches_a_cpu_read_of_the_bus() {
        let mut nes = Nes::new();
        nes.load_rom(&dmc_sync_rom()).expect("load NROM");

        // The ROM's loop ends on the read the fetch is halted for, so the
        // marker is what says the transfer landed there. A fetch charged to a
        // later access - or charged twice per request - leaves the loop
        // spinning, which is the hang this test exists to catch.
        for _ in 0..20 {
            nes.frame();
            if nes.peek(0x10) == 0x5A {
                return;
            }
        }
        panic!(
            "the DMA's byte never reached a `$4000` read: pc={:#06X}",
            nes.cpu_pc()
        );
    }

    /// The cycles a sprite transfer takes: the halt cycle, an alignment cycle
    /// when the loop starts on a write cycle, and 256 read/write pairs — 513 or
    /// 514, by the halt cycle's parity (Mesen's `ProcessPendingDma`, which is
    /// what `cpu_dummy_writes` and AccuracyCoin's `Misaligned OAM DMA` measure).
    #[test]
    fn test_a_sprite_transfer_takes_513_or_514_cycles() {
        for start_parity in 0..2u64 {
            let mut bus = NesBus::new();
            // Line the machine's parity up with the case being measured.
            while bus.cpu_cycles & 1 != start_parity {
                bus.advance_one_cpu_cycle();
            }
            bus.run_dma_transfer(0x02);
            let before = bus.cpu_cycles;
            bus.start_pending_dma(0x1234, false);
            while bus.dma_running() {
                bus.run_dma_cycle();
            }
            let taken = bus.cpu_cycles - before;
            assert!(
                taken == 513 || taken == 514,
                "a sprite transfer takes 513 or 514 cycles, took {taken}"
            );
            assert_eq!(
                taken,
                if start_parity == 0 { 514 } else { 513 },
                "an even halt cycle needs the alignment cycle, an odd one does not"
            );
        }
    }

    /// The DMC's own fetch: the halt cycle, then the dummy read, then the byte —
    /// three or four cycles by the parity at the halt, and never the fixed four
    /// this core used to charge (`dmc_dma_during_read4` and `sprdma_and_dmc_dma`
    /// are the ROMs that count them).
    #[test]
    fn test_a_dmc_fetch_takes_three_or_four_cycles() {
        for start_parity in 0..2u64 {
            let mut bus = NesBus::new();
            while bus.cpu_cycles & 1 != start_parity {
                bus.advance_one_cpu_cycle();
            }
            // A sample for the channel to want: one byte, at $C000.
            bus.apu.write_register_at(0x4013, 0x00, false);
            bus.apu.write_register_at(0x4010, 0x0F, false);
            bus.apu.write_register_at(0x4015, 0x10, false);
            // The channel asks a couple of cycles later (its enable delay), and
            // the machine's per-cycle tick picks the ask up at once, so what is
            // waited for here is the transfer being due.
            let mut guard = 0;
            while !bus.dma_running() {
                bus.advance_one_cpu_cycle();
                guard += 1;
                assert!(guard < 64, "the channel never asked for its byte");
            }

            let before = bus.cpu_cycles;
            bus.start_pending_dma(0x1234, false);
            while bus.dma_running() {
                bus.run_dma_cycle();
            }
            let taken = bus.cpu_cycles - before;
            assert!(
                (3..=4).contains(&taken),
                "a DMC fetch takes three or four cycles, took {taken}"
            );
        }
    }

    #[test]
    fn test_taking_the_dmc_request_consumes_it() {
        // Mesen's halt latch is one-shot: the CPU clears it when the transfer
        // starts, so the APU ticking through the transfer's own cycles cannot
        // arm a second one. `test_a_dmc_fetch_reaches_a_cpu_read_of_the_bus`
        // is where that shows up end to end; this pins the latch itself.
        use crate::apu::AudioChannel;
        let mut dmc = crate::apu::DmcChannel::new();
        dmc.write_register(0x4010, 0x4F); // loop, fastest rate
        dmc.write_register(0x4012, 0xFF); // sample at $FFC0
        dmc.write_register(0x4013, 0x00); // one byte
        dmc.write_register(0x4015, 0x10); // enable

        // The ask goes out on the write's own cycle here (see `set_enabled`:
        // this core keeps blargg's "filled immediately" over Mesen's transfer
        // delay, measured in §7).
        assert!(dmc.take_byte_request(), "the channel asks for a byte");
        assert_eq!(dmc.read_address(), 0xFFC0);
        assert!(
            !dmc.take_byte_request(),
            "taking the request consumes it, so the transfer's own cycles cannot arm a second one"
        );

        dmc.supply_dma_byte(0x00);
        assert_eq!(dmc.last_fetched_byte(), 0x00);
    }

    #[test]
    fn test_config_can_disable_audio_capture() {
        let rom = marker_rom(0);

        let mut loud = Nes::new();
        loud.load_rom(&rom).unwrap();
        loud.frame();
        assert!(loud.buffered_audio_frames() > 0);

        let mut silent = Nes::with_config(Config {
            enable_audio: false,
            ..Config::default()
        });
        silent.load_rom(&rom).unwrap();
        silent.frame();
        // The console still ran a whole frame; only sample capture is off.
        assert_eq!(silent.frame_number(), 1);
        assert_eq!(silent.buffered_audio_frames(), 0);
    }

    #[test]
    fn test_rom_without_prg_banks_is_rejected_not_panicked() {
        // A header claiming zero PRG banks used to slip past the size check and
        // divide by zero while reading the reset vector.
        let mut rom = marker_rom(0);
        rom[4] = 0;

        let mut nes = Nes::new();
        assert!(matches!(
            nes.load_rom(&rom),
            Err(NesError::InvalidRomFormat(_))
        ));
    }

    #[test]
    fn test_malformed_cartridges_never_panic() {
        // Whatever a user hands the emulator has to come back as a value: over
        // FFI a panic is a crash, so no header combination may turn into one.
        // This walks small-but-plausible PRG sizes, a promised-but-absent CHR
        // image (CHR-RAM) and unknown mapper numbers.
        for prg_banks in [0u8, 1, 2, 16] {
            for chr_banks in [0u8, 1, 8] {
                for mapper in [0u8, 1, 4, 5, 69, 74, 119, 241, 254] {
                    let mut rom = vec![0u8; 16];
                    rom[0..4].copy_from_slice(b"NES\x1a");
                    rom[4] = prg_banks;
                    rom[5] = chr_banks;
                    rom[6] = (mapper & 0x0F) << 4;
                    rom[7] = mapper & 0xF0;
                    // The CHR image the header promises is deliberately absent.
                    rom.resize(16 + prg_banks as usize * 16 * 1024, 0);

                    let mut nes = Nes::new();
                    match nes.load_rom(&rom) {
                        Ok(()) => nes.frame(),
                        Err(NesError::InvalidRomFormat(_))
                        | Err(NesError::UnsupportedMapper(_)) => {}
                        Err(other) => panic!("mapper {mapper} came back with {other:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn test_fill_audio_drains_into_the_host_buffer() {
        let rom = marker_rom(0);
        let mut nes = Nes::new();
        nes.load_rom(&rom).unwrap();
        nes.frame();

        let queued = nes.buffered_audio_frames();
        assert!(
            queued > 100,
            "a frame should carry audio, got {queued} frames"
        );

        // A device period shorter than the queue takes only part of it.
        let mut period = vec![0.0f32; 100];
        assert_eq!(nes.fill_audio(&mut period), 100);
        assert_eq!(nes.buffered_audio_frames(), queued - 50);

        // An odd-sized buffer never yields half a stereo frame.
        let mut odd = vec![0.0f32; 101];
        assert_eq!(nes.fill_audio(&mut odd), 100);

        // An oversized request takes the rest, then reports nothing left.
        let mut rest = vec![0.0f32; 8192];
        assert_eq!(nes.fill_audio(&mut rest), (queued - 100) * 2);
        assert_eq!(nes.fill_audio(&mut rest), 0);
        assert_eq!(nes.buffered_audio_frames(), 0);
    }

    #[test]
    fn test_audio_sample_rate_is_taken_from_the_config() {
        let rom = marker_rom(0);
        let mut nes = Nes::with_config(Config {
            sample_rate: 32_000,
            ..Config::default()
        });
        nes.load_rom(&rom).unwrap();
        assert_eq!(nes.audio_sample_rate(), 32_000);

        nes.frame();
        // A frame lasts 1/fps, so it carries sample_rate/fps stereo frames.
        let expected = 32_000.0 / Region::Ntsc.fps();
        let queued = nes.buffered_audio_frames() as f64;
        assert!((queued - expected).abs() <= 2.0, "{queued} vs {expected}");
    }

    #[test]
    fn test_audio_rate_is_correct_for_every_region() {
        // The machine advances three PPU dots per CPU cycle, so the rate the
        // APU is ticked at follows the PPU clock rather than the cartridge's
        // nominal CPU clock, which PAL boards do not match under that ratio.
        let rom = marker_rom(0);

        for region in [Region::Ntsc, Region::Pal] {
            let mut nes = Nes::with_config(Config {
                region_override: Some(region),
                ..Config::default()
            });
            nes.load_rom(&rom).unwrap();
            nes.frame();

            let expected = nes.audio_sample_rate() as f64 / region.fps();
            let queued = nes.buffered_audio_frames() as f64;
            assert!(
                (queued - expected).abs() <= 2.0,
                "{region:?}: {queued} frames per frame, expected {expected}"
            );
        }
    }

    #[test]
    fn test_queued_audio_is_bounded_and_drops_the_oldest() {
        let rom = marker_rom(0);
        let mut nes = Nes::new();
        nes.load_rom(&rom).unwrap();

        // Ten seconds without ever pulling: the queue must stop growing rather
        // than let a stalled host eat memory.
        for _ in 0..600 {
            nes.frame();
        }

        let cap = (nes.audio_sample_rate() as f64 * 0.5) as usize;
        let queued = nes.buffered_audio_frames();
        assert!(
            queued <= cap,
            "queue grew to {queued} frames, above the {cap} cap"
        );
        // Dropping is per-sample, so it stays near the cap instead of
        // collapsing to something useless.
        assert!(queued > cap - 1000, "queue collapsed to {queued} frames");
    }

    /// Running a frame in sub-frame slices must land on exactly the state a
    /// whole-frame run reaches; otherwise a paced host would desync from a
    /// frame-at-a-time one.
    #[test]
    fn test_cycle_slices_reach_the_same_state_as_a_whole_frame() {
        let rom = marker_rom(0);

        let mut whole = Nes::new();
        whole.load_rom(&rom).unwrap();
        whole.frame();

        let mut sliced = Nes::new();
        sliced.load_rom(&rom).unwrap();
        // Odd-sized slices so instructions straddle the boundaries.
        while !sliced.is_frame_ready() {
            sliced.run_cycles(97);
        }

        assert_eq!(sliced.save_state(), whole.save_state());
    }

    #[test]
    fn test_single_stepping_reaches_the_same_state_as_a_whole_frame() {
        let rom = marker_rom(0);

        let mut whole = Nes::new();
        whole.load_rom(&rom).unwrap();
        whole.frame();

        let mut stepped = Nes::new();
        stepped.load_rom(&rom).unwrap();
        for _ in 0..50_000 {
            if stepped.is_frame_ready() {
                break;
            }
            assert!(
                stepped.step_instruction() > 0,
                "an instruction must cost cycles"
            );
        }
        assert!(
            stepped.is_frame_ready(),
            "stepping never reached a frame boundary"
        );

        assert_eq!(stepped.save_state(), whole.save_state());
    }

    #[test]
    fn test_new_mappers_boot_a_rom() {
        for mapper in [10u8, 21, 22, 23, 24, 25, 26, 32, 33, 48, 65, 69, 74] {
            let mut nes = Nes::new();
            nes.load_rom(&marker_rom(mapper))
                .unwrap_or_else(|e| panic!("mapper {mapper} rejected the ROM: {e:?}"));
            for _ in 0..2 {
                nes.frame();
            }
            assert_eq!(
                nes.peek(0x0010),
                0x5A,
                "mapper {mapper} did not boot into its fixed bank"
            );
        }
    }

    /// A VRC6 cartridge that programmes its expansion audio and then idles.
    fn vrc6_audio_rom() -> Vec<u8> {
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1a");
        rom.push(8); // 128KB PRG
        rom.push(1); // 8KB CHR
                     // Mapper number = (flags7 & 0xF0) | (flags6 >> 4), so 24 = 0x18
                     // needs flags6 = 0x80 and flags7 = 0x10.
        rom.push(0x80);
        rom.push(0x10);
        rom.extend_from_slice(&[0u8; 8]);

        let mut prg = vec![0u8; 16 * 0x2000];
        let base = 15 * 0x2000; // last 8KB bank -> $E000
        let mut code: Vec<u8> = vec![
            0x78, // SEI
            0xD8, // CLD
            0xA9, 0x00, 0x8D, 0x00, 0x20, // STA $2000
            0x8D, 0x01, 0x20, // STA $2001
            // Pulse 1: mode bit set so the duty generator is ignored, volume 15.
            0xA9, 0xFF, 0x8D, 0x00, 0x90, // STA $9000
            0xA9, 0x00, 0x8D, 0x01, 0x90, // STA $9001
            0xA9, 0x80, 0x8D, 0x02, 0x90, // STA $9002 (enable)
            0xA9, 0x00, 0x8D, 0x03, 0x90, // STA $9003 (no scaling)
            0xA9, 0x5A, 0x85, 0x10, // marker: proves the program ran
        ];
        let loop_addr = 0xE000 + code.len() as u16;
        code.extend_from_slice(&[0x4C, loop_addr as u8, (loop_addr >> 8) as u8]);
        prg[base..base + code.len()].copy_from_slice(&code);
        prg[base + 0x30] = 0x40; // stray interrupts just return
        let vectors = prg.len() - 6;
        prg[vectors] = 0x30;
        prg[vectors + 1] = 0xE0;
        prg[vectors + 2] = 0x00;
        prg[vectors + 3] = 0xE0;
        prg[vectors + 4] = 0x30;
        prg[vectors + 5] = 0xE0;
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]);
        rom
    }

    /// Peak level of everything the console queued, pulled out of the core the
    /// way an audio device callback would.
    fn drain_audio_peak(nes: &mut Nes) -> f32 {
        let mut buffer = vec![0.0f32; 4096];
        let mut peak = 0.0f32;
        loop {
            let written = nes.fill_audio(&mut buffer);
            for sample in &buffer[..written] {
                peak = peak.max(sample.abs());
            }
            if written < buffer.len() {
                break;
            }
        }
        peak
    }

    /// A machine whose APU is silent produces silence: the resampler's
    /// integrator must not put a DC level of its own into the output.
    #[test]
    fn test_a_silent_machine_produces_silence() {
        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");
        for _ in 0..2 {
            nes.frame();
        }

        let mut buffer = vec![0.0f32; 4096];
        let written = nes.fill_audio(&mut buffer);
        assert!(written > 0, "the machine produced audio samples");
        assert!(
            buffer[..written].iter().all(|sample| *sample == 0.0),
            "silence stays silent: {:?}",
            &buffer[..written.min(8)]
        );
    }

    #[test]
    fn test_vrc6_expansion_audio_reaches_the_mixer() {
        let mut nes = Nes::new();
        nes.load_rom(&vrc6_audio_rom()).expect("load VRC6 ROM");

        let mut peak = 0.0f32;
        for _ in 0..10 {
            nes.frame();
            peak = peak.max(drain_audio_peak(&mut nes));
        }

        assert_eq!(nes.peek(0x0010), 0x5A, "the VRC6 test program never ran");
        assert!(
            peak > 0.01,
            "VRC6 expansion audio never reached the output (peak {peak})"
        );
    }

    /// Build a cartridge for `mapper` whose program plays one expansion-audio
    /// channel, then idles. `program` is placed at $E000.
    fn expansion_audio_rom(mapper: u8, program: &[u8]) -> Vec<u8> {
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1a");
        rom.push(8); // 128KB PRG
        rom.push(1); // 8KB CHR
        rom.push(mapper << 4);
        rom.push(mapper & 0xF0);
        rom.extend_from_slice(&[0u8; 8]);

        let mut prg = vec![0u8; 16 * 0x2000];
        let base = 15 * 0x2000;
        let mut code: Vec<u8> = vec![0x78, 0xD8]; // SEI, CLD
        code.extend_from_slice(&[0xA9, 0x00, 0x8D, 0x00, 0x20]); // STA $2000
        code.extend_from_slice(&[0x8D, 0x01, 0x20]); // STA $2001
        code.extend_from_slice(program);
        code.extend_from_slice(&[0xA9, 0x5A, 0x85, 0x10]); // marker
        let loop_addr = 0xE000 + code.len() as u16;
        code.extend_from_slice(&[0x4C, loop_addr as u8, (loop_addr >> 8) as u8]);
        prg[base..base + code.len()].copy_from_slice(&code);
        prg[base + 0xC0] = 0x40; // stray interrupts return

        let vectors = prg.len() - 6;
        prg[vectors] = (0xE000 + 0xC0) as u8;
        prg[vectors + 1] = 0xE0;
        prg[vectors + 2] = 0x00;
        prg[vectors + 3] = 0xE0;
        prg[vectors + 4] = (0xE000 + 0xC0) as u8;
        prg[vectors + 5] = 0xE0;
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]);
        rom
    }

    #[test]
    fn test_sunsoft_5b_audio_reaches_the_mixer() {
        // $C000 selects a YM2149 register, $E000 writes it: short tone A
        // period, tone enabled on A, full volume.
        let program: &[u8] = &[
            0xA9, 0x00, 0x8D, 0x00, 0xC0, // select register 0
            0xA9, 0x10, 0x8D, 0x00, 0xE0, // tone A period low
            0xA9, 0x01, 0x8D, 0x00, 0xC0, // select register 1
            0xA9, 0x00, 0x8D, 0x00, 0xE0, // tone A period high
            0xA9, 0x07, 0x8D, 0x00, 0xC0, // select register 7
            0xA9, 0x3E, 0x8D, 0x00, 0xE0, // mixer: tone A on, noise off
            0xA9, 0x08, 0x8D, 0x00, 0xC0, // select register 8
            0xA9, 0x0F, 0x8D, 0x00, 0xE0, // volume A = 15
        ];

        let mut nes = Nes::new();
        nes.load_rom(&expansion_audio_rom(69, program))
            .expect("load 5B ROM");

        let mut peak = 0.0f32;
        for _ in 0..10 {
            nes.frame();
            peak = peak.max(drain_audio_peak(&mut nes));
        }
        assert_eq!(nes.peek(0x0010), 0x5A, "the 5B test program never ran");
        assert!(
            peak > 0.01,
            "Sunsoft 5B expansion audio never reached the output (peak {peak})"
        );
    }

    #[test]
    fn test_mmc5_audio_reaches_the_mixer() {
        // Point $E000 at the last 8KB bank, enable pulse A, then play a note.
        let program: &[u8] = &[
            0xA9, 0x0F, 0x8D, 0x17, 0x51, // $5117 = 15 (last 8KB bank)
            0xA9, 0x01, 0x8D, 0x15, 0x50, // $5015 = enable pulse A
            0xA9, 0xBF, 0x8D, 0x00, 0x50, // duty 2, constant volume 15
            0xA9, 0x20, 0x8D, 0x02, 0x50, // period low
            0xA9, 0x08, 0x8D, 0x03, 0x50, // period high + length load
        ];

        let mut nes = Nes::new();
        nes.load_rom(&expansion_audio_rom(5, program))
            .expect("load MMC5 ROM");

        let mut peak = 0.0f32;
        for _ in 0..10 {
            nes.frame();
            peak = peak.max(drain_audio_peak(&mut nes));
        }
        assert_eq!(nes.peek(0x0010), 0x5A, "the MMC5 test program never ran");
        assert!(
            peak > 0.01,
            "MMC5 expansion audio never reached the output (peak {peak})"
        );
    }

    /// The master clock is what both chips are derived from, so the CPU cycle
    /// count and the PPU's own position have to agree with it dot for dot.
    ///
    /// This is the property the rest of the cycle work hangs off: an access
    /// placed "two dots into its cycle" is only meaningful if a cycle covers
    /// exactly the dots the clock model says it does.
    #[test]
    fn test_the_ppu_position_follows_the_master_clock() {
        /// Dots in one NTSC frame: 341 per scanline, 262 scanlines.
        const DOTS_PER_FRAME: u64 = 341 * 262;

        let mut nes = Nes::new();
        nes.load_rom(&marker_rom(0)).expect("load NROM");
        let cpu_divider = nes.region().cpu_divider() as u64;
        let ppu_divider = nes.region().ppu_divider() as u64;

        for frame in 1..=5u64 {
            nes.frame();

            let cycles = nes.cpu_cycles();
            assert_eq!(nes.master_clock(), cycles * cpu_divider, "frame {frame}");
            assert_eq!(
                nes.ppu_dots(),
                nes.master_clock() / ppu_divider,
                "the PPU's dots are the master clock divided down"
            );

            // This ROM leaves rendering off, so no odd frame skips a dot and a
            // frame is a whole 341 x 262 of them.
            let (scanline, dot) = nes.ppu_position();
            assert_eq!(
                nes.ppu_dots() % DOTS_PER_FRAME,
                scanline as u64 * 341 + dot as u64,
                "frame {frame}: the PPU is where the dots put it"
            );
            assert_eq!(nes.frame_number(), frame);
        }
    }

    /// A CPU cycle covers three PPU dots, on both regions, and an access sits
    /// two dots into the cycle - the placement the ROM suites pin down.
    #[test]
    fn test_a_cpu_cycle_covers_three_dots_with_the_access_two_in() {
        // NTSC's 12/4 is a whole number of dots per cycle, so it runs the
        // counted fast path and every cycle covers exactly three.
        let mut bus = NesBus::new();
        bus.region = Region::Ntsc;
        assert!(
            bus.has_whole_dot_ratio(),
            "NTSC takes the counted fast path"
        );
        let mut previous = bus.ppu_clock_at(0);
        for cycle in 1..=64u64 {
            let clock = bus.ppu_clock_at(cycle);
            assert_eq!(
                (clock - previous) / bus.ppu_divider(),
                bus.whole_dots_per_cpu_cycle() as u64,
                "cycle {cycle} does not cover three dots"
            );
            previous = clock;

            let cycle_start = bus.master_clock_at(cycle - 1);
            assert_eq!(
                bus.access_clock(cycle - 1) - cycle_start,
                ACCESS_DOTS as u64 * bus.ppu_divider(),
                "an access belongs two dots in"
            );
        }

        // PAL's 16/5 is 3.2, so it runs the master-clock path: four dots on
        // some cycles and three on the others, adding up to the real ratio over
        // five of them.
        bus.region = Region::Pal;
        assert!(
            !bus.has_whole_dot_ratio(),
            "PAL's cycle is not a whole number of dots"
        );
        let five_cycles = bus.master_clock_at(5) - bus.master_clock_at(0);
        assert_eq!(
            five_cycles / bus.ppu_divider(),
            16,
            "five PAL cycles are sixteen dots"
        );
        for cycle in 1..=64u64 {
            let start = bus.master_clock_at(cycle - 1);
            let end = bus.master_clock_at(cycle);
            let dots = (end - start) / bus.ppu_divider();
            assert!(
                dots == 3 || dots == 4,
                "cycle {cycle} covers {dots} dots, not three or four"
            );
        }
    }

    /// The two spellings of a cycle's dots - the counted loop a whole ratio
    /// takes and the master-clock run a fractional one will take - are held to
    /// each other, so flipping PAL to its real 3.2:1 cannot move where the dots
    /// fall without this failing first.
    #[test]
    fn test_the_whole_ratio_flags_agree_with_the_dividers() {
        for region in [Region::Ntsc, Region::Pal] {
            let mut bus = NesBus::new();
            bus.region = region;
            let whole = bus.cpu_divider() % bus.ppu_divider() == 0;
            assert_eq!(
                bus.has_whole_dot_ratio(),
                whole,
                "{region:?}: the fast path is only valid for a whole ratio"
            );
            if whole {
                assert_eq!(
                    bus.whole_dots_per_cpu_cycle() as u64 * bus.ppu_divider(),
                    bus.cpu_divider(),
                    "{region:?}: the counted dots have to cover the whole cycle"
                );
            }
        }
    }

    /// Driving the PPU to a master-clock target has to land it exactly where
    /// the counted path does, dot for dot.
    #[test]
    fn test_the_master_clock_path_places_the_same_dots() {
        let mut targeted = NesBus::new();
        let mut counted = NesBus::new();

        let mut clock = targeted.ppu_clock_at(0);
        for cycle in 0..256u64 {
            let target = targeted.master_clock_at(cycle + 1);
            targeted.run_ppu_to(&mut clock, target);
            counted.advance_one_cpu_cycle();

            assert_eq!(
                targeted.ppu.cycle, counted.ppu.cycle,
                "cycle {cycle}: the dot does not match"
            );
            assert_eq!(
                targeted.ppu.scanline, counted.ppu.scanline,
                "cycle {cycle}: the scanline does not match"
            );
            assert_eq!(
                clock,
                targeted.ppu_clock_at(cycle + 1),
                "cycle {cycle}: the PPU has to stop on a dot boundary"
            );
        }
    }
}
