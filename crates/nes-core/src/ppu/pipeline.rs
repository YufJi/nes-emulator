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
//! | 0 | nametable byte fetch, coarse X increment |
//! | 2 | attribute byte fetch |
//! | 4 | pattern low byte fetch |
//! | 6 | pattern high byte fetch |
//! | 7 | shift registers reload from the latches |
//!
//! The shift registers shift left on every dot of the line, and a pixel is
//! drawn on dots 1-256: the register's bit `15 - fine_x` is the current pixel,
//! which makes the tile fetched during one group appear in the next one. Groups
//! 321-336 refetch the first two tiles of the *following* line, which is where
//! a scanline's first tile comes from.

use crate::ppu::rendering::{nes_color, ChrSource, FrameBuffer, Mirroring};
use crate::ppu::{Ppu, PpuMask};
use crate::prelude::*;

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
    /// Address of the last access the pipeline made, for the cartridge's A12
    /// filter. `None` on dots that made no access.
    bus_access: Option<u16>,
    /// The sprite phase's inputs for the line being drawn, one entry per
    /// secondary-OAM slot: the pattern table the slot's tile comes from, and
    /// the tile itself.
    ///
    /// The sprite *evaluation* that fills these is still done a line at a time;
    /// what runs at the dot is the fetch, which is what the cartridge's A12
    /// filter sees.
    sprite_slots: [SpriteSlot; 8],
}

/// One sprite's fetch inputs: which pattern table its tile is read from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpriteSlot {
    /// Pattern-table base ($0000 or $1000).
    pub pattern_table: u16,
    /// Tile index; $FF is what the chip fetches for an empty slot.
    pub tile: u8,
}

impl crate::state::StateField for SpriteSlot {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u16(self.pattern_table);
        w.u8(self.tile);
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        self.pattern_table = r.u16();
        self.tile = r.u8();
    }
}

crate::state::state_fields!(PixelPipeline {
    shifters,
    tile_low,
    tile_high,
    nametable_byte,
    attribute_byte,
    frame,
    bg_opaque,
    mirroring,
    sprite_slots,
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
            bus_access: None,
            sprite_slots: [SpriteSlot::default(); 8],
        }
    }

    /// Hand the sprite phase the line's secondary-OAM slots.
    pub fn set_sprite_slots(&mut self, slots: [SpriteSlot; 8]) {
        self.sprite_slots = slots;
    }

    /// Forget the shift registers, as the chip does at the pre-render line.
    pub fn reset_shifters(&mut self) {
        self.shifters.reset();
    }

    /// Follow the cartridge's mirroring, for the nametable fetches.
    pub fn set_mirroring(&mut self, mirroring: Mirroring) {
        self.mirroring = mirroring;
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
                    let info = self.sprite_slots[slot];
                    let addr = info.pattern_table + (info.tile as u16) * 16;
                    let _ = chr.read_pattern(ppu, addr + (cycle - 257) % 8 - 4);
                    self.bus_access = Some(addr);
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
                let backdrop = nes_color(ppu.palette[0], ppu.mask);
                self.frame.set_pixel(x, scanline as usize, backdrop);
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
        }
    }

    /// Fetch the nametable byte for the tile being fetched and step `v` along.
    fn fetch_nametable(&mut self, ppu: &mut Ppu, chr: &mut dyn ChrSource) {
        let addr = 0x2000 | (ppu.v & 0x0FFF);
        self.nametable_byte = self.read_nametable(ppu, chr, addr);
        self.bus_access = Some(addr);
        increment_coarse_x(ppu);
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
            let color = nes_color(ppu.palette[palette_addr], ppu.mask);
            self.frame.set_pixel(x, y, color);
            self.bg_opaque[y * WIDTH + x] = 1;
        } else {
            // Lazy, because most calls land here and the palette lookup is not
            // free: this is the hottest line in the emulator.
            let backdrop = nes_color(ppu.palette[0], ppu.mask);
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
fn increment_coarse_x(ppu: &mut Ppu) {
    if ppu.v & 0x001F == 0x001F {
        ppu.v &= !0x001F;
        ppu.v ^= 0x0400;
    } else {
        ppu.v += 1;
    }
}

/// `v`'s coarse Y and fine Y increment at dot 256, wrapping at the bottom of a
/// nametable.
fn increment_y(ppu: &mut Ppu) {
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
            .filter(|&x| pipeline.frame_buffer().get_pixel(x, 0) == nes_color(0x30, ppu.mask))
            .collect();
        assert_eq!(drawn, vec![0], "only pixel 0 of the tile is set");
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
            .filter(|&x| pipeline.frame_buffer().get_pixel(x, 0) == nes_color(0x30, ppu.mask))
            .collect();
        assert_eq!(drawn, vec![0]);
    }
}
