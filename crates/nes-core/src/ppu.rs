//! PPU module - Picture Processing Unit
//!
//! The Ricoh 2C02 (NTSC) / 2C07 (PAL) handles all graphics rendering.
//! It generates a 256x240 pixel display at 60 FPS (NTSC) or 50 FPS (PAL).

use bitflags::bitflags;
pub mod rendering;
pub mod sprite;

use crate::mapper::Mapper;
pub use rendering::{BackgroundRenderer, FrameBuffer, Mirroring, ScanlineProcessor};
pub use sprite::{
    SecondaryOam, Sprite, SpriteAttributes, SpriteEvaluator, SpritePatternFetcher, SpriteRenderer,
};

/// Memory the PPU register logic needs beyond its own state.
///
/// The palette and nametables live in the PPU, but pattern tables live on the
/// cartridge and nametable mirroring is chosen by the mapper. The system bus
/// supplies those through [`PpuMemory::System`]; a standalone PPU (the unit
/// tests) keeps everything in its own flat VRAM via [`PpuMemory::Internal`].
///
/// Keeping this behind one parameter is what lets the PPU own the single copy
/// of the register state machine: both entry points drive the same code.
pub enum PpuMemory<'a> {
    /// Pattern tables and nametables both live in the PPU's flat VRAM.
    Internal,
    /// Pattern tables come from the cartridge, nametables are mirrored, and
    /// write-only registers return the CPU data bus value.
    System {
        /// Cartridge CHR source used for pattern table ($0000-$1FFF) access.
        chr: Option<&'a mut (dyn Mapper + 'static)>,
        /// Nametable mirroring selected by the mapper or cartridge header.
        mirroring: Mirroring,
        /// Value currently driven on the CPU data bus, for open-bus reads.
        open_bus: u8,
    },
}

bitflags! {
    /// PPU Control Register ($2000)
    ///
    /// Controls NMI, sprite size, pattern table selection, and VRAM increment.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PpuCtrl: u8 {
        /// Base nametable address (0: $2000, 1: $2400, 2: $2800, 3: $2C00)
        const NAMETABLE_X = 0x01;
        const NAMETABLE_Y = 0x02;
        /// VRAM address increment per CPU read/write of PPUDATA
        /// 0: add 1 (horizontal), 1: add 32 (vertical)
        const INCREMENT = 0x04;
        /// Sprite pattern table address for 8x8 sprites
        /// 0: $0000, 1: $1000 (ignored for 8x16 sprites)
        const SPRITE_PATTERN = 0x08;
        /// Background pattern table address
        /// 0: $0000, 1: $1000
        const BACKGROUND_PATTERN = 0x10;
        /// Sprite size
        /// 0: 8x8, 1: 8x16
        const SPRITE_SIZE = 0x20;
        /// PPU master/slave select
        /// 0: read from EXT, 1: write to EXT
        const MASTER_SLAVE = 0x40;
        /// Generate NMI at start of VBlank
        const NMI_ENABLE = 0x80;
    }
}

bitflags! {
    /// PPU Mask Register ($2001)
    ///
    /// Controls rendering enable, color emphasis, and clipping.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PpuMask: u8 {
        /// Grayscale mode (0: color, 1: grayscale)
        const GRAYSCALE = 0x01;
        /// Show background in leftmost 8 pixels
        const SHOW_BG_LEFT = 0x02;
        /// Show sprites in leftmost 8 pixels
        const SHOW_SPRITES_LEFT = 0x04;
        /// Show background
        const SHOW_BG = 0x08;
        /// Show sprites
        const SHOW_SPRITES = 0x10;
        /// Emphasize red (green on PAL/Dendy)
        const EMPHASIZE_RED = 0x20;
        /// Emphasize green (red on PAL/Dendy)
        const EMPHASIZE_GREEN = 0x40;
        /// Emphasize blue
        const EMPHASIZE_BLUE = 0x80;
    }
}

bitflags! {
    /// PPU Status Register ($2002)
    ///
    /// Read-only register containing PPU state flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PpuStatus: u8 {
        /// Sprite overflow flag (more than 8 sprites on a scanline)
        const SPRITE_OVERFLOW = 0x20;
        /// Sprite 0 hit flag
        const SPRITE_0_HIT = 0x40;
        /// VBlank flag (set during vertical blanking)
        const VBLANK = 0x80;
    }
}

