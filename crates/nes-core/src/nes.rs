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
use crate::ppu::rendering::{
    BackgroundRenderer, FrameBuffer, MapperChr, Mirroring, ScanlineProcessor, VramChr,
};
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

/// Magic prefix identifying a [`Nes::save_state`] blob.
const STATE_MAGIC: &[u8; 5] = b"NESST";
/// Save-state format version, bumped whenever the encoding changes.
const STATE_VERSION: u8 = 1;

/// PPU dots the machine advances per CPU cycle.
const PPU_DOTS_PER_CPU_CYCLE: f64 = 3.0;

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

/// Controller state for one player
#[derive(Debug, Clone, Default)]
struct ControllerState {
    /// Current button states (true = pressed)
    buttons: [bool; 8],
    /// Shift register for serial output (used by $4016/$4017 reads)
    shift_register: u8,
    /// Whether the shift register has been latched
    latched: bool,
}

crate::state::state_fields!(ControllerState {
    buttons,
    shift_register,
    latched,
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

    /// Latch the current button states into the shift register
    fn latch(&mut self) {
        // Pack button states into shift register
        // Order: A, B, Select, Start, Up, Down, Left, Right
        self.shift_register = 0;
        for (i, &pressed) in self.buttons.iter().enumerate() {
            if pressed {
                self.shift_register |= 1 << i;
            }
        }
        self.latched = true;
    }

    /// Release the latch so subsequent reads shift out button states.
    ///
    /// The strobe line going low is what freezes the shift register; without
    /// this the controller would report the A button forever.
    fn unlatch(&mut self) {
        self.latched = false;
    }

    /// Read one bit from the shift register and shift
    fn read(&mut self) -> u8 {
        if self.latched {
            // While latched, always return the state of button A (bit 0)
            self.buttons[0] as u8
        } else {
            // Read and shift
            let bit = self.shift_register & 1;
            self.shift_register >>= 1;
            // After 8 reads, subsequent reads return 1 (open bus behavior)
            bit | 0x40 // Return bit with open bus on bit 6
        }
    }
}

/// ROM header parser for iNES format
struct RomHeader {
    prg_rom_size: usize,
    chr_rom_size: usize,
    mapper_number: u16,
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

        let prg_rom_size = data[4] as usize * 16 * 1024; // 16KB units
        let chr_rom_size = data[5] as usize * 8 * 1024; // 8KB units

        let flags6 = data[6];
        let flags7 = data[7];

        let mirroring = if flags6 & 0x08 != 0 {
            Mirroring::FourScreen
        } else if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let has_trainer = flags6 & 0x04 != 0;
        let has_battery = flags6 & 0x02 != 0;

        // Mapper number from flags6 (high nibble) and flags7 (high nibble)
        let mapper_number = ((flags7 & 0xF0) as u16) | ((flags6 & 0xF0) as u16 >> 4);

        // TV system: byte 9 bit 0 is the classic PAL flag, byte 10 bits 0-1
        // encode it more reliably (0b10 = PAL).
        let flags9 = data[9];
        let flags10 = data[10];
        let region = if flags9 & 0x01 != 0 || (flags10 & 0x03) == 0x02 {
            Region::Pal
        } else {
            Region::Ntsc
        };

        let header = Self {
            prg_rom_size,
            chr_rom_size,
            mapper_number,
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
    /// Controller states (player 1 and 2)
    controllers: [ControllerState; 2],
    /// Controller strobe/latch state
    controller_strobe: bool,
    /// NMI pending flag (from PPU)
    nmi_pending: bool,
    /// IRQ pending flag (from APU or mapper)
    irq_pending: bool,
    /// Interleaved stereo audio waiting for the host to pull it
    audio_buffer: VecDeque<f32>,
    /// Fractional accumulator that paces the APU resampler
    audio_accumulator: f64,
    /// Rate the APU output is resampled to, for the host's audio device
    audio_sample_rate: u32,
    /// Cap on `audio_buffer`, derived from the sample rate
    audio_capacity: usize,
    /// CPU cycles accumulated for audio generation
    cpu_cycles: u64,
    /// CPU cycles the next OAM/DMC DMA transfer still has to steal
    dma_stall_cycles: u16,
    frame_buffer: FrameBuffer,
    bg_renderer: BackgroundRenderer,
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
    frame_ready: bool,
}

crate::state::state_fields!(NesBus {
    cpu_ram,
    ppu,
    apu,
    mapper,
    controllers,
    controller_strobe,
    nmi_pending,
    irq_pending,
    audio_accumulator,
    cpu_cycles,
    dma_stall_cycles,
    bg_renderer,
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
            controller_strobe: false,
            nmi_pending: false,
            irq_pending: false,
            audio_buffer: VecDeque::with_capacity(audio_capacity_floats(
                DEFAULT_SAMPLE_RATE,
                AUDIO_BUFFER_MS,
            )),
            audio_accumulator: 0.0,
            audio_sample_rate: DEFAULT_SAMPLE_RATE,
            audio_capacity: audio_capacity_floats(DEFAULT_SAMPLE_RATE, AUDIO_BUFFER_MS),
            cpu_cycles: 0,
            dma_stall_cycles: 0,
            frame_buffer: FrameBuffer::new(),
            bg_renderer: BackgroundRenderer::new(),
            sprite_evaluator: SpriteEvaluator::new(),
            mirroring: Mirroring::Horizontal,
            region: Region::Ntsc,
            region_override: None,
            enable_audio: true,
            open_bus: 0,
            has_battery: false,
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
        self.bg_renderer.set_mirroring(header.mirroring);
        self.region = self.region_override.unwrap_or(header.region);
        self.has_battery = header.has_battery;

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
        self.mapper.as_ref().and_then(|m| m.save_ram()).map(|r| r.to_vec())
    }

    /// Restore a snapshot produced by [`NesBus::save_ram`].
    fn load_save_ram(&mut self, data: &[u8]) {
        if let Some(ref mut mapper) = self.mapper {
            mapper.load_save_ram(data);
        }
    }

    /// Reset the system
    fn reset(&mut self, cpu: &mut Cpu) {
        cpu.reset(self);
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

        // One frame of CPU cycles for the cartridge's TV system.
        let target_cpu_cycles = self.region.cpu_cycles_per_frame() as u64;
        let start_cycles = cpu.cycles();

        while cpu.cycles() - start_cycles < target_cpu_cycles && !self.frame_ready {
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

        let start_cycles = cpu.cycles();
        let target_cpu_cycles = cycles as u64;
        while cpu.cycles() - start_cycles < target_cpu_cycles && !self.frame_ready {
            self.step_instruction(cpu);
        }

        (cpu.cycles() - start_cycles) as u32
    }

    /// Execute a single CPU instruction and advance the rest of the machine by
    /// the cycles it consumed.
    ///
    /// This is the smallest unit of emulation; `frame_ready` is left alone so a
    /// debugger can single-step across a frame boundary and still see it.
    fn step_instruction(&mut self, cpu: &mut Cpu) -> u8 {
        // Fetch and execute one instruction
        let opcode = self.read(cpu.pc());
        let cycles = cpu.execute_instruction(self, opcode);

        // Mappers whose IRQ counter runs at the CPU clock rate.
        if let Some(ref mut mapper) = self.mapper {
            mapper.on_cpu_cycles(cycles);
        }

        // Update PPU and APU based on cycles consumed
        self.on_cpu_cycle(cycles);

        // The CPU is halted while OAM or DMC DMA runs; the rest of the
        // machine keeps ticking.
        if self.dma_stall_cycles > 0 {
            let stall = self.dma_stall_cycles;
            self.dma_stall_cycles = 0;
            self.advance_cycles(stall);
        }

        cycles
    }

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
        let memory = PpuMemory::System {
            chr: self.mapper.as_deref_mut(),
            mirroring: self.mirroring,
            open_bus: self.open_bus,
        };
        self.ppu.read_register_with(addr, memory)
    }

    /// Write to PPU register
    fn write_ppu_register(&mut self, addr: u16, value: u8) {
        let memory = PpuMemory::System {
            chr: self.mapper.as_deref_mut(),
            mirroring: self.mirroring,
            open_bus: self.open_bus,
        };
        self.ppu.write_register_with(addr, value, memory);
    }

    /// Read from APU/register
    fn read_apu_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => {
                // APU status register
                self.apu.read_register(addr)
            }
            0x4016 => {
                // Controller 1 data
                self.controllers[0].read()
            }
            0x4017 => {
                // Controller 2 data
                self.controllers[1].read()
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
                // OAMDMA - sprite DMA transfer of 256 bytes into OAM.
                let source_addr = (value as u16) << 8;
                for i in 0..256 {
                    let byte = self.read(source_addr + i);
                    self.ppu.oam[self.ppu.oam_addr.wrapping_add(i as u8) as usize] = byte;
                }
                // The transfer halts the CPU for 513 cycles, or 514 when it
                // starts on an even cycle because of the extra alignment read.
                self.dma_stall_cycles = 513 + (self.cpu_cycles & 1) as u16;
            }
            0x4016 => {
                // Controller strobe
                self.controller_strobe = value & 1 != 0;
                if self.controller_strobe {
                    self.controllers[0].latch();
                    self.controllers[1].latch();
                } else {
                    self.controllers[0].unlatch();
                    self.controllers[1].unlatch();
                }
            }
            _ => {
                self.apu.write_register(addr, value);
            }
        }
    }

    /// Advance the APU by one CPU cycle and, when the resampler is due,
    /// append an interleaved stereo sample to the output buffer.
    fn tick_apu_cycle(&mut self) {
        self.apu.tick();

        // The DMC asks the bus for sample bytes; service the request
        // immediately so the channel keeps playing.
        if let Some(addr) = self.apu.dmc_dma_request() {
            let byte = self.read(addr);
            self.apu.supply_dmc_byte(byte);
            // A DMC sample fetch steals four cycles from the CPU.
            self.dma_stall_cycles = self.dma_stall_cycles.saturating_add(4);
        }

        // Resample the CPU-rate mixer output down to the host's device rate.
        // The APU keeps ticking when audio capture is off because its frame
        // IRQ and the DMC DMA requests above change CPU timing; only the
        // samples handed to the host are skipped.
        if !self.enable_audio {
            return;
        }

        // The machine advances three PPU dots per CPU cycle, so this is the
        // rate the APU is really ticked at — not the cartridge's nominal CPU
        // clock, which PAL boards do not match under that ratio.
        let cpu_clock_rate = self.region.ppu_clock_rate() as f64 / PPU_DOTS_PER_CPU_CYCLE;
        self.audio_accumulator += self.audio_sample_rate as f64;
        if self.audio_accumulator >= cpu_clock_rate {
            self.audio_accumulator -= cpu_clock_rate;
            // The cartridge's own sound hardware (VRC6, MMC5, ...) mixes on
            // top of the 2A03 output.
            let expansion = self.mapper.as_ref().map_or(0.0, |m| m.audio_output());
            let sample = self.apu.output() + expansion;
            // Mono APU output, duplicated for the left and right channels.
            self.audio_buffer.push_back(sample);
            self.audio_buffer.push_back(sample);
            self.trim_audio_buffer();
        }
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
    /// Rendering works off `ppu.render_v` (a dedicated scroll counter) rather
    /// than `ppu.v`, so the CPU writing nametables through $2006/$2007 during
    /// VBlank cannot disturb an in-progress frame.
    fn ppu_step(&mut self) {
        let scanline = self.ppu.scanline;
        let cycle = self.ppu.cycle;
        let pre_render = self.region.last_scanline();

        if scanline < 240 {
            // Visible scanlines
            if cycle == 256 {
                    // Reload the horizontal scroll for this line from t.
                    self.ppu.render_v =
                        (self.ppu.render_v & !0x041F) | (self.ppu.t & 0x041F);

                    // Background + sprites for this scanline. Pattern data is
                    // read through the cartridge so mid-frame CHR bank
                    // switches take effect immediately.
                    let mask = self.ppu.mask;
                    match self.mapper.as_deref_mut() {
                        Some(mapper) => {
                            let mut chr = MapperChr(mapper);
                            self.bg_renderer.render_scanline(&self.ppu, &mut chr, scanline, mask);
                            self.bg_renderer
                                .render_sprites_for_scanline(&self.ppu, &mut chr, scanline, mask);
                        }
                        None => {
                            let mut chr = VramChr;
                            self.bg_renderer.render_scanline(&self.ppu, &mut chr, scanline, mask);
                            self.bg_renderer
                                .render_sprites_for_scanline(&self.ppu, &mut chr, scanline, mask);
                        }
                    }
                    if self.bg_renderer.check_sprite_0_hit() {
                        self.ppu.status.insert(PpuStatus::SPRITE_0_HIT);
                    }

                    // Advance the vertical scroll for the next scanline.
                    self.ppu.render_v = ScanlineProcessor::increment_scroll_y(self.ppu.render_v);

                    // Evaluate sprites for the next scanline.
                    let next_scanline = scanline + 1;
                    if next_scanline < 240 {
                        let overflow = self.sprite_evaluator.evaluate(
                            &self.ppu.oam,
                            next_scanline,
                            self.ppu.ctrl,
                            self.ppu.oam_addr,
                        );
                        if overflow {
                            self.ppu.status.insert(PpuStatus::SPRITE_OVERFLOW);
                        }
                        self.bg_renderer.secondary_oam =
                            self.sprite_evaluator.secondary_oam.clone();
                    }
            }
        } else if scanline == 241 {
            // Start of VBlank
            if cycle == 1 {
                self.ppu.status.insert(PpuStatus::VBLANK);
                if self.ppu.ctrl.contains(PpuCtrl::NMI_ENABLE) {
                    self.nmi_pending = true;
                }
            }
        } else if scanline == pre_render {
            // Pre-render scanline
            if cycle == 1 {
                self.ppu.status.remove(PpuStatus::VBLANK);
                self.ppu.status.remove(PpuStatus::SPRITE_OVERFLOW);
                self.ppu.status.remove(PpuStatus::SPRITE_0_HIT);
                // The hit latch must also be forgotten, otherwise the
                // renderer would keep reporting last frame's hit.
                self.bg_renderer.sprite_renderer.clear_sprite_0_hit();
            }
            if cycle == 304 {
                // Vertical + horizontal scroll both reload from t.
                self.ppu.render_v = self.ppu.t;
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
                    mapper.write(addr, value);
                }
            }
        }
    }

    fn on_cpu_cycle(&mut self, cycles: u8) {
        self.advance_cycles(cycles as u16);
    }

    fn nmi_pending(&self) -> bool {
        self.nmi_pending
    }

    fn clear_nmi(&mut self) {
        self.nmi_pending = false;
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
            || self.apu.frame_counter_irq
            || self.apu.dmc.irq_active()
            || self.mapper.as_ref().map_or(false, |m| m.irq_pending())
    }
}

