//! PPU module - Picture Processing Unit
//!
//! The Ricoh 2C02 (NTSC) / 2C07 (PAL) handles all graphics rendering.
//! It generates a 256x240 pixel display at 60 FPS (NTSC) or 50 FPS (PAL).

use bitflags::bitflags;
pub mod pipeline;
pub mod rendering;
pub mod sprite;

use crate::mapper::Mapper;
pub use rendering::{FrameBuffer, Mirroring};
pub use sprite::{SecondaryOam, Sprite, SpriteAttributes, SpriteEvaluator};

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
        /// Whether the PPU is on a line it draws (a visible line, or the
        /// pre-render line, which Mesen counts as `_scanline == -1`).
        ///
        /// The register logic needs it: a `$2007` access on such a line
        /// advances the VRAM address in the chip's odd way.
        drawing_line: bool,
    },
}

impl PpuMemory<'_> {
    /// Whether the PPU is on a line it draws.
    ///
    /// The register logic reads it twice: a `$2007` access on such a line (with
    /// rendering on) advances the *scroll* instead of the address, and a
    /// `$2007` write stores the address' low byte instead of the value. Both
    /// decisions belong to the dot the access actually lands on, which is five
    /// dots after the CPU's cycle — so the flag travels with the memory source
    /// rather than being latched by the register write (Mesen reads
    /// `_scanline`/`IsRenderingEnabled()` at the delayed update, not at the
    /// write).
    pub fn drawing_line(&self) -> bool {
        match self {
            PpuMemory::System { drawing_line, .. } => *drawing_line,
            PpuMemory::Internal => false,
        }
    }
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
    /// The PPU's own NMI output latch.
    ///
    /// Set when a `$2000` write enables NMI underneath a VBlank flag that is
    /// already set, and cleared when it is disabled. The console's interrupt
    /// latch is the bus's, not this one: the CPU samples the line the flag and
    /// the enable make up (`NesBus::nmi_pending`), so this flag only serves the
    /// standalone PPU and its `Bus` implementations in tests and benchmarks.
    /// It does *not* follow the flag being set at the start of vertical
    /// blanking.
    nmi_pending: bool,
    /// Internal read buffer for $2007 reads
    read_buffer: u8,
    /// Open bus latch for PPU register reads
    open_bus_latch: u8,
    /// PPU dots left in which a `$2007` read is ignored, counted down by the
    /// machine once per dot (Mesen's `_ignoreVramRead`).
    ignore_vram_read: u8,
    /// The frame each bit of the open bus was last driven on.
    ///
    /// The data bus is not a latch with infinite memory: a bit the chip drove
    /// only survives about three frames, which is what blargg's `ppu_open_bus`
    /// measures. Mesen keeps the same per-bit stamp (`_openBusDecayStamp`).
    open_bus_decay_stamp: [u64; 8],
    /// Set when a $2002 read wins the race against the VBlank flag, so the flag
    /// and its NMI are skipped for this frame
    suppress_vblank: bool,
    /// Dots left before a `$2007` read's buffer refills, and the address it
    /// refills from.
    ///
    /// The byte a `$2007` read hands back is the buffer as it stands; the chip
    /// does not read memory to replace it until five dots later (Mesen's
    /// `_ppuMemoryDataReadStateMachine`). The address is *not* `v` when the
    /// refill runs: `v` moves a dot after the refill, so latching it here is
    /// what makes a read return the byte the address held at the access.
    ppudata_read_delay: u8,
    ppudata_read_address: u16,
    /// Dots left before a `$2007` write lands, the byte waiting for them, and
    /// the address it lands on (Mesen's `_ppuMemoryDataWriteStateMachine`,
    /// `_ppuMemoryDataWriteLatch`).
    ppudata_write_delay: u8,
    ppudata_write_latch: u8,
    ppudata_write_address: u16,
    /// Set when a delayed `$2007` access wants `v` moved on the *next* dot
    /// (Mesen's `_needVideoRamIncrement`).
    ///
    /// This is why a `$2007` access is not simply "write, then increment": the
    /// chip shows the byte at the address being touched for one dot before the
    /// address moves, which `full_palette` draws as a one-pixel glitch.
    need_vram_increment: bool,
    /// Dots left before a `$2006` write moves `v`, and the address it moves it
    /// to (Mesen's `_updateVramAddrDelay`/`_updateVramAddr`).
    ///
    /// Only the *second* write to `$2006` updates the address `$2007` uses, and
    /// it does so three dots later — which is what moves the instant A12 rises
    /// for a scanline counter (blargg's `3-A12_clocking`).
    vram_addr_delay: u8,
    vram_addr_update: u16,
    /// The bus address a delayed access wants to put in front of the cartridge
    /// on the dot it lands on (Mesen's `SetBusAddress`).
    ///
    /// Transient by construction: the machine takes it on the same dot it is
    /// set and hands it to the mapper, so it never survives into a save state.
    pending_bus_address: Option<u16>,
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
    ignore_vram_read,
    open_bus_decay_stamp,
    suppress_vblank,
    ppudata_read_delay,
    ppudata_read_address,
    ppudata_write_delay,
    ppudata_write_latch,
    ppudata_write_address,
    need_vram_increment,
    vram_addr_delay,
    vram_addr_update,
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
            ignore_vram_read: 0,
            open_bus_decay_stamp: [0; 8],
            ppudata_read_delay: 0,
            ppudata_read_address: 0,
            ppudata_write_delay: 0,
            ppudata_write_latch: 0,
            ppudata_write_address: 0,
            need_vram_increment: false,
            vram_addr_delay: 0,
            vram_addr_update: 0,
            pending_bus_address: None,
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
        self.ignore_vram_read = 0;
        self.open_bus_decay_stamp = [0; 8];
        self.clear_delayed_state();
    }

    /// Drop the `$2007`/`$2006` delays and anything they were about to do.
    ///
    /// A reset takes the chip's pending work with it: a `$2007` write that had
    /// not landed yet does not land after the reset, which is what Mesen's
    /// `Reset` does by zeroing the same counters.
    fn clear_delayed_state(&mut self) {
        self.ppudata_read_delay = 0;
        self.ppudata_write_delay = 0;
        self.ppudata_write_latch = 0;
        self.need_vram_increment = false;
        self.vram_addr_delay = 0;
        self.pending_bus_address = None;
    }

    /// A soft reset — the reset button (`NesPpu::Reset(true)`).
    ///
    /// The registers and the frame's position go back to their power-on
    /// values; the memories do not. VRAM, palette RAM and OAM keep what the
    /// program left in them, and so does the scroll's `v`, which hardware only
    /// clears at power-on. The status flags survive too: only a power-on reset
    /// clears those.
    pub fn soft_reset(&mut self) {
        self.ctrl = PpuCtrl::empty();
        self.mask = PpuMask::empty();

        self.oam_addr = 0;
        self.t = 0;
        self.x = 0;
        self.w = false;

        self.scanline = 0;
        self.cycle = 0;

        self.nmi_pending = false;
        self.suppress_vblank = false;
        self.read_buffer = 0;
        self.open_bus_latch = 0;
        self.ignore_vram_read = 0;
        self.open_bus_decay_stamp = [0; 8];
        self.clear_delayed_state();
    }

    /// Read from a PPU register ($2000-$2007)
    ///
    /// This is the standalone form, for the unit tests: it runs the chip's
    /// delayed state machines to completion before returning (`$2006`'s three
    /// dots and `$2007`'s five, plus the increment behind them), so a caller can
    /// touch a register and read back what it did without driving dots itself.
    /// The machine uses [`Ppu::read_register_with`] and leaves the delays to the
    /// dots — `NesBus` ticks them once per dot, as the chip does. The six-dot
    /// `$2007` window is *not* run down here, so a test can still watch it.
    ///
    /// # Arguments
    /// * `addr` - Register address (should be in $2000-$2007 range)
    ///
    /// # Returns
    /// The value read from the register
    pub fn read_register(&mut self, addr: u16) -> u8 {
        let value = self.read_register_with(addr, PpuMemory::Internal);
        self.settle_delayed_state();
        value
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
                // $2002: PPUSTATUS - read-only. A read clears VBlank and resets
                // the write toggle. Games such as Super Mario Bros. spin on the
                // sprite 0 flag while waiting for the status bar split, so
                // leaving the toggle set stalls their timing loops. Bits 0-4 are
                // not driven and read back as open bus.
                //
                // Sprite overflow and sprite 0 hit are *not* touched here: they
                // belong to the frame that set them and are cleared at
                // pre-render, which is Mesen's `UpdateStatusFlag` - a read
                // clears the VBlank flag and the CPU's NMI latch, nothing else.
                // Taking them away on a read instead cuts their life short, and
                // a test that reads the flag twice sees a different chip.
                //
                // A read landing on the dot just before the one the VBlank flag
                // is set on wins that race: the flag never appears for this
                // frame, and neither does the NMI it would have raised. This is
                // what the suppression tests in blargg's suite are about, and
                // games use it to hold back the NMI for a frame.
                //
                // The machine places the read two dots into its cycle, so when
                // the bus asks, the PPU is standing on dot 0 of the scanline -
                // whose successor is the dot the flag is set on. blargg's
                // `02-vbl_set_time` and `06-suppression` are what pin this
                // window down.
                if self.scanline == 241 && self.cycle == 0 {
                    self.suppress_vblank = true;
                }
                let status = self.status.bits();
                self.status.remove(PpuStatus::VBLANK);
                self.w = false;
                let value = (status & 0xE0) | (self.open_bus(&memory) & 0x1F);
                self.drive_open_bus_bits(value, 0xE0);
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
                self.drive_open_bus(value);
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
    /// The standalone form, with the same convenience as
    /// [`Ppu::read_register`]: the delays are run to completion before it
    /// returns.
    ///
    /// # Arguments
    /// * `addr` - Register address (should be in $2000-$2007 range)
    /// * `value` - Value to write
    pub fn write_register(&mut self, addr: u16, value: u8) {
        self.write_register_with(addr, value, PpuMemory::Internal);
        self.settle_delayed_state();
    }

    /// Run the delayed register state machines to completion.
    ///
    /// Long enough for the longest chain: a `$2007` write lands five dots in
    /// and asks for the increment a dot later.
    fn settle_delayed_state(&mut self) {
        let window = self.ignore_vram_read;
        for _ in 0..8 {
            self.tick_delayed_state(&mut PpuMemory::Internal);
        }
        // The delay runs out here; the six-dot back-to-back window does not, so
        // a test can still walk it.
        self.ignore_vram_read = window;
        self.pending_bus_address = None;
    }

    /// Write to a PPU register using an explicit memory source.
    ///
    /// No register write touches memory any more: `$2007`'s write is latched
    /// here and lands five dots later, in [`Ppu::tick_delayed_state`], which is
    /// where the memory source is needed. Kept as a `_with` twin of
    /// [`Ppu::read_register_with`] so the two entry points stay symmetrical.
    pub fn write_register_with(&mut self, addr: u16, value: u8, _memory: PpuMemory<'_>) {
        // Every write has the chip drive the data bus, which is what puts the
        // value there for a later open-bus read.
        self.drive_open_bus(value);

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
                self.write_ppudata(value);
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
            // w:                   = 0
            self.t = (self.t & !0x00FF) | (value as u16);
            self.w = false;
            // `v` follows `t` three dots later, not here (`_updateVramAddrDelay`).
            // The gap is visible: an access the program slips into it still
            // reads the previous address, and a scanline counter watching A12
            // sees the address arrive three dots late.
            self.vram_addr_update = self.t;
            self.vram_addr_delay = Self::VRAM_ADDR_DELAY_DOTS;
        }
    }

    /// Read from PPUDATA ($2007)
    fn read_ppudata(&mut self, memory: &mut PpuMemory<'_>) -> u8 {
        // Two `$2007` reads in quick succession (within six dots) are one read
        // too many for the chip: the second is ignored and hands back the open
        // bus. Mesen spells the window out as `_ignoreVramRead = 6`, a countdown
        // of PPU cycles.
        if self.ignore_vram_read > 0 {
            return self.open_bus(memory);
        }

        let addr = self.v & 0x3FFF;
        let result;

        // What the read hands back is the buffer; the memory read that replaces
        // it does not begin until the CPU's cycle has ended and two more PPU
        // dots have gone by, and the address it uses is this one — `v` only
        // moves after the refill (Mesen's `_ppuMemoryDataReadStateMachine` and
        // `_needVideoRamIncrement`). Reading memory here instead would be the
        // same answer in a non-rendering loop and a different one whenever the
        // program touches `v` or the cartridge's CHR in the gap.
        self.ppudata_read_address = addr;
        self.ppudata_read_delay = Self::PPUDATA_DELAY_DOTS;

        if addr < 0x3F00 {
            result = self.read_buffer;
            // The chip drives the byte it hands back.
            self.drive_open_bus(result);
        } else {
            // Palette RAM: returned immediately, while the buffer's refill — the
            // nametable data "behind" the palette — waits with every other read.
            // Palette entries are six bits wide, so the top two bits of the byte
            // the read returns are not driven at all: they are whatever the data
            // bus was holding. "Note: When grayscale is turned on, the read
            // values also have the grayscale mask applied to them" (Mesen's
            // `ReadRegister`), so the palette read runs through the same mask
            // the drawn pixels do.
            result = (self.palette[Self::palette_index(addr)] & self.palette_ram_mask())
                | (self.open_bus(memory) & 0xC0);
            self.drive_open_bus_bits(result, 0x3F);
        }

        // Back-to-back accesses from here on are one too many.
        self.ignore_vram_read = Self::IGNORE_VRAM_READ_DOTS;
        result
    }

    /// The mask a palette value goes through on its way to the screen: grayscale
    /// clamps it to the grey column, which is a mask of $30 (Mesen's
    /// `_paletteRamMask`).
    fn palette_ram_mask(&self) -> u8 {
        if self.mask.contains(PpuMask::GRAYSCALE) {
            0x30
        } else {
            0x3F
        }
    }

    /// PPU dots a second `$2007` access is ignored for.
    const IGNORE_VRAM_READ_DOTS: u8 = 6;

    /// PPU dots a `$2007` access spends waiting for its memory access (Mesen's
    /// `_ppuMemoryDataReadStateMachine`/`_ppuMemoryDataWriteStateMachine`, both
    /// five).
    const PPUDATA_DELAY_DOTS: u8 = 5;

    /// PPU dots a `$2006` write spends before it moves `v` (Mesen's
    /// `_updateVramAddrDelay`).
    const VRAM_ADDR_DELAY_DOTS: u8 = 3;

    /// Count one PPU dot off the `$2007`-access window, which the machine calls
    /// once per dot.
    pub fn tick_vram_access_window(&mut self) {
        self.ignore_vram_read = self.ignore_vram_read.saturating_sub(1);
    }

    /// One dot's worth of the register state machines (Mesen's `UpdateState`,
    /// which runs at the tail of every dot).
    ///
    /// The order is Mesen's, and it matters: a `$2006` update lands first, then
    /// an increment a previous access asked for, then a read's buffer refill,
    /// then a write. An increment and a refill can both be due on the same dot,
    /// and the refill must read the address the increment has just left behind.
    pub fn tick_delayed_state(&mut self, memory: &mut PpuMemory<'_>) {
        if self.vram_addr_delay > 0 {
            self.vram_addr_delay -= 1;
            if self.vram_addr_delay == 0 {
                self.v = self.vram_addr_update;
                // The bugs a straddled increment produces corrupt both
                // registers, which is Mesen's "the new value of V back into T".
                self.t = self.v;
                if !memory.drawing_line() || !self.is_rendering() {
                    // The address only reaches the cartridge when the fetches
                    // are not already holding the bus, which is why a `$2006`
                    // write during rendering does not clock a scanline counter.
                    self.pending_bus_address = Some(self.v & 0x3FFF);
                }
            }
        }

        // Mesen counts the `$2007` window down between the `$2006` update and
        // the increment; the position only matters in that the dot an access
        // happens on counts, so a second access on the dot after it is still
        // inside the window.
        self.ignore_vram_read = self.ignore_vram_read.saturating_sub(1);

        if self.need_vram_increment {
            self.need_vram_increment = false;
            self.increment_vram_addr(memory);
        }

        if self.ppudata_read_delay > 0 {
            self.ppudata_read_delay -= 1;
            if self.ppudata_read_delay == 0 {
                // A palette read buffers the nametable data behind the palette.
                let addr = self.ppudata_read_address;
                let addr = if addr >= 0x3F00 { addr & 0x2FFF } else { addr };
                self.read_buffer = self.read_vram(addr, memory);
                // The chip's memory read drives its address bus (`SetBusAddress`
                // inside `ReadVram`), which is the other half of what a scanline
                // counter watches for.
                self.pending_bus_address = Some(self.ppudata_read_address);
                self.need_vram_increment = true;
            }
        }

        if self.ppudata_write_delay > 0 {
            self.ppudata_write_delay -= 1;
            if self.ppudata_write_delay == 0 {
                self.land_ppudata_write(memory);
                self.need_vram_increment = true;
            }
        }
    }

    /// Take the address a delayed access wants to show the cartridge.
    ///
    /// The machine calls this on every dot and hands what it finds to the
    /// mapper, which is where a scanline counter's A12 comes from.
    pub fn take_pending_bus_address(&mut self) -> Option<u16> {
        self.pending_bus_address.take()
    }

    /// Whether either rendering unit is on (Mesen's `IsRenderingEnabled`).
    fn is_rendering(&self) -> bool {
        self.mask
            .intersects(PpuMask::SHOW_BG | PpuMask::SHOW_SPRITES)
    }

    /// Put a `$2007` write where it belongs (Mesen's `UpdateState` branch).
    fn land_ppudata_write(&mut self, memory: &mut PpuMemory<'_>) {
        let addr = self.ppudata_write_address;

        if addr >= 0x3F00 {
            // Palette RAM stores 6 bits. The four sprite backdrop entries
            // ($3F10/$3F14/$3F18/$3F1C) alias the background ones, so they
            // share storage.
            let palette_addr = Self::palette_index(addr);
            let value = self.ppudata_write_latch & 0x3F;
            self.palette[palette_addr] = value;
            if palette_addr % 4 == 0 {
                self.palette[palette_addr + 0x10] = value;
            }
        } else if memory.drawing_line() && self.is_rendering() {
            // "During rendering, the value written is ignored, and instead the
            // address' LSB is used (not confirmed, based on Visual NES)."
            let low = (addr & 0x00FF) as u8;
            self.write_vram(addr, low, memory);
            self.pending_bus_address = Some(addr);
        } else {
            let value = self.ppudata_write_latch;
            self.write_vram(addr, value, memory);
            self.pending_bus_address = Some(addr);
        }
    }

    /// Write to PPUDATA ($2007)
    fn write_ppudata(&mut self, value: u8) {
        // The byte is latched, not stored: the VRAM write happens five dots
        // later, and it is *that* write the cartridge's CHR sees — so a bank
        // switch in the gap changes where the byte lands. Both of the write's
        // oddities (the palette going to its six-bit storage, and the rendering
        // time replacement of the value with the address' low byte) belong to
        // the delayed write too, which is why they live in
        // `Ppu::land_ppudata_write` and read `memory` when they run.
        self.ppudata_write_address = self.v & 0x3FFF;
        self.ppudata_write_latch = value;
        self.ppudata_write_delay = Self::PPUDATA_DELAY_DOTS;
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

    /// The palette entry an address resolves to, mirrors included.
    ///
    /// Every address in $3F00-$3FFF names one: the storage is 32 bytes and the
    /// whole page repeats it eight times.
    pub fn palette_at(&self, addr: u16) -> u8 {
        self.palette[Self::palette_index(addr & 0x3FFF)]
    }

    /// Bits of the open bus the chip drove too long ago to still be there.
    const OPEN_BUS_DECAY_FRAMES: u64 = 3;

    /// Put `value` on the PPU's data bus and stamp the bits the chip is
    /// *driving*.
    ///
    /// The mask matters: a `$2002` read hands back the byte the status register
    /// makes up, whose low five bits are whatever the bus happened to hold
    /// rather than anything the chip drove. Those bits are not refreshed, so
    /// their decay keeps running — which is exactly what blargg's
    /// `ppu_open_bus` checks.
    fn drive_open_bus(&mut self, value: u8) {
        self.drive_open_bus_bits(value, 0xFF);
    }

    /// As [`Ppu::drive_open_bus`], for a byte only some of whose bits the chip
    /// drives.
    fn drive_open_bus_bits(&mut self, value: u8, driven: u8) {
        self.open_bus_latch = value;
        for bit in 0..8 {
            if driven & (1 << bit) != 0 && value & (1 << bit) != 0 {
                self.open_bus_decay_stamp[bit] = self.frame;
            }
        }
    }

    /// Open-bus value returned by write-only or unused register reads.
    ///
    /// It is the *PPU's* own latch, not the CPU-side data bus the bus carries
    /// around: the two chips have separate bus latches, and a read of a
    /// write-only PPU register returns what the PPU last had on its bus — the
    /// byte of its last register write, `$2007` access included — regardless of
    /// what the CPU touched in between. `PpuMemory::System`'s `open_bus` field
    /// is the CPU's, which the PPU never sees.
    ///
    /// The latch decays: a bit the chip has not driven for
    /// [`Ppu::OPEN_BUS_DECAY_FRAMES`] frames reads back as zero.
    fn open_bus(&self, _memory: &PpuMemory<'_>) -> u8 {
        let mut value = self.open_bus_latch;
        for bit in 0..8 {
            if value & (1 << bit) != 0
                && self.frame >= self.open_bus_decay_stamp[bit] + Self::OPEN_BUS_DECAY_FRAMES
            {
                value &= !(1 << bit);
            }
        }
        value
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
            if let PpuMemory::System {
                chr: Some(mapper), ..
            } = memory
            {
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
            if let PpuMemory::System {
                chr: Some(mapper), ..
            } = memory
            {
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
    /// Advance the VRAM address after a `$2007` access.
    ///
    /// Off a drawing line (or with rendering off) this is the documented
    /// increment, 1 or 32. On one it is the chip's odd behaviour, which Mesen
    /// spells out in `UpdateVideoRamAddr`: "it will update v in an odd way,
    /// triggering a coarse X increment and a Y increment simultaneously" —
    /// both halves of the scroll advance instead of the address.
    fn increment_vram_addr(&mut self, memory: &PpuMemory<'_>) {
        let drawing = matches!(
            memory,
            PpuMemory::System {
                drawing_line: true,
                ..
            }
        );
        let rendering = self
            .mask
            .intersects(PpuMask::SHOW_BG | PpuMask::SHOW_SPRITES);

        if drawing && rendering {
            crate::ppu::pipeline::increment_coarse_x(self);
            crate::ppu::pipeline::increment_y(self);
            return;
        }

        let increment = if self.ctrl.contains(PpuCtrl::INCREMENT) {
            32
        } else {
            1
        };
        self.v = self.v.wrapping_add(increment) & 0x7FFF;
        // The address the increment lands on is the one the cartridge sees
        // ("Trigger memory read when setting the vram address - needed by MMC3
        // IRQ counter: should be clocked when A12 changes to 1 via $2007
        // read/write"). While the picture is being drawn the fetches own the
        // bus, so nothing is shown.
        self.pending_bus_address = Some(self.v & 0x3FFF);
    }

    /// Check if NMI is pending
    pub fn nmi_pending(&self) -> bool {
        self.nmi_pending
    }

    /// Whether the VBlank flag is set.
    ///
    /// One half of the console's NMI line, which is high while this flag and
    /// [`PpuCtrl::NMI_ENABLE`] are both set.
    pub fn vblank_flag_set(&self) -> bool {
        self.status.contains(PpuStatus::VBLANK)
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

    /// A `$2002` read landing on the dot just before the one the VBlank flag is
    /// set on takes the frame's flag and NMI away, and the suppression is
    /// consumed exactly once.
    #[test]
    fn test_ppustatus_read_wins_the_vblank_race() {
        let mut ppu = Ppu::new();
        ppu.ctrl = PpuCtrl::NMI_ENABLE;
        ppu.scanline = 241;
        // The flag is set on dot 1, so the dot before it is dot 0 - where the
        // machine stands when a read lands two dots into its cycle.
        ppu.cycle = 0;

        let value = ppu.read_register(0x2002);
        assert_eq!(value & 0x80, 0, "the flag is not set yet");
        assert!(
            ppu.take_vblank_suppression(),
            "the frame's VBlank is suppressed"
        );
        assert!(
            !ppu.take_vblank_suppression(),
            "and only for that one frame"
        );

        // On the set dot itself the flag is already there, and the read takes
        // it away by clearing it rather than by suppressing it.
        let mut ppu = Ppu::new();
        ppu.ctrl = PpuCtrl::NMI_ENABLE;
        ppu.scanline = 241;
        ppu.cycle = 1;
        ppu.status.insert(PpuStatus::VBLANK);
        let value = ppu.read_register(0x2002);
        assert_eq!(value & 0x80, 0x80, "the flag is visible on its own dot");
        assert!(!ppu.take_vblank_suppression(), "nothing left to suppress");

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

        // A second read has to wait out the chip's six-dot window, or it is
        // ignored and hands back the open bus (Mesen's `_ignoreVramRead`).
        assert_eq!(
            ppu.read_register(0x2007),
            0,
            "an immediate second read is ignored"
        );
        for _ in 0..Ppu::IGNORE_VRAM_READ_DOTS {
            ppu.tick_vram_access_window();
        }
        let data2 = ppu.read_register(0x2007);
        assert_eq!(data2, 0x55);
    }

    /// On a line the chip is drawing, a `$2007` access moves the scroll
    /// instead of the address: "a coarse X increment and a Y increment
    /// simultaneously" (Mesen's `UpdateVideoRamAddr`). Off such a line — or
    /// with rendering off — the documented 1/32 increment applies.
    #[test]
    fn test_ppudata_advances_the_scroll_on_a_drawing_line() {
        // Rendering on, and the PPU standing on a visible line.
        let drawing = || PpuMemory::System {
            chr: None,
            mirroring: Mirroring::Horizontal,
            open_bus: 0,
            drawing_line: true,
        };
        let mut ppu = Ppu::new();
        ppu.mask = PpuMask::SHOW_BG;
        ppu.v = 0x0000;
        ppu.write_register_with(0x2007, 0x11, drawing());
        // The write lands five dots later and `v` moves a dot after *that*, so
        // the increment needs six dots' worth of ticks — and both decisions
        // read the line the PPU is standing on when they run, not the one it
        // was on when the CPU wrote.
        for _ in 0..6 {
            ppu.tick_delayed_state(&mut drawing());
        }
        // Coarse X went up by one and fine Y went up by one: neither the 1 nor
        // the 32 the control register would have picked.
        assert_eq!(ppu.v & 0x001F, 1, "coarse X advanced");
        assert_eq!((ppu.v >> 12) & 0x07, 1, "fine Y advanced");

        // The same access with rendering off is the ordinary increment.
        let mut ppu = Ppu::new();
        ppu.mask = PpuMask::empty();
        ppu.v = 0x0000;
        ppu.write_register_with(0x2007, 0x11, drawing());
        for _ in 0..6 {
            ppu.tick_delayed_state(&mut drawing());
        }
        assert_eq!(ppu.v, 1, "with rendering off, v just increments");
    }

    /// The grayscale bit clamps palette values on their way out of $2007 too,
    /// not only on their way to the screen.
    #[test]
    fn test_palette_reads_take_the_grayscale_mask() {
        let mut ppu = Ppu::new();
        ppu.palette[0] = 0x2A;

        let read = |ppu: &mut Ppu| {
            ppu.v = 0x3F00;
            ppu.ignore_vram_read = 0;
            ppu.read_register(0x2007)
        };

        assert_eq!(read(&mut ppu) & 0x3F, 0x2A, "colour as stored");
        ppu.mask = PpuMask::GRAYSCALE;
        assert_eq!(read(&mut ppu) & 0x3F, 0x20, "clamped to the grey column");
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

    /// A `$2007` read hands back the buffer at once, but the read that replaces
    /// the buffer — and the `v` increment behind it — belong to dots that have
    /// not happened yet. Five dots for the refill, one more for the address
    /// (Mesen's `_ppuMemoryDataReadStateMachine` and `_needVideoRamIncrement`).
    #[test]
    fn test_ppudata_read_refills_the_buffer_five_dots_later() {
        let mut ppu = Ppu::new();
        ppu.vram[0x2000] = 0x33;
        ppu.vram[0x2001] = 0x44;
        ppu.v = 0x2000;
        ppu.ignore_vram_read = 0;

        // The first read returns the empty buffer and starts the refill.
        assert_eq!(ppu.read_register_with(0x2007, PpuMemory::Internal), 0);

        // Nothing has landed yet, and `v` has not moved either.
        for dot in 1..=4 {
            ppu.tick_delayed_state(&mut PpuMemory::Internal);
            assert_eq!(ppu.read_buffer, 0, "buffer filled on dot {dot}");
            assert_eq!(ppu.v, 0x2000, "v moved on dot {dot}");
        }

        // Dot 5 is the refill; the address follows on dot 6, from the address
        // the *access* used, not from wherever `v` has got to.
        ppu.tick_delayed_state(&mut PpuMemory::Internal);
        assert_eq!(
            ppu.read_buffer, 0x33,
            "the refill reads the access' address"
        );
        assert_eq!(
            ppu.v, 0x2000,
            "the increment waits one dot behind the refill"
        );
        ppu.tick_delayed_state(&mut PpuMemory::Internal);
        assert_eq!(ppu.v, 0x2001, "the increment lands a dot after the refill");
        assert_eq!(ppu.read_buffer, 0x33, "and does not re-read memory");
    }

    /// A `$2007` write is latched for five dots before it lands, so a bank
    /// switch, a scroll change or an `$2006` write inside the gap changes where
    /// the byte goes.
    #[test]
    fn test_ppudata_write_lands_five_dots_later() {
        let mut ppu = Ppu::new();
        ppu.v = 0x2000;

        ppu.write_register_with(0x2007, 0x5A, PpuMemory::Internal);
        for dot in 1..=4 {
            ppu.tick_delayed_state(&mut PpuMemory::Internal);
            assert_eq!(ppu.vram[0x2000], 0, "the write landed on dot {dot}");
        }
        ppu.tick_delayed_state(&mut PpuMemory::Internal);
        assert_eq!(ppu.vram[0x2000], 0x5A, "the write lands on the fifth dot");
        assert_eq!(ppu.v, 0x2000, "the increment trails the write by a dot");
        ppu.tick_delayed_state(&mut PpuMemory::Internal);
        assert_eq!(ppu.v, 0x2001);
    }

    /// Only the *second* write to `$2006` moves `v`, and it does so three dots
    /// later (Mesen's `_updateVramAddrDelay`) — the gap is long enough for a
    /// `$2007` access in between to still see the old address.
    #[test]
    fn test_ppuaddr_second_write_moves_v_three_dots_later() {
        let mut ppu = Ppu::new();
        ppu.write_register(0x2006, 0x20);
        assert_eq!(ppu.v, 0, "the first write only touches t");

        ppu.write_register_with(0x2006, 0x00, PpuMemory::Internal);
        assert_eq!(ppu.t, 0x2000, "t is set at once");
        for dot in 1..=2 {
            ppu.tick_delayed_state(&mut PpuMemory::Internal);
            assert_eq!(ppu.v, 0, "v moved on dot {dot}");
        }
        ppu.tick_delayed_state(&mut PpuMemory::Internal);
        assert_eq!(ppu.v, 0x2000, "v follows t on the third dot");
        assert_eq!(ppu.t, 0x2000, "and t follows it back");
    }

    /// During rendering the written value is thrown away and the address' low
    /// byte is stored in its place (Mesen: "the value written is ignored, and
    /// instead the address' LSB is used").
    #[test]
    fn test_ppudata_write_during_rendering_stores_the_address_low_byte() {
        let mut ppu = Ppu::new();
        ppu.mask = PpuMask::SHOW_BG;
        ppu.v = 0x2345;
        let drawing = || PpuMemory::System {
            chr: None,
            mirroring: Mirroring::Horizontal,
            open_bus: 0,
            drawing_line: true,
        };

        ppu.write_register_with(0x2007, 0x5A, drawing());
        for _ in 0..5 {
            ppu.tick_delayed_state(&mut drawing());
        }
        assert_eq!(
            ppu.vram[0x2345], 0x45,
            "the low byte of the address was stored, not the value"
        );
    }

    /// With rendering off, an address in $3F00-$3FFF puts *that* palette entry
    /// on the wire instead of the backdrop — the rule `full_palette` draws its
    /// grid with.
    #[test]
    fn test_palette_at_resolves_the_mirrors() {
        let mut ppu = Ppu::new();
        ppu.palette[0x00] = 0x0F;
        ppu.palette[0x11] = 0x21;

        assert_eq!(ppu.palette_at(0x3F00), 0x0F);
        assert_eq!(ppu.palette_at(0x3F20), 0x0F, "the page repeats");
        assert_eq!(ppu.palette_at(0x3F11), 0x21);
        assert_eq!(ppu.palette_at(0x3F10), 0x0F, "$3F10 mirrors $3F00");
    }
}