impl crate::state::StateField for PpuCtrl {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u8(self.bits());
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        *self = PpuCtrl::from_bits_truncate(r.u8());
    }
}

impl crate::state::StateField for PpuMask {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u8(self.bits());
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        *self = PpuMask::from_bits_truncate(r.u8());
    }
}

impl crate::state::StateField for PpuStatus {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u8(self.bits());
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        *self = PpuStatus::from_bits_truncate(r.u8());
    }
}

/// PPU state
#[derive(Debug)]
pub struct Ppu {
    // VRAM and memory
    /// 16KB PPU VRAM (includes pattern tables, nametables)
    pub vram: [u8; 0x4000],
    /// 32 bytes palette RAM ($3F00-$3F1F)
    pub palette: [u8; 0x20],
    /// 256 bytes OAM (Object Attribute Memory for sprites)
    pub oam: [u8; 0x100],

    // Registers
    /// PPU control register ($2000)
    pub ctrl: PpuCtrl,
    /// PPU mask register ($2001)
    pub mask: PpuMask,
    /// PPU status register ($2002)
    pub status: PpuStatus,
    /// OAM address register ($2003)
    pub oam_addr: u8,

    // Internal registers (for scrolling and VRAM access)
    /// Current VRAM address (15 bits, tV, tH, VT, HT, FV)
    pub v: u16,
    /// Temporary VRAM address (15 bits)
    pub t: u16,
    /// Fine X scroll (3 bits)
    pub x: u8,
    /// Write toggle for $2005/$2006 (false = first write, true = second write)
    pub w: bool,
    /// Render scroll counter.
    ///
    /// The rendering pipeline keeps its own copy of the scroll position so that
    /// the CPU writing nametables through $2006/$2007 (which mutates `v`)
    /// cannot disturb an in-progress frame. It is reloaded from `t` on the
    /// pre-render scanline and advanced once per visible scanline, mirroring
    /// the cntV/cntH/cntVT counters used by the reference implementation.
    pub render_v: u16,

    // Rendering state
    /// Current scanline (0-261, where 0-239 are visible, 240 is post-render,
    /// 241-260 is VBlank, 261 is pre-render)
    pub scanline: u16,
    /// Current cycle within scanline (0-340)
    pub cycle: u16,
    /// Frame counter
    pub frame: u64,

    // Internal state
    /// NMI pending flag
    nmi_pending: bool,
    /// Internal read buffer for $2007 reads
    read_buffer: u8,
    /// Open bus latch for PPU register reads
    open_bus_latch: u8,
    /// Set when a $2002 read wins the race against the VBlank flag, so the flag
    /// and its NMI are skipped for this frame
    suppress_vblank: bool,
}

crate::state::state_fields!(Ppu {
    vram,
    palette,
    oam,
    ctrl,
    mask,
    status,
    oam_addr,
    v,
    t,
    x,
    w,
    render_v,
    scanline,
    cycle,
    frame,
    nmi_pending,
    read_buffer,
    open_bus_latch,
    suppress_vblank,
});

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

impl Ppu {
    /// Create a new PPU instance with all memory zeroed
    pub fn new() -> Self {
        Self {
            // Memory arrays are zero-initialized
            vram: [0; 0x4000],
            palette: [0; 0x20],
            oam: [0; 0x100],

            // Registers initialized to 0
            ctrl: PpuCtrl::empty(),
            mask: PpuMask::empty(),
            status: PpuStatus::empty(),
            oam_addr: 0,

            // Internal registers
            v: 0,
            t: 0,
            x: 0,
            w: false,
            render_v: 0,

            // Rendering state
            scanline: 0,
            cycle: 0,
            frame: 0,

            // Internal state
            nmi_pending: false,
            read_buffer: 0,
            open_bus_latch: 0,
            suppress_vblank: false,
        }
    }

