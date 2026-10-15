//! The PPU's per-dot pixel pipeline.
//!
//! Mesen's shape, and the shape the hardware has: the chip fetches a tile
//! every eight dots, shifts it out one pixel per dot, and draws the pixel on
//! the dot it belongs to. The chip's *own* dot counter drives all of it, which
//! is why a mid-line `$2001`/`$2005`/`$2006` write takes effect from the dot it
//! lands on instead of for a whole line or not at all.
//!
//! Two things follow from fetching at the dot rather than reconstructing the
//! fetches later:
//!
//! * the address the cartridge sees is the address of a real access, so an
//!   A12 filter (MMC3 and its clones) clocks on real fetches;
//! * the scroll registers are the ones the CPU writes — there is no separate
//!   copy for rendering, exactly as Mesen uses its `_videoRamAddr`.
//!
//! Timing, one fetch group of eight dots (`(cycle - 1) % 8`):
//!
//! | dot | background |
//! | --- | --- |
//! | 0 | nametable byte fetch |
//! | 2 | attribute byte fetch |
//! | 4 | pattern low byte fetch |
//! | 6 | pattern high byte fetch |
//! | 7 | shift registers reload from the latches, coarse X increment |
//!
//! Coarse X moves on at the group's *last* dot, not right after the nametable
//! read: the attribute fetch two dots later asks `v` which of the four palette
//! pairs this tile uses, and which attribute byte covers it. Mesen steps it in
//! the same place (`IncHorizontalScrolling` on `(_cycle & 0x07) == 0`, which is
//! the reload dot here) — stepping it earlier hands every tile its *right-hand
//! neighbour's* palette.
//!
//! The shift registers shift left on every dot of the line, and a pixel is
//! drawn on dots 1-256: the register's bit `15 - fine_x` is the current pixel,
//! which makes the tile fetched during one group appear in the next one. Groups
//! 321-336 refetch the first two tiles of the *following* line, which is where
//! a scanline's first tile comes from.

use crate::ppu::rendering::{nes_color, ChrSource, FrameBuffer, Mirroring};
use crate::ppu::{Ppu, PpuMask, PpuStatus};
use crate::prelude::*;
use crate::Region;

/// Width of the picture in pixels, and of one line of the frame buffer.
const WIDTH: usize = crate::ppu::rendering::WIDTH;

/// Background shift registers and latches.
///
/// The shifters are 16 bits wide with the tile being drawn in the high byte:
/// the low byte is the tail end of the tile before it, which is what the
/// hardware has and what makes the reload a plain `|=` into the high byte.
/// Attribute bits are stored as whole bytes (`0x00`/`0xFF`) so the same shift
/// and mask can pick the palette out.
#[derive(Debug, Default, Clone)]
struct Shifters {
    low: u16,
    high: u16,
    attr_low: u16,
    attr_high: u16,
    attr_latch_low: u16,
    attr_latch_high: u16,
}

crate::state::state_fields!(Shifters {
    low,
    high,
    attr_low,
    attr_high,
    attr_latch_low,
    attr_latch_high,
});

impl Shifters {
    /// One dot's shift, left the way the hardware shifts.
    fn shift(&mut self) {
        self.low <<= 1;
        self.high <<= 1;
        self.attr_low <<= 1;
        self.attr_high <<= 1;
    }