impl NesBus {
    /// Advance the PPU and APU by `cycles` CPU cycles without executing any
    /// CPU instructions. Used for normal instruction timing and for the cycles
    /// stolen by OAM/DMC DMA.
    fn advance_cycles(&mut self, cycles: u16) {
        // Update PPU (3 PPU cycles per CPU cycle)
        let ppu_cycles = cycles * 3;

        for _ in 0..ppu_cycles {
            // Advance PPU cycle
            self.ppu.cycle += 1;

            if self.ppu.cycle > 340 {
                self.ppu.cycle = 0;
                self.ppu.scanline += 1;

                if self.ppu.scanline > self.region.last_scanline() {
                    self.ppu.scanline = 0;
                    self.ppu.frame += 1;
                    self.frame_ready = true;

                    self.frame_buffer.pixels.copy_from_slice(self.bg_renderer.get_frame());
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
                            self.bg_renderer.set_mirroring(mode);
                        }
                    }
                }
            }
            self.ppu_step();
        }

        // The APU runs one tick per CPU cycle.
        for _ in 0..cycles {
            self.tick_apu_cycle();
        }

        // Update CPU cycle counter
        self.cpu_cycles += cycles as u64;
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
        bus.reset(&mut cpu);
        Self { cpu, bus, config }
    }

    /// The configuration this console was created with.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Load an iNES cartridge image and reset the console.
    pub fn load_rom(&mut self, data: &[u8]) -> crate::Result<()> {
        self.bus.load_rom(data)?;
        self.bus.reset(&mut self.cpu);
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
        self.bus.ppu.frame
    }

    /// Whether a frame has finished since the last [`Nes::clear_frame_ready`].
    pub fn is_frame_ready(&self) -> bool {
        self.bus.frame_ready
    }

    /// Clear the frame-ready flag.
    pub fn clear_frame_ready(&mut self) {
        self.bus.frame_ready = false;
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
        if data.len() < HEADER || !data.starts_with(STATE_MAGIC) || data[STATE_MAGIC.len()] != STATE_VERSION {
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

        // Point v at $0000 and upload a CHR byte through $2007.
        bus.write(0x2006, 0x00);
        bus.write(0x2006, 0x00);
        bus.write(0x2007, 0x5A);

        // The write reached the cartridge, not the PPU's own VRAM.
        assert_eq!(bus.mapper.as_mut().unwrap().read(0x0000), 0x5A);

        // Reading back through $2007 is buffered: the old buffer first, then
        // the byte fetched from cartridge CHR.
        bus.write(0x2006, 0x00);
        bus.write(0x2006, 0x00);
        assert_eq!(bus.read(0x2007), 0x00);
        assert_eq!(bus.read(0x2007), 0x5A);
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

        // Test latch
        controller.latch();
        assert!(controller.latched);

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
        controller.latch();
        assert_eq!(controller.read() & 1, 1);
        assert_eq!(controller.read() & 1, 1);

        // Releasing the strobe freezes the shift register and reads walk
        // through the buttons in order: A, B, Select, Start, ...
        controller.unlatch();
        let bits: Vec<u8> = (0..8).map(|_| controller.read() & 1).collect();
        assert_eq!(bits, vec![1, 0, 0, 1, 0, 0, 0, 0]);
    }

    /// PPUSTATUS reads must clear VBlank *and* the sprite flags, otherwise
    /// games that spin on the sprite 0 hit (Super Mario Bros. does, to time its
    /// status bar split) stall in their wait loops.
    #[test]
    fn ppustatus_read_clears_vblank_and_sprite_flags() {
        let mut nes = Nes::new();
        nes.bus.ppu.status.insert(
            PpuStatus::VBLANK | PpuStatus::SPRITE_OVERFLOW | PpuStatus::SPRITE_0_HIT,
        );

        let value = nes.peek(0x2002);

        assert_eq!(value & 0x80, 0x80, "VBlank should be reported by the read");
        assert_eq!(value & 0x40, 0x40, "sprite 0 hit should be reported by the read");
        assert_eq!(value & 0x20, 0x20, "sprite overflow should be reported by the read");
        assert!(!nes.bus.ppu.status.contains(PpuStatus::VBLANK));
        assert!(!nes.bus.ppu.status.contains(PpuStatus::SPRITE_OVERFLOW));
        assert!(!nes.bus.ppu.status.contains(PpuStatus::SPRITE_0_HIT));
        assert!(!nes.bus.ppu.w, "the write toggle should be reset by the read");
    }

    /// Super Mario Bros. samples the Select button on only one frame out of
    /// every sixteen (it gates on its `$0780` counter), and its joypad routine
    /// turns Select into a single-frame "new press" flag, so a sustained press
    /// is only accepted when it *starts* on that sampling frame. This pins the
    /// ROM's behaviour so a future CPU/PPU/NMI change cannot silently alter it,
    /// and so the web UI can keep reporting raw button state without any
    /// pulse-while-held workaround.
    // Reads the ROM off disk, so it needs the `std` feature.
    #[cfg(feature = "std")]
    #[test]
    fn smb_select_is_sampled_once_per_sixteen_frames() {
        let Ok(rom) = std::fs::read("../../roms/超级玛莉.NES") else {
            eprintln!("Skipping: SMB ROM not found");
            return;
        };

        let mut toggled = 0;
        for offset in 0..16u32 {
            let mut nes = Nes::new();
            nes.load_rom(&rom).expect("SMB should load");
            for _ in 0..240 + offset {
                nes.frame();
            }

            nes.set_button(1, BUTTON_SELECT, true);
            for _ in 0..6 {
                nes.frame();
            }
            nes.set_button(1, BUTTON_SELECT, false);
            for _ in 0..30 {
                nes.frame();
            }

            // $077A is the player count: 0 = 1 PLAYER, 1 = 2 PLAYER.
            if nes.peek(0x077A) != 0 {
                toggled += 1;
            }
        }

        assert_eq!(
            toggled, 1,
            "exactly one of the sixteen starting phases should be accepted"
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

        bus.write(0x0000, 0xFF);
        let status = bus.read(0x2002);
        assert_eq!(status & 0x1F, 0x1F, "bits 0-4 should come from open bus");
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

    /// Minimal battery-backed NROM cartridge: it copies $6000 into $0010 and
    /// then writes $42 to $6000.
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
    fn test_fill_audio_drains_into_the_host_buffer() {
        let rom = marker_rom(0);
        let mut nes = Nes::new();
        nes.load_rom(&rom).unwrap();
        nes.frame();

        let queued = nes.buffered_audio_frames();
        assert!(queued > 100, "a frame should carry audio, got {queued} frames");

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
            assert!(stepped.step_instruction() > 0, "an instruction must cost cycles");
        }
        assert!(stepped.is_frame_ready(), "stepping never reached a frame boundary");

        assert_eq!(stepped.save_state(), whole.save_state());
    }

    #[test]
    fn test_new_mappers_boot_a_rom() {
        for mapper in [10u8, 21, 22, 23, 24, 25, 26, 32, 33, 48, 65, 69] {
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
}