    /// Reset the PPU to initial state
    pub fn reset(&mut self) {
        // Clear memory
        self.vram.fill(0);
        self.palette.fill(0);
        self.oam.fill(0);

        // Reset registers
        self.ctrl = PpuCtrl::empty();
        self.mask = PpuMask::empty();
        self.status = PpuStatus::empty();
        self.oam_addr = 0;

        // Reset internal registers
        self.v = 0;
        self.t = 0;
        self.x = 0;
        self.w = false;
        self.render_v = 0;

        // Reset rendering state
        self.scanline = 0;
        self.cycle = 0;
        self.frame = 0;

        // Reset internal state
        self.nmi_pending = false;
        self.suppress_vblank = false;
        self.read_buffer = 0;
        self.open_bus_latch = 0;
    }

    /// Read from a PPU register ($2000-$2007)
    ///
    /// # Arguments
    /// * `addr` - Register address (should be in $2000-$2007 range)
    ///
    /// # Returns
    /// The value read from the register
    pub fn read_register(&mut self, addr: u16) -> u8 {
        self.read_register_with(addr, PpuMemory::Internal)
    }

    /// Read from a PPU register using an explicit memory source.
    ///
    /// This is the single implementation of the register logic. The system bus
    /// calls it with [`PpuMemory::System`] so pattern tables come from the
    /// cartridge and nametables are mirrored; [`Ppu::read_register`] is the
    /// standalone form used by the unit tests.
    pub fn read_register_with(&mut self, addr: u16, mut memory: PpuMemory<'_>) -> u8 {
        match addr & 0x2007 {
            0x2002 => {
                // $2002: PPUSTATUS - read-only. A read clears VBlank, sprite
                // overflow and sprite 0 hit and resets the write toggle. Games
                // such as Super Mario Bros. spin on the sprite 0 flag while
                // waiting for the status bar split, so leaving it set stalls
                // their timing loops. Bits 0-4 are not driven and read back as
                // open bus.
                //
                // A read landing on the dot the VBlank flag is about to be set
                // wins that race: the flag never appears for this frame, and
                // neither does the NMI it would have raised. This is what the
                // suppression tests in blargg's suite are about, and games use
                // it to hold back the NMI for a frame.
                // The whole CPU step is three dots wide, so a read that lands
                // anywhere in the step containing the flag's own dot is racing
                // it. blargg's `02-vbl_set_time` is what pins this window down.
                if self.scanline == 241 && self.cycle <= 1 {
                    self.suppress_vblank = true;
                }
                let status = self.status.bits();
                self.status.remove(
                    PpuStatus::VBLANK | PpuStatus::SPRITE_OVERFLOW | PpuStatus::SPRITE_0_HIT,
                );
                self.w = false;
                let value = (status & 0xE0) | (self.open_bus(&memory) & 0x1F);
                self.open_bus_latch = value;
                value
            }
            0x2004 => {
                // $2004: OAMDATA - read OAM. Attribute bytes (every 4th byte
                // from offset 2) keep only the implemented bits, which is
                // invisible to the sprite renderer but matches hardware reads.
                let value = self.oam[self.oam_addr as usize];
                let value = if (self.oam_addr & 3) == 2 {
                    value & 0xE3
                } else {
                    value
                };
                self.open_bus_latch = value;
                value
            }
            0x2007 => {
                // $2007: PPUDATA - read VRAM
                self.read_ppudata(&mut memory)
            }
            // $2000/$2001/$2003/$2005/$2006 are write-only or unused and read
            // back as open bus.
            _ => self.open_bus(&memory),
        }
    }

    /// Write to a PPU register ($2000-$2007)
    ///
    /// # Arguments
    /// * `addr` - Register address (should be in $2000-$2007 range)
    /// * `value` - Value to write
    pub fn write_register(&mut self, addr: u16, value: u8) {
        self.write_register_with(addr, value, PpuMemory::Internal)
    }