    /// Move the latches into the low bytes of the shifters.
    ///
    /// The tile fetched during this group of eight dots starts here and walks
    /// up into the high byte over the next eight dots, which is what puts it on
    /// screen two groups after its fetch — eight dots after the reload, because
    /// the reload itself is not shifted.
    fn reload(&mut self, tile_low: u8, tile_high: u8) {
        self.low = (self.low & 0xFF00) | tile_low as u16;
        self.high = (self.high & 0xFF00) | tile_high as u16;
        self.attr_low = (self.attr_low & 0xFF00) | self.attr_latch_low;
        self.attr_high = (self.attr_high & 0xFF00) | self.attr_latch_high;
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The per-dot renderer: fetches, shift registers, sprites and the picture.
pub struct PixelPipeline {
    shifters: Shifters,
    /// Pattern bytes of the tile being fetched, before they are reloaded.
    tile_low: u8,
    tile_high: u8,
    /// Nametable and attribute bytes of the tile being fetched.
    nametable_byte: u8,
    attribute_byte: u8,
    /// The finished frame, swapped with the host's copy at each frame boundary.
    frame: FrameBuffer,
    /// Whether the background drew an opaque pixel at each of the 256x240
    /// positions, which is what sprite priority and the sprite-0 hit test on.
    bg_opaque: Vec<u8>,
    /// Nametable mirroring, following the cartridge.
    mirroring: Mirroring,
    /// TV system, which decides what the emphasis bits mean.
    region: Region,
    /// Address of the last access the pipeline made, for the cartridge's A12
    /// filter. `None` on dots that made no access.
    bus_access: Option<u16>,
    /// The secondary-OAM slots the sprite phase fetches for, filled by the
    /// line's evaluation: one entry per slot.
    ///
    /// The *evaluation* that fills these is still done a line at a time; the
    /// fetch, the drawing and the sprite-0 hit all run at the dot.
    sprite_slots: [SpriteSlot; 8],
    /// The pattern bytes fetched for those slots, which is what the drawing
    /// phase reads.
    sprite_tiles: [SpriteTile; 8],
}

/// One of the line's eight secondary-OAM slots, as the evaluation found it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpriteSlot {
    /// OAM byte 0: the sprite's Y position, one less than its top line.
    pub y: u8,
    /// OAM byte 1: tile index.
    pub tile: u8,
    /// OAM byte 2: attributes (flips, priority, palette).
    pub attributes: u8,
    /// OAM byte 3: X position.
    pub x: u8,
    /// Whether this slot holds OAM sprite 0, which is the only one that can
    /// raise the sprite-0 hit.
    pub is_sprite_zero: bool,
    /// Whether the evaluation put a sprite here at all.
    ///
    /// The chip reports an empty slot by fetching tile $FF; this is what keeps
    /// that fetch from ever being drawn.
    pub occupied: bool,
}

crate::state::state_fields!(SpriteSlot {
    y,
    tile,
    attributes,
    x,
    is_sprite_zero,
    occupied,
});

impl SpriteSlot {
    fn vertical_flip(&self) -> bool {
        self.attributes & 0x80 != 0
    }
    fn horizontal_flip(&self) -> bool {
        self.attributes & 0x40 != 0
    }
    fn behind_background(&self) -> bool {
        self.attributes & 0x20 != 0
    }
    fn palette(&self) -> u8 {
        (self.attributes >> 2) & 0x03
    }
}

/// A secondary-OAM slot's pattern bytes for the line being drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SpriteTile {
    low: u8,
    high: u8,
    /// Whether the slot was occupied, i.e. whether the fetched bytes mean
    /// anything. The chip fetches tile $FF for an empty slot; this is what
    /// keeps that fetch from being drawn.
    occupied: bool,
}

crate::state::state_fields!(SpriteTile {
    low,
    high,
    occupied
});

crate::state::state_fields!(PixelPipeline {
    shifters,
    tile_low,
    tile_high,
    nametable_byte,
    attribute_byte,
    frame,
    bg_opaque,
    mirroring,
    region,
    sprite_slots,
    sprite_tiles,
});

impl Default for PixelPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl PixelPipeline {
    /// A pipeline with an empty picture and empty shift registers.
    pub fn new() -> Self {
        Self {
            shifters: Shifters::default(),
            tile_low: 0,
            tile_high: 0,
            nametable_byte: 0,
            attribute_byte: 0,
            frame: FrameBuffer::new(),
            bg_opaque: vec![0; WIDTH * 240],
            mirroring: Mirroring::Horizontal,
            region: Region::Ntsc,
            bus_access: None,
            sprite_slots: [SpriteSlot::default(); 8],
            sprite_tiles: [SpriteTile::default(); 8],
        }
    }

    /// Hand the sprite phase the line's secondary-OAM slots.
    pub fn set_sprite_slots(&mut self, slots: [SpriteSlot; 8]) {
        self.sprite_slots = slots;
        for (tile, slot) in self.sprite_tiles.iter_mut().zip(slots.iter()) {
            tile.low = 0;
            tile.high = 0;
            tile.occupied = slot.occupied;
        }
    }

    /// Forget the shift registers, as the chip does at the pre-render line.
    pub fn reset_shifters(&mut self) {
        self.shifters.reset();
    }

    /// Follow the cartridge's mirroring, for the nametable fetches.
    pub fn set_mirroring(&mut self, mirroring: Mirroring) {
        self.mirroring = mirroring;
    }

    /// Follow the console's TV system, which swaps what the `$2001` emphasis
    /// bits mean on PAL and Dendy (Mesen's `SetMaskRegister`).
    pub fn set_region(&mut self, region: Region) {
        self.region = region;
    }

    /// Nametable mirroring in use.
    pub fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    /// The address of the last access the pipeline made, taking it away.
    ///
    /// The cartridge is handed exactly the addresses the chip read, on the dot
    /// it read them, which is what an A12 filter is built on.
    pub fn take_bus_access(&mut self) -> Option<u16> {
        self.bus_access.take()
    }

    /// The picture being drawn.
    pub fn frame_buffer(&self) -> &FrameBuffer {
        &self.frame
    }

    /// The picture being drawn, for the frame swap.
    pub fn frame_buffer_mut(&mut self) -> &mut FrameBuffer {
        &mut self.frame
    }

    /// Whether the background produced an opaque pixel at `(x, y)`.
    pub fn bg_opaque(&self, x: usize, y: usize) -> bool {
        self.bg_opaque[y * WIDTH + x] != 0
    }

    /// The whole background-opacity plane, which the sprite renderer tests
    /// against for priority and the sprite-0 hit.
    pub fn bg_opaque_plane(&self) -> &[u8] {
        &self.bg_opaque
    }

