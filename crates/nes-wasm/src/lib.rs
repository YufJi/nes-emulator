//! JSNES WASM bindings
//!
//! This crate provides WebAssembly bindings for the JSNES emulator,
//! allowing it to run in web browsers.

use nes_core::apu::Apu;
use nes_core::bus::Bus;
use nes_core::cpu::Cpu;
use nes_core::mapper::{create_mapper, Mapper};
use nes_core::ppu::rendering::{BackgroundRenderer, FrameBuffer, Mirroring, ScanlineProcessor};
use nes_core::ppu::sprite::SpriteEvaluator;
use nes_core::ppu::{Ppu, PpuCtrl, PpuMask, PpuStatus};
use nes_core::JsnesError;
use wasm_bindgen::prelude::*;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

/// Button constants matching the JavaScript API
pub const BUTTON_A: u8 = 0;
pub const BUTTON_B: u8 = 1;
pub const BUTTON_SELECT: u8 = 2;
pub const BUTTON_START: u8 = 3;
pub const BUTTON_UP: u8 = 4;
pub const BUTTON_DOWN: u8 = 5;
pub const BUTTON_LEFT: u8 = 6;
pub const BUTTON_RIGHT: u8 = 7;

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
    has_trainer: bool,
}

impl RomHeader {
    fn parse(data: &[u8]) -> core::result::Result<(Self, usize), JsnesError> {
        if data.len() < 16 {
            return Err(JsnesError::InvalidRomFormat(
                "ROM too small for header".to_string(),
            ));
        }

        // Check iNES magic number
        if &data[0..4] != b"NES\x1a" {
            return Err(JsnesError::InvalidRomFormat(
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

        // Mapper number from flags6 (high nibble) and flags7 (high nibble)
        let mapper_number = ((flags7 & 0xF0) as u16) | ((flags6 & 0xF0) as u16 >> 4);

        let header = Self {
            prg_rom_size,
            chr_rom_size,
            mapper_number,
            mirroring,
            has_trainer,
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
    /// Audio sample buffer for output
    audio_buffer: Vec<f32>,
    /// CPU cycles accumulated for audio generation
    cpu_cycles: u64,
    frame_buffer: FrameBuffer,
    bg_renderer: BackgroundRenderer,
    /// Sprite evaluator for the current scanline
    sprite_evaluator: SpriteEvaluator,
    /// Nametable mirroring mode from the cartridge header
    mirroring: Mirroring,
    frame_ready: bool,
    /// PPU read buffer for $2007
    ppu_read_buffer: u8,
}

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
            audio_buffer: Vec::with_capacity(4096),
            cpu_cycles: 0,
            frame_buffer: FrameBuffer::new(),
            bg_renderer: BackgroundRenderer::new(),
            sprite_evaluator: SpriteEvaluator::new(),
            mirroring: Mirroring::Horizontal,
            frame_ready: false,
            ppu_read_buffer: 0,
        }
    }

    /// Load a ROM into the system
    fn load_rom(&mut self, data: &[u8]) -> core::result::Result<(), JsnesError> {
        let (header, data_offset) = RomHeader::parse(data)?;

        // Extract PRG-ROM
        let prg_start = data_offset;
        let prg_end = prg_start + header.prg_rom_size;
        if data.len() < prg_end {
            return Err(JsnesError::InvalidRomFormat(
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

        Ok(())
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
        self.audio_buffer.clear();

        // NTSC: 262 scanlines per frame, 341 PPU cycles per scanline
        // Each CPU cycle = 3 PPU cycles
        // Total PPU cycles per frame = 262 * 341 = 89,342
        // Total CPU cycles per frame = 89,342 / 3 ≈ 29,780

        let target_cpu_cycles = 29780; // Approximate cycles per frame for NTSC
        let start_cycles = cpu.cycles();

        while cpu.cycles() - start_cycles < target_cpu_cycles && !self.frame_ready {
            // Fetch and execute one instruction
            let opcode = self.read(cpu.pc());
            let cycles = cpu.execute_instruction(self, opcode);

            // Update PPU and APU based on cycles consumed
            self.on_cpu_cycle(cycles);
        }

        self.frame_ready
    }

    /// Get the current frame buffer
    fn frame_buffer(&self) -> &FrameBuffer {
        &self.frame_buffer
    }

    /// Get the audio samples
    fn audio_samples(&self) -> &[f32] {
        &self.audio_buffer
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
        // PPU register addresses are mirrored every 8 bytes from $2000-$3FFF
        let reg = (addr & 0x07) as u8;

        match reg {
            0x02 => {
                // PPUSTATUS - a read clears VBlank, sprite overflow and sprite 0
                // hit, and resets the write toggle. Games such as Super Mario
                // Bros. spin on the sprite 0 flag while waiting for the status
                // bar split, so leaving it set stalls their timing loops.
                let status = self.ppu.status.bits();
                self.ppu.status.remove(
                    PpuStatus::VBLANK | PpuStatus::SPRITE_OVERFLOW | PpuStatus::SPRITE_0_HIT,
                );
                self.ppu.w = false;
                status
            }
            0x04 => {
                // OAMDATA - read from OAM
                self.ppu.oam[self.ppu.oam_addr as usize]
            }
            0x07 => {
                // PPUDATA - read from VRAM
                let value = self.ppu_read_buffer;
                let vram_addr = self.ppu.v & 0x3FFF;

                // Update read buffer
                if vram_addr >= 0x3F00 {
                    // Palette reads don't have the delayed read
                    self.ppu_read_buffer = self.ppu.palette[Self::palette_index(vram_addr)];
                } else {
                    self.ppu_read_buffer =
                        self.ppu.vram[self.mirroring.mirror_vram_addr(vram_addr) as usize];
                }

                // Increment VRAM address
                let increment = if self.ppu.ctrl.contains(nes_core::ppu::PpuCtrl::INCREMENT) {
                    32
                } else {
                    1
                };
                self.ppu.v = (self.ppu.v + increment) & 0x7FFF;

                value
            }
            _ => {
                // Open bus for other registers
                0
            }
        }
    }

    /// Write to PPU register
    fn write_ppu_register(&mut self, addr: u16, value: u8) {
        // PPU register addresses are mirrored every 8 bytes from $2000-$3FFF
        let reg = (addr & 0x07) as u8;

        match reg {
            0x00 => {
                // PPUCTRL
                self.ppu.ctrl = nes_core::ppu::PpuCtrl::from_bits_truncate(value);
                // Update nametable bits in temporary VRAM address
                self.ppu.t = (self.ppu.t & !0x0C00) | ((value as u16 & 0x03) << 10);
            }
            0x01 => {
                // PPUMASK
                self.ppu.mask = nes_core::ppu::PpuMask::from_bits_truncate(value);
            }
            0x03 => {
                // OAMADDR
                self.ppu.oam_addr = value;
            }
            0x04 => {
                // OAMDATA
                self.ppu.oam[self.ppu.oam_addr as usize] = value;
                self.ppu.oam_addr = self.ppu.oam_addr.wrapping_add(1);
            }
            0x05 => {
                // PPUSCROLL
                if !self.ppu.w {
                    // First write: X scroll
                    self.ppu.t = (self.ppu.t & !0x001F) | ((value as u16 >> 3) & 0x001F);
                    self.ppu.x = value & 0x07;
                } else {
                    // Second write: Y scroll
                    self.ppu.t = (self.ppu.t & !0x73E0)
                        | ((value as u16 & 0x07) << 12)
                        | (((value as u16 >> 3) & 0x1F) << 5);
                }
                self.ppu.w = !self.ppu.w;
            }
            0x06 => {
                // PPUADDR
                if !self.ppu.w {
                    // First write: high byte
                    self.ppu.t = (self.ppu.t & 0x00FF) | ((value as u16 & 0x3F) << 8);
                } else {
                    // Second write: low byte
                    self.ppu.t = (self.ppu.t & 0xFF00) | value as u16;
                    self.ppu.v = self.ppu.t;
                }
                self.ppu.w = !self.ppu.w;
            }
            0x07 => {
                // PPUDATA
                let vram_addr = self.ppu.v & 0x3FFF;

                if vram_addr >= 0x3F00 {
                    // Palette write. The sprite backdrop entries
                    // ($3F10/$3F14/$3F18/$3F1C) alias the background ones
                    // ($3F00/$3F04/$3F08/$3F0C) in both directions.
                    let palette_addr = Self::palette_index(vram_addr);
                    self.ppu.palette[palette_addr] = value;
                    if palette_addr % 4 == 0 {
                        self.ppu.palette[palette_addr + 0x10] = value;
                    }
                } else {
                    self.ppu.vram[self.mirroring.mirror_vram_addr(vram_addr) as usize] = value;
                }

                // Increment VRAM address
                let increment = if self.ppu.ctrl.contains(nes_core::ppu::PpuCtrl::INCREMENT) {
                    32
                } else {
                    1
                };
                self.ppu.v = (self.ppu.v + increment) & 0x7FFF;
            }
            _ => {}
        }
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
            _ => 0,
        }
    }

    /// Write to APU/register
    fn write_apu_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x4014 => {
                // OAMDMA - sprite DMA transfer
                // This transfers 256 bytes from CPU memory to OAM
                let source_addr = (value as u16) << 8;
                for i in 0..256 {
                    let byte = self.read(source_addr + i);
                    self.ppu.oam[self.ppu.oam_addr.wrapping_add(i as u8) as usize] = byte;
                }
                // DMA takes 513 or 514 CPU cycles (odd cycle alignment)
                // For simplicity, we'll just note this
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

    /// Generate audio samples for the given number of CPU cycles
    fn generate_audio(&mut self, cycles: u16) {
        // APU runs at CPU rate
        // For now, generate silence
        // TODO: Implement actual APU audio generation
        let _ = cycles;
    }

    /// Map a palette RAM address ($3F00-$3F1F) to its storage index.
    ///
    /// The four sprite backdrop entries at $3F10/$3F14/$3F18/$3F1C are mirrors
    /// of the background ones at $3F00/$3F04/$3F08/$3F0C, so they share storage.
    fn palette_index(addr: u16) -> usize {
        let index = (addr & 0x1F) as usize;
        if index >= 0x10 && index % 4 == 0 {
            index - 0x10
        } else {
            index
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

        match scanline {
            // Visible scanlines
            0..=239 => {
                if cycle == 256 {
                    // Reload the horizontal scroll for this line from t.
                    self.ppu.render_v =
                        (self.ppu.render_v & !0x041F) | (self.ppu.t & 0x041F);

                    // Background + sprites for this scanline.
                    let mask = self.ppu.mask;
                    self.bg_renderer
                        .render_scanline(&self.ppu, scanline, mask);
                    self.bg_renderer
                        .render_sprites_for_scanline(&self.ppu, scanline, mask);
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
            }
            // Start of VBlank
            241 => {
                if cycle == 1 {
                    self.ppu.status.insert(PpuStatus::VBLANK);
                    if self.ppu.ctrl.contains(PpuCtrl::NMI_ENABLE) {
                        self.nmi_pending = true;
                    }
                }
            }
            // Pre-render scanline
            261 => {
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
            _ => {}
        }
    }
}

impl Bus for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
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
                    0 // Open bus
                }
            }
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
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
        // Update PPU (3 PPU cycles per CPU cycle)
        let ppu_cycles = cycles as u16 * 3;

        for _ in 0..ppu_cycles {
            // Advance PPU cycle
            self.ppu.cycle += 1;

            if self.ppu.cycle > 340 {
                self.ppu.cycle = 0;
                self.ppu.scanline += 1;

                if self.ppu.scanline > 261 {
                    self.ppu.scanline = 0;
                    self.ppu.frame += 1;
                    self.frame_ready = true;

                    if let Some(ref mut mapper) = self.mapper {
                        for addr in 0..0x2000u16 {
                            self.ppu.vram[addr as usize] = mapper.read(addr);
                        }
                    }

                    self.frame_buffer.pixels.copy_from_slice(self.bg_renderer.get_frame());
                }
            }
            self.ppu_step();
        }

        // Generate audio
        self.generate_audio(cycles as u16);

        // Update CPU cycle counter
        self.cpu_cycles += cycles as u64;
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
            || self.mapper.as_ref().map_or(false, |m| m.irq_pending())
    }
}

/// WASM-exposed NES emulator
///
/// This struct wraps the NES system and provides JavaScript-accessible methods
/// for running the emulator in a web browser.
#[wasm_bindgen]
pub struct JsNes {
    /// The CPU
    cpu: Cpu,
    /// The system bus (contains PPU, APU, mapper, controllers)
    bus: NesBus,
}

#[wasm_bindgen]
impl JsNes {
    /// Create a new NES emulator instance
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        #[cfg(feature = "console_error_panic_hook")]
        console_error_panic_hook::set_once();

        let mut bus = NesBus::new();
        let mut cpu = Cpu::new();

        // Initial reset
        bus.reset(&mut cpu);

        Self { cpu, bus }
    }

    /// Load a ROM from byte data
    ///
    /// # Arguments
    /// * `data` - The ROM file data as a byte array
    ///
    /// # Returns
    /// Ok(()) on success, or an error message on failure
    #[wasm_bindgen]
    pub fn load_rom(&mut self, data: &[u8]) -> core::result::Result<(), String> {
        self.bus.load_rom(data).map_err(|e| format!("{:?}", e))?;
        self.bus.reset(&mut self.cpu);
        Ok(())
    }

    /// Execute one frame of emulation
    ///
    /// This runs the CPU for approximately one frame's worth of cycles
    /// and updates the PPU and APU accordingly.
    #[wasm_bindgen]
    pub fn frame(&mut self) {
        self.bus.run_frame(&mut self.cpu);
    }

    /// Get the current frame buffer as a Vec<u32>
    ///
    /// Returns a vector of ARGB pixels (256x240 = 61,440 pixels).
    /// Each pixel is a 32-bit value in the format 0xAARRGGBB.
    #[wasm_bindgen]
    pub fn get_frame_buffer(&self) -> Vec<u32> {
        self.bus.frame_buffer().pixels.clone()
    }

    /// Get the frame buffer pointer (for efficient JS access)
    ///
    /// Returns a pointer to the frame buffer data that can be used
    /// with JavaScript's Uint32Array.
    #[wasm_bindgen]
    pub fn get_frame_buffer_ptr(&self) -> *const u32 {
        self.bus.frame_buffer().pixels.as_ptr()
    }

    /// Get the frame buffer length in pixels
    #[wasm_bindgen]
    pub fn get_frame_buffer_len(&self) -> usize {
        self.bus.frame_buffer().pixels.len()
    }

    /// Set a button state for a player
    ///
    /// # Arguments
    /// * `player` - Player number (1 or 2)
    /// * `button` - Button index (0-7, see BUTTON_* constants)
    /// * `pressed` - true if pressed, false if released
    #[wasm_bindgen]
    pub fn set_button(&mut self, player: u8, button: u8, pressed: bool) {
        self.bus.set_button(player, button, pressed);
    }

    /// Press a button (convenience method)
    #[wasm_bindgen]
    pub fn button_down(&mut self, player: u8, button: u8) {
        self.set_button(player, button, true);
    }

    /// Release a button (convenience method)
    #[wasm_bindgen]
    pub fn button_up(&mut self, player: u8, button: u8) {
        self.set_button(player, button, false);
    }

    /// Get audio samples generated during the last frame
    ///
    /// Returns a vector of f32 audio samples in the range [-1.0, 1.0].
    /// Samples are interleaved as [left, right, left, right, ...].
    #[wasm_bindgen]
    pub fn get_audio_samples(&self) -> Vec<f32> {
        self.bus.audio_samples().to_vec()
    }

    /// Get the audio sample buffer pointer
    ///
    /// Returns a pointer to the audio buffer that can be used
    /// with JavaScript's Float32Array.
    #[wasm_bindgen]
    pub fn get_audio_samples_ptr(&self) -> *const f32 {
        self.bus.audio_samples().as_ptr()
    }

    /// Get the number of audio samples available
    #[wasm_bindgen]
    pub fn get_audio_samples_len(&self) -> usize {
        self.bus.audio_samples().len()
    }

    /// Reset the emulator
    ///
    /// This resets the CPU and system state as if the reset button
    /// was pressed on the NES console.
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.bus.reset(&mut self.cpu);
    }

    /// Get the current frame number
    #[wasm_bindgen]
    pub fn get_frame_number(&self) -> u64 {
        self.bus.ppu.frame
    }

    /// Check if a frame is ready for display
    #[wasm_bindgen]
    pub fn is_frame_ready(&self) -> bool {
        self.bus.frame_ready
    }

    /// Clear the frame ready flag
    #[wasm_bindgen]
    pub fn clear_frame_ready(&mut self) {
        self.bus.frame_ready = false;
    }

}

impl Default for JsNes {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the button constant for A button
#[wasm_bindgen]
pub fn get_button_a() -> u8 {
    BUTTON_A
}

/// Get the button constant for B button
#[wasm_bindgen]
pub fn get_button_b() -> u8 {
    BUTTON_B
}

/// Get the button constant for SELECT button
#[wasm_bindgen]
pub fn get_button_select() -> u8 {
    BUTTON_SELECT
}

/// Get the button constant for START button
#[wasm_bindgen]
pub fn get_button_start() -> u8 {
    BUTTON_START
}

/// Get the button constant for UP button
#[wasm_bindgen]
pub fn get_button_up() -> u8 {
    BUTTON_UP
}

/// Get the button constant for DOWN button
#[wasm_bindgen]
pub fn get_button_down() -> u8 {
    BUTTON_DOWN
}

/// Get the button constant for LEFT button
#[wasm_bindgen]
pub fn get_button_left() -> u8 {
    BUTTON_LEFT
}

/// Get the button constant for RIGHT button
#[wasm_bindgen]
pub fn get_button_right() -> u8 {
    BUTTON_RIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_nes_new() {
        let _nes = JsNes::new();
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
        let mut nes = JsNes::new();
        nes.bus.ppu.status.insert(
            PpuStatus::VBLANK | PpuStatus::SPRITE_OVERFLOW | PpuStatus::SPRITE_0_HIT,
        );

        let value = nes.bus.read(0x2002);

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
    #[test]
    fn smb_select_is_sampled_once_per_sixteen_frames() {
        let Ok(rom) = std::fs::read("../../roms/超级玛莉.NES") else {
            eprintln!("Skipping: SMB ROM not found");
            return;
        };

        let mut toggled = 0;
        for offset in 0..16u32 {
            let mut nes = JsNes::new();
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
            if nes.bus.read(0x077A) != 0 {
                toggled += 1;
            }
        }

        assert_eq!(
            toggled, 1,
            "exactly one of the sixteen starting phases should be accepted"
        );
    }
}