    /// Write to a PPU register using an explicit memory source.
    ///
    /// See [`Ppu::read_register_with`] for why the memory source is a
    /// parameter rather than state owned by the PPU.
    pub fn write_register_with(&mut self, addr: u16, value: u8, mut memory: PpuMemory<'_>) {
        // Update open bus latch on all writes
        self.open_bus_latch = value;

        match addr & 0x2007 {
            0x2000 => {
                // $2000: PPUCTRL
                self.write_ppuctrl(value);
            }
            0x2001 => {
                // $2001: PPUMASK
                self.write_ppumask(value);
            }
            0x2002 => {
                // $2002: PPUSTATUS - read-only, writes are ignored
            }
            0x2003 => {
                // $2003: OAMADDR
                self.oam_addr = value;
            }
            0x2004 => {
                // $2004: OAMDATA
                self.oam[self.oam_addr as usize] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            0x2005 => {
                // $2005: PPUSCROLL
                self.write_ppuscroll(value);
            }
            0x2006 => {
                // $2006: PPUADDR
                self.write_ppuaddr(value);
            }
            0x2007 => {
                // $2007: PPUDATA
                self.write_ppudata(value, &mut memory);
            }
            _ => {
                // Should not happen due to mask
            }
        }
    }

    /// Write to PPUCTRL ($2000)
    fn write_ppuctrl(&mut self, value: u8) {
        let nmi_was_enabled = self.ctrl.contains(PpuCtrl::NMI_ENABLE);
        self.ctrl = PpuCtrl::from_bits_truncate(value);

        // Update temporary VRAM address bits 10-11 (nametable select)
        // t: ....BA.. ........ = d: ......BA
        self.t = (self.t & !0x0C00) | ((value as u16 & 0x03) << 10);

        // The NMI line follows "NMI enabled AND VBlank flag set" while it is
        // set, so enabling it in the middle of vertical blanking raises the
        // interrupt at once rather than waiting for the next frame. Disabling
        // it takes the request back.
        let nmi_enabled = self.ctrl.contains(PpuCtrl::NMI_ENABLE);
        let vblank = self.status.contains(PpuStatus::VBLANK);
        if nmi_enabled && vblank && !nmi_was_enabled {
            self.nmi_pending = true;
        } else if !nmi_enabled && self.nmi_pending {
            self.nmi_pending = false;
        }
    }

    /// Write to PPUMASK ($2001)
    fn write_ppumask(&mut self, value: u8) {
        self.mask = PpuMask::from_bits_truncate(value);
    }

    /// Write to PPUSCROLL ($2005)
    fn write_ppuscroll(&mut self, value: u8) {
        if !self.w {
            // First write: X scroll
            // t: ........ ...HGFED = d: HGFED...
            // x:               CBA = d: .....CBA
            // w:                   = 1
            self.t = (self.t & !0x001F) | ((value as u16 >> 3) & 0x001F);
            self.x = value & 0x07;
            self.w = true;
        } else {
            // Second write: Y scroll
            // t: CBA..HG FED..... = d: HGFEDCBA
            // w:                   = 0
            self.t =
                (self.t & !0x73E0) | ((value as u16 & 0x07) << 12) | ((value as u16 & 0xF8) << 2);
            self.w = false;
        }
    }

    /// Write to PPUADDR ($2006)
    fn write_ppuaddr(&mut self, value: u8) {
        if !self.w {
            // First write: high byte
            // t: .FEDCBA ........ = d: ..FEDCBA
            // t: X...... ........ = 0
            // w:                   = 1
            self.t = (self.t & !0x7F00) | ((value as u16 & 0x3F) << 8);
            self.w = true;
        } else {
            // Second write: low byte
            // t: ........ HGFEDCBA = d: HGFEDCBA
            // v                    = t
            // w:                   = 0
            self.t = (self.t & !0x00FF) | (value as u16);
            self.v = self.t;
            self.w = false;
        }
    }

    /// Read from PPUDATA ($2007)
    fn read_ppudata(&mut self, memory: &mut PpuMemory<'_>) -> u8 {
        let addr = self.v & 0x3FFF;
        let result;

        if addr < 0x3F00 {
            // Pattern tables and nametables: buffered read
            result = self.read_buffer;
            self.read_buffer = self.read_vram(addr, memory);
        } else {
            // Palette RAM: returned immediately, but the buffer still picks up
            // the nametable data "behind" the palette.
            result = self.palette[Self::palette_index(addr)];
            self.read_buffer = self.read_vram(addr & 0x2FFF, memory);
        }

        // Increment VRAM address
        self.increment_vram_addr();

        result
    }

    /// Write to PPUDATA ($2007)
    fn write_ppudata(&mut self, value: u8, memory: &mut PpuMemory<'_>) {
        let addr = self.v & 0x3FFF;

        if addr >= 0x3F00 {
            // Palette RAM stores 6 bits. The four sprite backdrop entries
            // ($3F10/$3F14/$3F18/$3F1C) alias the background ones, so they
            // share storage.
            let palette_addr = Self::palette_index(addr);
            self.palette[palette_addr] = value & 0x3F;
            if palette_addr % 4 == 0 {
                self.palette[palette_addr + 0x10] = value & 0x3F;
            }
        } else {
            self.write_vram(addr, value, memory);
        }

        // Increment VRAM address
        self.increment_vram_addr();
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

    /// Open-bus value returned by write-only or unused register reads.
    fn open_bus(&self, memory: &PpuMemory<'_>) -> u8 {
        match memory {
            PpuMemory::Internal => self.open_bus_latch,
            PpuMemory::System { open_bus, .. } => *open_bus,
        }
    }

    /// Read from VRAM at the given address
    fn read_vram(&self, addr: u16, memory: &mut PpuMemory<'_>) -> u8 {
        let addr = addr & 0x3FFF;

        if addr < 0x2000 {
            // Pattern tables ($0000-$1FFF): cartridge CHR in the system
            // configuration, flat VRAM for a standalone PPU.
            match memory {
                PpuMemory::Internal => self.vram[addr as usize],
                PpuMemory::System { chr, .. } => match chr {
                    Some(mapper) => mapper.read(addr),
                    None => 0,
                },
            }
        } else if addr < 0x3F00 {
            // Nametables ($2000-$2FFF): a cartridge can point a window at its
            // own RAM (MMC5's ExRAM), and answers first when it does.
            if let PpuMemory::System { chr: Some(mapper), .. } = memory {
                if let Some(byte) = mapper.nametable_read(addr) {
                    return byte;
                }
            }
            // Otherwise mirroring is chosen by the mapper.
            let mirrored = match memory {
                PpuMemory::Internal => addr,
                PpuMemory::System { mirroring, .. } => mirroring.mirror_vram_addr(addr),
            };
            self.vram[mirrored as usize]
        } else {
            // Palette RAM ($3F00-$3FFF)
            self.palette[Self::palette_index(addr)]
        }
    }

    /// Write to VRAM at the given address
    fn write_vram(&mut self, addr: u16, value: u8, memory: &mut PpuMemory<'_>) {
        let addr = addr & 0x3FFF;

        if addr < 0x2000 {
            // Pattern tables ($0000-$1FFF): CHR-RAM uploads must land on the
            // cartridge so the renderer sees them.
            match memory {
                PpuMemory::Internal => self.vram[addr as usize] = value,
                PpuMemory::System { chr, .. } => {
                    if let Some(mapper) = chr {
                        mapper.write(addr, value);
                    }
                }
            }
        } else if addr < 0x3F00 {
            // Nametables ($2000-$2FFF): a cartridge that maps a window to its
            // own RAM takes the write instead.
            if let PpuMemory::System { chr: Some(mapper), .. } = memory {
                if mapper.nametable_write(addr, value) {
                    return;
                }
            }
            let mirrored = match memory {
                PpuMemory::Internal => addr,
                PpuMemory::System { mirroring, .. } => mirroring.mirror_vram_addr(addr),
            };
            self.vram[mirrored as usize] = value;
        }
        // Palette handled separately in write_ppudata
    }

    /// Increment VRAM address after $2007 read/write
    fn increment_vram_addr(&mut self) {
        let increment = if self.ctrl.contains(PpuCtrl::INCREMENT) {
            32
        } else {
            1
        };
        self.v = self.v.wrapping_add(increment) & 0x7FFF;
    }

    /// Check if NMI is pending
    pub fn nmi_pending(&self) -> bool {
        self.nmi_pending
    }

    /// Clear NMI pending flag
    pub fn clear_nmi(&mut self) {
        self.nmi_pending = false;
    }

    /// Get current scanline
    pub fn scanline(&self) -> u16 {
        self.scanline
    }

    /// Get current cycle
    pub fn cycle(&self) -> u16 {
        self.cycle
    }

    /// Get current frame number
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Take the "a $2002 read won the VBlank race" flag, if it was set.
    ///
    /// The bus calls this on the dot the flag would otherwise appear, so the
    /// suppression lasts exactly the frame it was made for.
    pub fn take_vblank_suppression(&mut self) -> bool {
        core::mem::take(&mut self.suppress_vblank)
    }

    /// Step the PPU by one dot (1/3 CPU cycle)
    pub fn step(&mut self) {
        // TODO: Implement PPU rendering logic
        self.cycle += 1;
        if self.cycle > 340 {
            self.cycle = 0;
            self.scanline += 1;
            if self.scanline > 261 {
                self.scanline = 0;
                self.frame += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let ppu = Ppu::new();

        // Check memory is zeroed
        assert!(ppu.vram.iter().all(|&x| x == 0));
        assert!(ppu.palette.iter().all(|&x| x == 0));
        assert!(ppu.oam.iter().all(|&x| x == 0));

        // Check registers are initialized
        assert_eq!(ppu.ctrl.bits(), 0);
        assert_eq!(ppu.mask.bits(), 0);
        assert_eq!(ppu.status.bits(), 0);
        assert_eq!(ppu.oam_addr, 0);

        // Check internal registers
        assert_eq!(ppu.v, 0);
        assert_eq!(ppu.t, 0);
        assert_eq!(ppu.x, 0);
        assert!(!ppu.w);

        // Check rendering state
        assert_eq!(ppu.scanline, 0);
        assert_eq!(ppu.cycle, 0);
        assert_eq!(ppu.frame, 0);
    }

    /// A $2002 read landing on the VBlank flag's own dot takes the frame's flag
    /// and NMI away, and the suppression is consumed exactly once.
    #[test]
    fn test_ppustatus_read_wins_the_vblank_race() {
        let mut ppu = Ppu::new();
        ppu.ctrl = PpuCtrl::NMI_ENABLE;
        ppu.scanline = 241;
        ppu.cycle = 1;

        let value = ppu.read_register(0x2002);
        assert_eq!(value & 0x80, 0, "the flag is not visible on the dot it is set");
        assert!(ppu.take_vblank_suppression(), "the frame's VBlank is suppressed");
        assert!(!ppu.take_vblank_suppression(), "and only for that one frame");

        // A read anywhere else behaves as an ordinary status read.
        ppu.scanline = 241;
        ppu.cycle = 100;
        ppu.read_register(0x2002);
        assert!(!ppu.take_vblank_suppression());
    }

    #[test]
    fn test_reset() {
        let mut ppu = Ppu::new();

        // Modify some state
        ppu.vram[0] = 0xFF;
        ppu.palette[0] = 0x3F;
        ppu.oam[0] = 0xAA;
        ppu.ctrl = PpuCtrl::NMI_ENABLE;
        ppu.mask = PpuMask::SHOW_BG;
        ppu.status = PpuStatus::VBLANK;
        ppu.v = 0x7FFF;
        ppu.scanline = 100;

        // Reset
        ppu.reset();

        // Check memory is cleared
        assert_eq!(ppu.vram[0], 0);
        assert_eq!(ppu.palette[0], 0);
        assert_eq!(ppu.oam[0], 0);

        // Check registers are reset
        assert_eq!(ppu.ctrl.bits(), 0);
        assert_eq!(ppu.mask.bits(), 0);
        assert_eq!(ppu.status.bits(), 0);
        assert_eq!(ppu.v, 0);
        assert_eq!(ppu.scanline, 0);
    }

    #[test]
    fn test_registers() {
        let mut ppu = Ppu::new();

        // Test $2000 (PPUCTRL) write
        ppu.write_register(0x2000, 0x80); // NMI enable
        assert!(ppu.ctrl.contains(PpuCtrl::NMI_ENABLE));
        assert_eq!(ppu.open_bus_latch, 0x80);

        // Test $2000 read (write-only, returns open bus)
        let val = ppu.read_register(0x2000);
        assert_eq!(val, 0x80);

        // Test $2001 (PPUMASK) write
        ppu.write_register(0x2001, 0x08); // Show background
        assert!(ppu.mask.contains(PpuMask::SHOW_BG));

        // Test $2002 (PPUSTATUS) read
        ppu.status = PpuStatus::VBLANK;
        let status = ppu.read_register(0x2002);
        assert!(status & 0x80 != 0); // VBlank flag set
        assert!(!ppu.status.contains(PpuStatus::VBLANK)); // Cleared after read
        assert!(!ppu.w); // Write toggle cleared

        // Test $2003 (OAMADDR) write
        ppu.write_register(0x2003, 0x10);
        assert_eq!(ppu.oam_addr, 0x10);

        // Test $2004 (OAMDATA) write and read
        ppu.write_register(0x2004, 0xAB);
        assert_eq!(ppu.oam[0x10], 0xAB);
        assert_eq!(ppu.oam_addr, 0x11);

        ppu.oam_addr = 0x10;
        let oam_val = ppu.read_register(0x2004);
        assert_eq!(oam_val, 0xAB);

        // Test $2005 (PPUSCROLL) - first write
        ppu.write_register(0x2005, 0x3F); // X scroll = 7 (fine), coarse X = 7
        assert_eq!(ppu.x, 7);
        assert_eq!(ppu.t & 0x1F, 7); // coarse X
        assert!(ppu.w);

        // Test $2005 (PPUSCROLL) - second write
        ppu.write_register(0x2005, 0x4A); // Y scroll
        assert!(!ppu.w);

        // Test $2006 (PPUADDR) - first write
        ppu.write_register(0x2006, 0x20); // High byte
        assert!(ppu.w);

        // Test $2006 (PPUADDR) - second write
        ppu.write_register(0x2006, 0x00); // Low byte
        assert!(!ppu.w);
        assert_eq!(ppu.v, 0x2000);

        // Test $2007 (PPUDATA) write
        ppu.write_register(0x2007, 0x55);
        assert_eq!(ppu.vram[0x2000], 0x55);
        // Address should increment by 1 (default)
        assert_eq!(ppu.v, 0x2001);

        // Test $2007 (PPUDATA) read
        ppu.v = 0x2000;
        let data = ppu.read_register(0x2007);
        // First read returns buffer (0), buffer gets actual value
        assert_eq!(data, 0);
        // Second read returns previous buffer value
        let data2 = ppu.read_register(0x2007);
        assert_eq!(data2, 0x55);
    }

    #[test]
    fn test_ppuctrl_nametable() {
        let mut ppu = Ppu::new();

        // Test nametable selection via PPUCTRL
        ppu.write_register(0x2000, 0x00); // Nametable 0 ($2000)
        assert_eq!(ppu.t & 0x0C00, 0x0000);

        ppu.write_register(0x2000, 0x01); // Nametable 1 ($2400)
        assert_eq!(ppu.t & 0x0C00, 0x0400);

        ppu.write_register(0x2000, 0x02); // Nametable 2 ($2800)
        assert_eq!(ppu.t & 0x0C00, 0x0800);

        ppu.write_register(0x2000, 0x03); // Nametable 3 ($2C00)
        assert_eq!(ppu.t & 0x0C00, 0x0C00);
    }

    #[test]
    fn test_vram_increment() {
        let mut ppu = Ppu::new();

        // Set VRAM address to $2000
        ppu.write_register(0x2006, 0x20);
        ppu.write_register(0x2006, 0x00);
        assert_eq!(ppu.v, 0x2000);

        // Write with increment by 1 (default)
        ppu.write_register(0x2007, 0xAA);
        assert_eq!(ppu.v, 0x2001);

        // Enable increment by 32
        ppu.write_register(0x2000, 0x04);
        ppu.write_register(0x2007, 0xBB);
        assert_eq!(ppu.v, 0x2021); // +32
    }

    #[test]
    fn test_palette_write() {
        let mut ppu = Ppu::new();

        // Set VRAM address to palette area ($3F00)
        ppu.write_register(0x2006, 0x3F);
        ppu.write_register(0x2006, 0x00);

        // Write to palette (only 6 bits are stored)
        ppu.write_register(0x2007, 0xFF);
        assert_eq!(ppu.palette[0], 0x3F); // Masked to 6 bits

        // Write to $3F10 (should mirror to $3F00 for backdrop)
        // Note: full mirroring logic may be implemented separately
    }

    #[test]
    fn test_oam_attribute_bits() {
        let mut ppu = Ppu::new();

        // Write to OAM address 0x02 (first sprite's attribute byte)
        ppu.write_register(0x2003, 0x02);
        ppu.write_register(0x2004, 0xFF);

        // Read back - bits 2-4 should be clear
        ppu.write_register(0x2003, 0x02);
        let val = ppu.read_register(0x2004);
        assert_eq!(val, 0xE3); // 0xFF & 0xE3
    }
}