    /// Run the pipeline for the dot the PPU has just advanced to.
    ///
    /// The PPU's own `scanline`/`cycle` are the input: the machine counts the
    /// dots, and this reads the position it is standing on. `pre_render` is the
    /// region's last scanline, which the pipeline treats as part of the visible
    /// range (Mesen's `_scanline == -1`).
    #[inline]
    pub fn step(&mut self, ppu: &mut Ppu, chr: &mut dyn ChrSource, pre_render: u16) {
        self.bus_access = None;
        let cycle = ppu.cycle;
        let scanline = ppu.scanline;

        let visible = scanline < 240 || scanline == pre_render;
        if !visible {
            return;
        }

        let mask = ppu.mask;
        let show_bg = mask.contains(PpuMask::SHOW_BG);
        let show_sprites = mask.contains(PpuMask::SHOW_SPRITES);
        let rendering = show_bg || show_sprites;
        let in_fetch_range = (1..=256).contains(&cycle) || (321..=336).contains(&cycle);

        // Dots 1-256 are the visible pixels; 321-336 refetch the first two
        // tiles of the next line, and the reload at 336 is what leaves them in
        // the registers for it.
        let fetch_phase = cycle.wrapping_sub(1) % 8;
        if rendering && show_bg && in_fetch_range {
            match fetch_phase {
                0 => self.fetch_nametable(ppu, chr),
                2 => self.fetch_attribute(ppu, chr),
                4 => self.fetch_pattern_low(ppu, chr),
                6 => self.fetch_pattern_high(ppu, chr),
                _ => {}
            }
        }

        // Dots 257-320 fetch the sprites: two nametable reads the chip throws
        // away, then the slot's pattern bytes. The thrown-away reads matter to
        // the cartridge even though nothing uses them — they are what takes A12
        // low between two sprites' pattern fetches, which is what an MC-ACC
        // clone's IRQ is built on.
        if rendering && show_sprites && (257..=320).contains(&cycle) {
            let slot = ((cycle - 257) / 8) as usize;
            match (cycle - 257) % 8 {
                0 | 2 => {
                    let addr = 0x2000 | (ppu.v & 0x0FFF);
                    let _ = self.read_nametable(ppu, chr, addr);
                    self.bus_access = Some(addr);
                }
                4 | 6 => {
                    let (addr, row) = self.sprite_pattern_addr(ppu, slot);
                    // The tile's eight low-plane bytes come first and the high
                    // plane's eight follow them: phase 4 reads `row`, phase 6
                    // reads `row + 8`. Reading the same address twice hands the
                    // low plane in as the high one, which halves the sprite's
                    // colours — every pixel whose planes are (0, 1), colour 2,
                    // reads as (0, 0) and is dropped as transparent.
                    let high_plane = if (cycle - 257) % 8 == 4 { 0 } else { 8 };
                    let pattern_addr = addr + row + high_plane;
                    let byte = chr.read_pattern(ppu, pattern_addr);
                    if high_plane == 0 {
                        self.sprite_tiles[slot].low = byte;
                    } else {
                        self.sprite_tiles[slot].high = byte;
                    }
                    self.bus_access = Some(pattern_addr);
                }
                _ => {}
            }
        }

        // The vertical part of the scroll lives in `v`: the coarse Y and fine Y
        // increment at dot 256, and the horizontal half is reloaded from `t` at
        // 257 for the next line. Neither happens with rendering off: `v` is the
        // CPU's own VRAM address then, and the chip must leave it alone.
        if rendering {
            if in_fetch_range && cycle == 256 {
                increment_y(ppu);
            }
            if cycle == 257 {
                copy_horizontal(ppu);
            }
            if scanline == pre_render && (280..=304).contains(&cycle) {
                copy_vertical(ppu);
            }
        }

        if scanline < 240 && (1..=256).contains(&cycle) {
            let x = (cycle - 1) as usize;
            let row = scanline as usize * WIDTH;
            if x == 0 {
                self.bg_opaque[row..row + WIDTH].fill(0);
            }
            if show_bg {
                self.draw_background_pixel(ppu, x);
            } else {
                // With the background off the chip puts the backdrop on the
                // wire, and sprites draw over it.
                let backdrop = nes_color(ppu.palette[0], ppu.mask, self.region);
                self.frame.set_pixel(x, scanline as usize, backdrop);
            }
            if show_sprites {
                self.draw_sprite_pixel(ppu, x);
            }
        }

        // Shift every dot of the line up to 336: the sprite phase shifts the
        // background registers too, and the dots after 336 must not, or the
        // prefetched tiles would not survive into the next line. The shift
        // comes after the draw, so the pixel just drawn is the one the register
        // was holding at the start of the dot.
        if rendering && (1..=336).contains(&cycle) {
            self.shifters.shift();
        }

        // The reload is last, and only every eighth dot: the tile fetched
        // during this group starts in the low byte and has the whole of the
        // next group to walk up into the high byte.
        if rendering && show_bg && in_fetch_range && fetch_phase == 7 {
            self.shifters.reload(self.tile_low, self.tile_high);
            // Coarse X steps on with it, once the group's own nametable,
            // attribute and pattern fetches are done: the increment is what
            // takes `v` to the tile the *next* group fetches, and it is the
            // `v` a mid-line `$2005`/`$2006` write from the CPU lands on.
            increment_coarse_x(ppu);
        }
    }

    /// Fetch the nametable byte for the tile being fetched.
    ///
    /// Coarse X is *not* stepped here: `v` has to keep pointing at this tile
    /// until the group's last dot, or the attribute fetch would read the
    /// neighbour's palette (see the module docs).
    fn fetch_nametable(&mut self, ppu: &mut Ppu, chr: &mut dyn ChrSource) {
        let addr = 0x2000 | (ppu.v & 0x0FFF);
        self.nametable_byte = self.read_nametable(ppu, chr, addr);
        self.bus_access = Some(addr);
    }

    /// Fetch the attribute byte and pick the two palette bits out of it.
    fn fetch_attribute(&mut self, ppu: &mut Ppu, chr: &mut dyn ChrSource) {
        let addr = 0x23C0 | (ppu.v & 0x0C00) | ((ppu.v >> 4) & 0x38) | ((ppu.v >> 2) & 0x07);
        self.attribute_byte = self.read_nametable(ppu, chr, addr);
        self.bus_access = Some(addr);

        // The attribute byte covers a 4x4 tile block, two bits per 2x2 group;
        // which pair belongs to this tile is decided by coarse X bit 1 and
        // coarse Y bit 1 of `v`.
        let shift = ((ppu.v >> 4) & 4) | (ppu.v & 2);
        let palette = (self.attribute_byte >> shift) & 0x03;
        self.shifters.attr_latch_low = if palette & 0x01 != 0 { 0xFF } else { 0 };
        self.shifters.attr_latch_high = if palette & 0x02 != 0 { 0xFF } else { 0 };
    }

    /// Fetch the low pattern byte of the tile being fetched.
    fn fetch_pattern_low(&mut self, ppu: &mut Ppu, chr: &mut dyn ChrSource) {
        let addr = background_pattern_addr(ppu, self.nametable_byte);
        self.tile_low = chr.read_pattern(ppu, addr);
        self.bus_access = Some(addr);
    }

    /// Fetch the high pattern byte of the tile being fetched.
    fn fetch_pattern_high(&mut self, ppu: &mut Ppu, chr: &mut dyn ChrSource) {
        let addr = background_pattern_addr(ppu, self.nametable_byte) + 8;
        self.tile_high = chr.read_pattern(ppu, addr);
        self.bus_access = Some(addr);
    }

    /// Read a nametable byte through the cartridge where it has an opinion.
    fn read_nametable(&self, ppu: &Ppu, chr: &mut dyn ChrSource, addr: u16) -> u8 {
        if let Some(byte) = chr.read_nametable(ppu, addr) {
            return byte;
        }
        ppu.vram[self.mirroring.mirror_vram_addr(addr) as usize]
    }

    /// Pattern address of a slot's sprite row, and the row within the tile.
    ///
    /// The sprites the evaluation found during the line being drawn are for the
    /// *next* line, so the row comes from that one: how far the sprite's own
    /// top is above it, flipped if the attributes say so. 8x16 sprites pick
    /// their pattern table from bit 0 of the tile and their second tile from
    /// bit 0 of the row.
    fn sprite_pattern_addr(&self, ppu: &Ppu, slot: usize) -> (u16, u16) {
        let sprite = self.sprite_slots[slot];
        let height = if ppu.ctrl.contains(crate::ppu::PpuCtrl::SPRITE_SIZE) {
            16u16
        } else {
            8u16
        };
        // The evaluation only keeps sprites that cover that line, so the
        // subtraction stays in range; saturate anyway so a slot the evaluation
        // did not fill cannot wrap.
        let mut row = ppu
            .scanline
            .saturating_add(1)
            .saturating_sub(sprite.y as u16 + 1);
        if sprite.vertical_flip() {
            row = height - 1 - row;
        }

        let (table, tile) = if height == 16 {
            let table = if sprite.tile & 1 != 0 { 0x1000 } else { 0x0000 };
            (table, (sprite.tile & 0xFE) as u16 + (row / 8))
        } else {
            let table = if ppu.ctrl.contains(crate::ppu::PpuCtrl::SPRITE_PATTERN) {
                0x1000
            } else {
                0x0000
            };
            (table, sprite.tile as u16)
        };
        (table + tile * 16, row & 7)
    }

    /// Draw the sprite pixel for this dot, if any sprite covers it.
    ///
    /// Secondary OAM is in priority order, so the first slot with an opaque
    /// pixel wins. The sprite-0 hit is raised *here*, on the dot the pixel is
    /// drawn on, which is what makes it usable as a mid-line timing reference.
    fn draw_sprite_pixel(&mut self, ppu: &mut Ppu, x: usize) {
        let y = ppu.scanline as usize;
        for slot in 0..8 {
            let sprite = self.sprite_slots[slot];
            let tile = self.sprite_tiles[slot];
            if !tile.occupied {
                continue;
            }
            if x < sprite.x as usize || x >= sprite.x as usize + 8 {
                continue;
            }
            // Bit 7 of the pattern byte is the sprite's leftmost pixel, so the
            // pixel for this column is counted from the other end — unless the
            // sprite is flipped, which is exactly the other way round.
            let column = x - sprite.x as usize;
            let bit = if sprite.horizontal_flip() {
                column
            } else {
                7 - column
            };
            let low = (tile.low >> bit) & 0x01;
            let high = (tile.high >> bit) & 0x01;
            let color_index = (high << 1) | low;
            if color_index == 0 {
                // A transparent pixel of a higher-priority sprite does not hide
                // the ones behind it.
                continue;
            }

            let clipped = x < 8 && !ppu.mask.contains(PpuMask::SHOW_SPRITES_LEFT);
            let bg_opaque = self.bg_opaque[y * WIDTH + x] != 0;

            // Sprite 0's hit: both planes have an opaque pixel here, the pixel
            // is not the rightmost column (where the chip never reports it),
            // and neither plane is clipped at the left edge.
            if sprite.is_sprite_zero
                && !clipped
                && bg_opaque
                && x != 255
                && !ppu.status.contains(PpuStatus::SPRITE_0_HIT)
                && ppu.mask.contains(PpuMask::SHOW_BG)
                && !(x < 8 && !ppu.mask.contains(PpuMask::SHOW_BG_LEFT))
            {
                ppu.status.insert(PpuStatus::SPRITE_0_HIT);
            }

            if !clipped && !(sprite.behind_background() && bg_opaque) {
                let palette_addr = 0x10 + (sprite.palette() as usize) * 4 + color_index as usize;
                let color = nes_color(ppu.palette[palette_addr], ppu.mask, self.region);
                self.frame.set_pixel(x, y, color);
            }
            return;
        }
    }

    /// Draw the pixel the background shift registers hold for this dot.
    ///
    /// Every pixel is written: a transparent bit (or one the left-edge clip
    /// covers) shows the backdrop, exactly as the chip drives it.
    #[inline]
    fn draw_background_pixel(&mut self, ppu: &Ppu, x: usize) {
        let y = ppu.scanline as usize;
        let bitmap = self.shifters.low | self.shifters.high;
        let bit = 15 - ppu.x as u16;
        // A zero in the pattern data means "nothing here": the palette's
        // backdrop shows through, and sprites draw over it.
        let clipped = x < 8 && !ppu.mask.contains(PpuMask::SHOW_BG_LEFT);
        if (bitmap >> bit) & 1 != 0 && !clipped {
            let low = ((self.shifters.low << ppu.x) >> 15) & 0x01;
            let high = ((self.shifters.high << ppu.x) >> 15) & 0x01;
            let attr_low = ((self.shifters.attr_low << ppu.x) >> 15) & 0x01;
            let attr_high = ((self.shifters.attr_high << ppu.x) >> 15) & 0x01;
            let palette = (attr_high << 1) | attr_low;
            let color_index = (high << 1) | low;

            let palette_addr = (palette as usize) * 4 + color_index as usize;
            let color = nes_color(ppu.palette[palette_addr], ppu.mask, self.region);
            self.frame.set_pixel(x, y, color);
            self.bg_opaque[y * WIDTH + x] = 1;
        } else {
            // Lazy, because most calls land here and the palette lookup is not
            // free: this is the hottest line in the emulator.
            let backdrop = nes_color(ppu.palette[0], ppu.mask, self.region);
            self.frame.set_pixel(x, y, backdrop);
        }
    }

    /// Clear the per-pixel background plane, as the chip does each frame.
    pub fn clear_bg_plane(&mut self) {
        self.bg_opaque.fill(0);
    }
}

/// Address of the background pattern byte for `tile`, on the row `v` points at.
///
/// The pattern table is the one PPUCTRL selects, the row is `v`'s fine Y, and
/// the low byte of the pair is returned — the high byte is next to it.
fn background_pattern_addr(ppu: &Ppu, tile: u8) -> u16 {
    let base = if ppu.ctrl.contains(crate::ppu::PpuCtrl::BACKGROUND_PATTERN) {
        0x1000
    } else {
        0x0000
    };
    let fine_y = (ppu.v >> 12) & 0x07;
    base + (tile as u16) * 16 + fine_y
}

/// `v`'s coarse X increments after every nametable fetch, wrapping into the
/// nametable select bit at the end of a row.
pub(crate) fn increment_coarse_x(ppu: &mut Ppu) {
    if ppu.v & 0x001F == 0x001F {
        ppu.v &= !0x001F;
        ppu.v ^= 0x0400;
    } else {
        ppu.v += 1;
    }
}

/// `v`'s coarse Y and fine Y increment at dot 256, wrapping at the bottom of a
/// nametable.
pub(crate) fn increment_y(ppu: &mut Ppu) {
    if ppu.v & 0x7000 != 0x7000 {
        ppu.v += 0x1000;
    } else {
        ppu.v &= !0x7000;
        let mut y = (ppu.v & 0x03E0) >> 5;
        if y == 29 {
            y = 0;
            ppu.v ^= 0x0800;
        } else if y == 31 {
            y = 0;
        } else {
            y += 1;
        }
        ppu.v = (ppu.v & !0x03E0) | (y << 5);
    }
}

/// Copy the horizontal half of `t` into `v`, at dot 257 of each line.
fn copy_horizontal(ppu: &mut Ppu) {
    ppu.v = (ppu.v & !0x041F) | (ppu.t & 0x041F);
}

/// Copy the vertical half of `t` into `v`, during pre-render dots 280-304.
fn copy_vertical(ppu: &mut Ppu) {
    ppu.v = (ppu.v & !0x7BE0) | (ppu.t & 0x7BE0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ppu::rendering::VramChr;

    /// A PPU whose first nametable holds `tiles` in row 0, with each tile's
    /// pattern table row 0 low plane set from `patterns`.
    fn ppu_with(tiles: &[(usize, u8)], patterns: &[(u8, u8)]) -> Ppu {
        let mut ppu = Ppu::new();
        for &(x, tile) in tiles {
            ppu.vram[0x2000 + x] = tile;
        }
        for &(tile, low) in patterns {
            // Pattern table 0: tile * 16 bytes, row 0 is the first byte.
            ppu.vram[(tile as usize) * 16] = low;
        }
        ppu.palette[0] = 0x0F;
        ppu.palette[1] = 0x30;
        ppu
    }

    /// Give every row of a tile in pattern table 0 the same low-plane byte, so
    /// a test does not have to know which fine Y a line lands on.
    fn fill_tile_rows(ppu: &mut Ppu, tile: u8, low: u8) {
        for row in 0..8 {
            ppu.vram[(tile as usize) * 16 + row] = low;
        }
    }

    /// Run one whole scanline's dots (0-340) through the pipeline.
    fn run_line(pipeline: &mut PixelPipeline, ppu: &mut Ppu, scanline: u16) {
        ppu.scanline = scanline;
        for dot in 0..=340u16 {
            ppu.cycle = dot;
            let mut chr = VramChr;
            pipeline.step(ppu, &mut chr, 261);
        }
    }

    #[test]
    fn test_first_tile_lands_at_the_left_edge() {
        // Tile 1 at coarse X = 0, and tile 1's row 0 lights only pixel 0.
        let mut ppu = ppu_with(&[(0, 1)], &[(1, 0x80)]);
        ppu.mask = PpuMask::SHOW_BG | PpuMask::SHOW_BG_LEFT;
        ppu.v = 0x0000;
        ppu.t = 0x0000;
        ppu.x = 0;
        ppu.palette[1] = 0x30;

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261); // pre-render prefetch
        run_line(&mut pipeline, &mut ppu, 0);

        let drawn: Vec<usize> = (0..16)
            .filter(|&x| {
                pipeline.frame_buffer().get_pixel(x, 0)
                    == nes_color(0x30, ppu.mask, pipeline.region)
            })
            .collect();
        assert_eq!(drawn, vec![0], "only pixel 0 of the tile is set");
    }

    /// The emphasis bits reach the frame buffer as Mesen's nine-bit palette
    /// resolves them, not through the renderer's own arithmetic.
    #[test]
    fn test_emphasis_reaches_the_frame_buffer_as_mesens_palette_resolves_it() {
        let mut ppu = ppu_with(&[(0, 1)], &[(1, 0x80)]);
        ppu.mask = PpuMask::SHOW_BG | PpuMask::SHOW_BG_LEFT | PpuMask::EMPHASIZE_RED;
        ppu.v = 0x0000;
        ppu.t = 0x0000;
        ppu.x = 0;
        ppu.palette[1] = 0x21;

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        run_line(&mut pipeline, &mut ppu, 0);

        assert_eq!(
            pipeline.frame_buffer().get_pixel(0, 0),
            0xFF64_93D6,
            "red emphasis damps green and blue to 84% of the palette entry"
        );
    }

    #[test]
    fn test_scroll_penalty() {
        // The same tile, but fine X = 1: pixel 0 of the tile lands at screen
        // x = -1, so the screen shows the rest of the tile from x = 0.
        let mut ppu = ppu_with(&[(0, 1)], &[(1, 0b0100_0000)]);
        ppu.mask = PpuMask::SHOW_BG | PpuMask::SHOW_BG_LEFT;
        ppu.v = 0x0000;
        ppu.t = 0x0000;
        ppu.x = 1;

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        run_line(&mut pipeline, &mut ppu, 0);

        let drawn: Vec<usize> = (0..16)
            .filter(|&x| {
                pipeline.frame_buffer().get_pixel(x, 0)
                    == nes_color(0x30, ppu.mask, pipeline.region)
            })
            .collect();
        assert_eq!(drawn, vec![0]);
    }

    /// `v`'s coarse X steps on at the last dot of a fetch group, not with the
    /// nametable read: the attribute byte two dots later, and the row the
    /// pattern fetches read, both belong to the tile the nametable byte named.
    #[test]
    fn test_coarse_x_steps_on_at_the_end_of_the_fetch_group() {
        let mut ppu = ppu_with(&[(0, 1), (1, 2)], &[(1, 0x00), (2, 0x00)]);
        ppu.mask = PpuMask::SHOW_BG | PpuMask::SHOW_BG_LEFT;
        ppu.v = 0x0000;
        ppu.t = 0x0000;

        let mut pipeline = PixelPipeline::new();
        let mut chr = VramChr;
        for dot in 1..=8u16 {
            ppu.cycle = dot;
            pipeline.step(&mut ppu, &mut chr, 261);
            assert_eq!(
                ppu.v & 0x001F,
                if dot == 8 { 1 } else { 0 },
                "coarse X after dot {dot}"
            );
        }
    }

    /// A tile reads the pair of palette bits its *own* coarse X names. Both
    /// tiles of a pair share them, so the boundary sits between coarse X 1 and
    /// 2 — six dots after the nametable read that started the group.
    #[test]
    fn test_a_tile_takes_its_own_attribute_quadrant() {
        let mut ppu = ppu_with(&[(1, 1)], &[(1, 0x80)]);
        // Left pair of the attribute byte's top half: palette 0; right pair:
        // palette 2.
        ppu.vram[0x23C0] = 0b0000_1000;
        ppu.mask = PpuMask::SHOW_BG | PpuMask::SHOW_BG_LEFT;
        ppu.v = 0x0000;
        ppu.t = 0x0000;
        ppu.x = 0;
        ppu.palette[1] = 0x30;
        ppu.palette[9] = 0x16;

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        run_line(&mut pipeline, &mut ppu, 0);

        assert_eq!(
            pipeline.frame_buffer().get_pixel(8, 0),
            nes_color(0x30, ppu.mask, pipeline.region),
            "coarse X 1 is left of the pair boundary, so it keeps palette 0"
        );
    }

    /// The attribute *address* is the group's own tile too: the last tile of a
    /// four-tile block reads that block's byte, not the next one's.
    #[test]
    fn test_the_attribute_address_is_the_tiles_own_block() {
        let mut ppu = ppu_with(&[(3, 1)], &[(1, 0x80)]);
        ppu.vram[0x23C0] = 0b0000_1100;
        ppu.vram[0x23C1] = 0b0000_0000;
        ppu.mask = PpuMask::SHOW_BG | PpuMask::SHOW_BG_LEFT;
        ppu.v = 0x0000;
        ppu.t = 0x0000;
        ppu.x = 0;
        ppu.palette[1] = 0x30;
        ppu.palette[13] = 0x27;

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        run_line(&mut pipeline, &mut ppu, 0);

        assert_eq!(
            pipeline.frame_buffer().get_pixel(24, 0),
            nes_color(0x27, ppu.mask, pipeline.region),
            "coarse X 3 takes the pair the $23C0 byte gives it"
        );
    }

    /// The sprite-0 hit arrives on the dot the overlapping pixel is drawn on,
    /// which is what makes it usable as a timing reference mid-line.
    #[test]
    fn test_sprite_zero_hit_lands_on_its_pixel() {
        for sprite_x in [0u8, 10] {
            // The background is opaque on the whole row — tile 1 in every
            // column of the nametable, and all of its pixels lit — so wherever
            // the sprite lands there is something to hit.
            let mut ppu = ppu_with(&[(0, 1), (1, 1)], &[(1, 0xFF)]);
            fill_tile_rows(&mut ppu, 1, 0xFF);
            // Both planes are on, and the left edge is not clipped.
            ppu.mask = PpuMask::SHOW_BG
                | PpuMask::SHOW_BG_LEFT
                | PpuMask::SHOW_SPRITES
                | PpuMask::SHOW_SPRITES_LEFT;
            ppu.v = 0x0000;
            ppu.t = 0x0000;
            ppu.x = 0;

            let mut pipeline = PixelPipeline::new();
            run_line(&mut pipeline, &mut ppu, 261);

            // Sprite 0 sits on line 1 (an OAM Y of 0 means the sprite's top row
            // is line 1), and its tile lights pixel 0.
            let mut slots = [SpriteSlot::default(); 8];
            slots[0] = SpriteSlot {
                y: 0,
                tile: 2,
                attributes: 0,
                x: sprite_x,
                is_sprite_zero: true,
                occupied: true,
            };
            ppu.vram[2 * 16] = 0x80;
            pipeline.set_sprite_slots(slots);

            // Line 0's sprite phase fetches the pattern bytes for line 1.
            run_line(&mut pipeline, &mut ppu, 0);
            assert!(
                !ppu.status.contains(PpuStatus::SPRITE_0_HIT),
                "the hit belongs to line 1, not to the line the fetch ran on"
            );

            ppu.scanline = 1;
            let mut hit_dot = None;
            for dot in 0..=340u16 {
                ppu.cycle = dot;
                let mut chr = VramChr;
                pipeline.step(&mut ppu, &mut chr, 261);
                if hit_dot.is_none() && ppu.status.contains(PpuStatus::SPRITE_0_HIT) {
                    hit_dot = Some(dot);
                }
            }

            // The sprite covers screen x = sprite_x .. sprite_x + 7, and the
            // background is opaque on pixel 0, so the hit lands on the first
            // overlapping pixel.
            assert_eq!(
                hit_dot,
                Some(sprite_x as u16 + 1),
                "sprite at x = {sprite_x} should hit on its own pixel"
            );
        }
    }

    /// Both of a sprite's pattern planes come from their own eight bytes of the
    /// tile: phase 4 reads row `r`, phase 6 reads `r + 8`. Reading `r` for both
    /// hands the low plane in as the high one, and that halves what a sprite can
    /// draw — every pixel whose planes are (0, 1) is colour 2, and read back as
    /// (0, 0) it is dropped as transparent. Super Mario Bros' sprite is the
    /// plain sight of it: half its colours, the rest see-through.
    #[test]
    fn test_a_sprites_high_plane_is_the_tiles_second_half() {
        let mut ppu = ppu_with(&[], &[]);
        ppu.mask = PpuMask::SHOW_SPRITES | PpuMask::SHOW_SPRITES_LEFT;
        ppu.palette[0x11] = 0x21;
        ppu.palette[0x12] = 0x2A;

        // Tile 2's row 0, in pattern table 0: the low plane lights column 6
        // only (bit 7 is the leftmost pixel), the high plane column 5 only.
        // Column 5 is therefore colour 2 and column 6 colour 1.
        for row in 0..8 {
            ppu.vram[2 * 16 + row] = 0b0000_0010;
            ppu.vram[2 * 16 + 8 + row] = 0b0000_0100;
        }

        let mut slots = [SpriteSlot::default(); 8];
        slots[0] = SpriteSlot {
            y: 0,
            tile: 2,
            attributes: 0,
            x: 0,
            is_sprite_zero: false,
            occupied: true,
        };

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        pipeline.set_sprite_slots(slots);
        // Line 0's sprite phase fetches the pattern bytes for line 1.
        run_line(&mut pipeline, &mut ppu, 0);
        run_line(&mut pipeline, &mut ppu, 1);

        let frame = pipeline.frame_buffer();
        assert_eq!(
            frame.get_pixel(5, 1),
            nes_color(ppu.palette[0x12], ppu.mask, pipeline.region),
            "column 5 is (low 0, high 1): colour 2, drawn"
        );
        assert_eq!(
            frame.get_pixel(6, 1),
            nes_color(ppu.palette[0x11], ppu.mask, pipeline.region),
            "column 6 is (low 1, high 0): colour 1"
        );
    }

    /// A transparent pixel does not hide the sprite behind it, and a sprite
    /// behind an opaque background pixel does not draw over it.
    #[test]
    fn test_sprite_priority_and_transparency() {
        let mut ppu = ppu_with(&[(0, 1)], &[(1, 0xF0)]);
        fill_tile_rows(&mut ppu, 1, 0xF0);
        ppu.mask = PpuMask::SHOW_BG
            | PpuMask::SHOW_BG_LEFT
            | PpuMask::SHOW_SPRITES
            | PpuMask::SHOW_SPRITES_LEFT;
        ppu.v = 0x0000;
        ppu.t = 0x0000;
        // Palette 0 of the sprite palettes, colour 3 (both pattern planes are
        // lit on the sprite's rows).
        ppu.palette[0x13] = 0x37;

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);

        // Slot 0's tile is blank (transparent) where slot 1 has a pixel, and
        // slot 1 is drawn behind the background.
        let mut slots = [SpriteSlot::default(); 8];
        slots[0] = SpriteSlot {
            y: 0,
            tile: 3,
            attributes: 0,
            x: 0,
            is_sprite_zero: false,
            occupied: true,
        };
        slots[1] = SpriteSlot {
            y: 0,
            tile: 4,
            attributes: 0b0010_0000,
            x: 0,
            is_sprite_zero: false,
            occupied: true,
        };
        for row in 0..8 {
            // Both planes, so the sprite's pixels are colour 3: an opaque pixel
            // is what the priority test needs, and the low plane alone would be
            // colour 1 (the high plane is the tile's second eight bytes).
            ppu.vram[4 * 16 + row] = 0xFF;
            ppu.vram[4 * 16 + 8 + row] = 0xFF;
        }
        pipeline.set_sprite_slots(slots);
        run_line(&mut pipeline, &mut ppu, 0);
        run_line(&mut pipeline, &mut ppu, 1);

        // The background covers x = 0..3 (its pattern is 0b1111_0000) and the
        // sprite is behind it there, so only x = 4..7 gets the sprite colour.
        let sprite_color = nes_color(0x37, ppu.mask, pipeline.region);
        let drawn: Vec<usize> = (0..8)
            .filter(|&x| pipeline.frame_buffer().get_pixel(x, 1) == sprite_color)
            .collect();
        assert_eq!(drawn, vec![4, 5, 6, 7]);
    }
}
