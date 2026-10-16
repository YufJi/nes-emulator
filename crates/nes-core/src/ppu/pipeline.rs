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
    /// The line's secondary-OAM bytes, as the fetch reads them (Mesen's
    /// `_secondarySpriteRam`).
    ///
    /// The bus hands these over on dot 257 — `LoadSpriteTileInfo` reads them on
    /// dots 261, 269, … 317, and the walk that fills secondary OAM is over by
    /// dot 256, so one copy at 257 is the same bytes.
    secondary_oam: [u8; 0x20],
    /// The pattern bytes each of the eight slots was fetched for (Mesen's
    /// `_spriteTiles` — eight of its sixty-four entries, since the sprite limit
    /// is not removed).
    sprite_tiles: [SpriteInfo; 8],
    /// Each slot's place on the line, encoded as `(x + 1) << 4 | slot` — the dot
    /// its shifter becomes live on (Mesen's `_spriteShifterList`).
    ///
    /// Nine entries, because the sweep walks a sentinel: entry 8 is always
    /// [`SPRITE_SHIFTER_DONE`], which is what stops it.
    sprite_shifter_list: [u16; 9],
    /// Where the sweep of `sprite_shifter_list` stands (Mesen's
    /// `_nextSpriteShifter` and `_nextSpriteShifterCycle`).
    next_sprite_shifter: usize,
    next_sprite_shifter_cycle: u16,
    /// The shifters whose pixels are being output, the ones waiting for their X
    /// to come round, and the ones that have finished (Mesen's `_activeSpriteShifters`,
    /// `_countingSpriteShifters` and `_expiredSpriteShifters`).
    active_sprite_shifters: u8,
    counting_sprite_shifters: u8,
    expired_sprite_shifters: u8,
    /// How many sprites the line's fetch found, which is what decides whether
    /// the sprite half of the pixel runs at all (Mesen's `_spriteCount` and
    /// `_processSprites`).
    sprite_count: u8,
    process_sprites: bool,
    /// Whether the line's first secondary-OAM entry is treated as sprite 0
    /// (Mesen's `_sprite0Visible`, copied from `_sprite0Added`).
    sprite_0_visible: bool,
    /// The dots the background and sprite pixels are clipped on, from the mask
    /// at the start of the frame and on every `$2001` write (Mesen's
    /// `_minimumDrawBgCycle`, `_minimumDrawSpriteCycle` and
    /// `_minimumDrawSpriteStandardCycle`).
    minimum_draw_bg_cycle: u16,
    minimum_draw_sprite_cycle: u16,
    minimum_draw_sprite_standard_cycle: u16,
}

/// One sprite's fetched pattern bytes and the attributes that go with them —
/// Mesen's `NesSpriteInfo`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SpriteInfo {
    /// Attributes bit 5: draw the sprite behind an opaque background pixel.
    background_priority: bool,
    /// OAM byte 3: the sprite's leftmost column.
    sprite_x: u8,
    /// The two pattern bytes, already mirrored if the sprite is.
    low: u8,
    high: u8,
    /// `(attributes & 3) << 2 | 0x10`, the palette the sprite draws in.
    palette_offset: u8,
}

crate::state::state_fields!(SpriteInfo {
    background_priority,
    sprite_x,
    low,
    high,
    palette_offset,
});

/// Mesen's `SpriteShifterDone`: a shifter-list entry that names no sprite.
const SPRITE_SHIFTER_DONE: u16 = 0x8000;

/// Where a slot's pattern bytes are on the line the chip is fetching for, and
/// whether the sprite is on that line at all — Mesen's `LoadSprite`, less the
/// work it does for the shifters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SpriteFetch {
    /// Address of the tile's first row.
    addr: u16,
    /// The row within the tile, which is `addr + row` and `addr + row + 8`.
    row: u16,
    /// Whether the sprite covers the line being fetched for.
    in_range: bool,
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
    region,
    secondary_oam,
    sprite_tiles,
    sprite_shifter_list,
    next_sprite_shifter,
    next_sprite_shifter_cycle,
    active_sprite_shifters,
    counting_sprite_shifters,
    expired_sprite_shifters,
    sprite_count,
    process_sprites,
    sprite_0_visible,
    minimum_draw_bg_cycle,
    minimum_draw_sprite_cycle,
    minimum_draw_sprite_standard_cycle,
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
            secondary_oam: [0xFF; 0x20],
            sprite_tiles: [SpriteInfo::default(); 8],
            sprite_shifter_list: [SPRITE_SHIFTER_DONE; 9],
            next_sprite_shifter: 0,
            next_sprite_shifter_cycle: 0,
            active_sprite_shifters: 0,
            counting_sprite_shifters: 0,
            expired_sprite_shifters: 0,
            sprite_count: 0,
            process_sprites: false,
            sprite_0_visible: false,
            minimum_draw_bg_cycle: 0,
            minimum_draw_sprite_cycle: 0,
            minimum_draw_sprite_standard_cycle: 0,
        }
    }

    /// Hand the sprite phase the line's secondary-OAM slots.
    /// Hand over the line's secondary OAM and whether its first entry counts as
    /// sprite 0, on the dot the chip starts its sprite fetch.
    ///
    /// Mesen copies `_sprite0Added` into `_sprite0Visible` on every dot of
    /// 257-320 and reads `_secondarySpriteRam` at each fetch dot; the walk that
    /// writes both is over by dot 256, so one copy at 257 carries the same
    /// values.
    pub fn begin_sprite_fetch(&mut self, secondary_oam: [u8; 0x20], sprite_0_added: bool) {
        self.secondary_oam = secondary_oam;
        self.sprite_0_visible = sprite_0_added;
        self.sprite_count = 0;
    }

    /// The shifter list, for the diagnostics and the tests that ask which dot a
    /// sprite's pixels start on.
    pub fn sprite_shifters(&self) -> &[u16; 9] {
        &self.sprite_shifter_list
    }

    /// Recompute the left-edge clip dots (Mesen's `UpdateMinimumDrawCycles`).
    ///
    /// Mesen calls this on a reset, at the top of the frame, and from
    /// `SetMaskRegister` — so a mid-frame walk with the left-column bits does
    /// move the clip dots, while the frame's *first* dots come from the mask as
    /// it stood when the frame began.
    pub fn update_minimum_draw_cycles(&mut self, mask: PpuMask) {
        self.minimum_draw_bg_cycle = if mask.contains(PpuMask::SHOW_BG) {
            if mask.contains(PpuMask::SHOW_BG_LEFT) {
                0
            } else {
                8
            }
        } else {
            300
        };
        self.minimum_draw_sprite_cycle = if mask.contains(PpuMask::SHOW_SPRITES) {
            if mask.contains(PpuMask::SHOW_SPRITES_LEFT) {
                0
            } else {
                8
            }
        } else {
            300
        };
        self.minimum_draw_sprite_standard_cycle = if mask.contains(PpuMask::SHOW_SPRITES) {
            if mask.contains(PpuMask::SHOW_SPRITES_LEFT) {
                0
            } else {
                8
            }
        } else {
            300
        };
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

    /// The secondary-OAM slots the sprite phase is holding for this line.
    /// Run the pipeline for the dot the PPU has just advanced to.
    ///
    /// The PPU's own `scanline`/`cycle` are the input: the machine counts the
    /// dots, and this reads the position it is standing on. `pre_render` is the
    /// region's last scanline, which the pipeline treats as part of the visible
    /// range (Mesen's `_scanline == -1`).
    #[inline]
    pub fn step(
        &mut self,
        ppu: &mut Ppu,
        chr: &mut dyn ChrSource,
        pre_render: u16,
        rendering_delayed: bool,
    ) {
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
        // away, then the slot's pattern bytes — both planes in the one dot, as
        // Mesen's `LoadSpriteTileInfo` does. The thrown-away reads matter to the
        // cartridge even though nothing uses them — they are what takes A12 low
        // between two sprites' pattern fetches, which is what an MC-ACC clone's
        // IRQ is built on.
        //
        // The window hangs off the rendering state and *not* off the sprite
        // enable bit: Mesen gates the whole of 257-320 on `_prevRenderingEnabled`
        // (`LoadSpriteTileInfo` asks nothing else), so with the background on and
        // sprites off the chip still fetches. The bytes are then never drawn,
        // because the drawing half is what asks `$2001`'s sprite bit.
        if rendering_delayed && (257..=320).contains(&cycle) {
            match (cycle - 257) % 8 {
                0 | 2 => {
                    let addr = 0x2000 | (ppu.v & 0x0FFF);
                    let _ = self.read_nametable(ppu, chr, addr);
                    self.bus_access = Some(addr);
                }
                4 => self.load_sprite_tile_info(ppu, chr, ((cycle - 257) / 8) as usize),
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

        // The sprite shifters are stepped for this dot *before* the pixel is
        // built, and they are stepped whether or not the picture is being drawn
        // — Mesen's `ProcessRenderingDisabledPixel` runs the same sweep.
        if scanline < 240 && (1..=256).contains(&cycle) {
            self.process_sprite_shifters(cycle);
        }

        if scanline < 240 && (1..=256).contains(&cycle) {
            let x = (cycle - 1) as usize;
            let row = scanline as usize * WIDTH;
            if x == 0 {
                self.bg_opaque[row..row + WIDTH].fill(0);
            }
            if show_bg {
                self.draw_background_pixel(ppu, x);
            } else if !rendering && ppu.v & 0x3F00 == 0x3F00 {
                // "If the current VRAM address points in the range
                // $3F00-$3FFF during forced blanking, the color indicated by
                // this palette location will be shown on screen instead of the
                // backdrop color" (Mesen's `DefaultNesPpu::DrawPixel`). It is
                // what `full_palette` draws its grid with: with rendering off
                // the program aims `v` at the entry it wants on the wire.
                let color = nes_color(ppu.palette_at(ppu.v), ppu.mask, self.region);
                self.frame.set_pixel(x, scanline as usize, color);
            } else {
                // With the background off the chip puts the backdrop on the
                // wire, and sprites draw over it.
                let backdrop = nes_color(ppu.palette[0], ppu.mask, self.region);
                self.frame.set_pixel(x, scanline as usize, backdrop);
            }
            self.draw_sprites(ppu, cycle, x, rendering_delayed);
        } else if cycle == 0 {
            // Dot 0 belongs to this line's shifters: Mesen restarts them here,
            // sorting the list by X and starting the sweep at its head
            // (`ProcessScanlineFirstCycle` runs for `_scanline < 240`, which
            // includes the pre-render line).
            self.begin_sprite_line();
        } else if cycle == 339 {
            // Dot 339 is where the line's shifters are handed on: the active
            // mask is dropped, and the ones whose X never came go into counting
            // mode so the sweep activates them.
            self.finish_sprite_line(rendering_delayed);
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

    /// Mesen's `LoadSprite`: where a slot's pattern bytes live for the line
    /// being fetched, and whether the sprite is on that line at all.
    ///
    /// The line the chip compares against is truncated to eight bits — the
    /// pre-render line is 261 here and -1 in Mesen, so both sides end up using
    /// **5** — and the difference is a signed one widened to sixteen bits, so a
    /// sprite whose Y is *below* it comes out as a large number and is rejected.
    /// That rejection is what keeps a secondary-OAM entry left over from the
    /// previous line from being drawn as a sprite on line 0, and it is why the
    /// pre-render fetch is the "dummy `$FF` tiles" path Mesen describes.
    ///
    /// Two ROM checks measure it: `sprite_hit_tests/07.screen_bottom`'s check 4
    /// (a Y of 255 must never hit) and AccuracyCoin's `Sprite0Hit_Behavior`.
    fn sprite_fetch(&self, ppu: &Ppu, sprite_y: u8, attributes: u8, tile_index: u8) -> SpriteFetch {
        let (height, size_mask) = if ppu.ctrl.contains(crate::ppu::PpuCtrl::SPRITE_SIZE) {
            (16u16, 15u16)
        } else {
            (8u16, 7u16)
        };

        // `(uint8_t)_scanline`: 261 becomes 5, 311 becomes 55, visible lines are
        // themselves. Then the subtraction Mesen's `uint16_t rangeResult` gets.
        let scanline8 = (ppu.scanline & 0xFF) as i32;
        let mut range = (scanline8 - sprite_y as i32) as u16;
        if attributes & 0x80 != 0 {
            range ^= size_mask;
        }

        // `(((tileIndex & 0x01) << 12) | ((tileIndex & ~0x01) << 4))` for 8x16:
        // the pattern table comes from bit 0 of the tile index and the second
        // tile from bit 3 of the row. 8x8 takes its table from `$2000` and keeps
        // the tile index whole.
        let addr = if height == 16 {
            let table = if tile_index & 1 != 0 { 0x1000 } else { 0x0000 };
            table | ((tile_index & 0xFE) as u16) << 4 | ((range & 0x08) << 1)
        } else {
            let table = if ppu.ctrl.contains(crate::ppu::PpuCtrl::SPRITE_PATTERN) {
                0x1000
            } else {
                0x0000
            };
            table | (tile_index as u16) << 4
        };

        SpriteFetch {
            addr,
            row: range & 0x07,
            in_range: range <= size_mask,
        }
    }

    /// Mesen's `LoadSpriteTileInfo`: fetch one slot's sprite on the dot the chip
    /// does, out of the secondary-OAM bytes as they stand.
    ///
    /// The slot comes from the dot (`(cycle - 257) >> 3`), not from a counter, so
    /// a line whose fetch starts half way through still lands on the right entry.
    /// Both pattern planes are read in this one dot — Mesen puts them both here —
    /// and that is also what the cartridge sees, so the bus is low between two
    /// sprites' fetches for one dot rather than two.
    fn load_sprite_tile_info(&mut self, ppu: &mut Ppu, chr: &mut dyn ChrSource, slot: usize) {
        let bytes = &self.secondary_oam[slot * 4..slot * 4 + 4];
        let (sprite_y, tile_index, attributes, sprite_x) = (bytes[0], bytes[1], bytes[2], bytes[3]);
        let fetch = self.sprite_fetch(ppu, sprite_y, attributes, tile_index);
        let pattern_addr = fetch.addr + fetch.row;

        let mut info = SpriteInfo {
            background_priority: attributes & 0x20 != 0,
            sprite_x,
            palette_offset: ((attributes & 0x03) << 2) | 0x10,
            low: chr.read_pattern(ppu, pattern_addr),
            high: chr.read_pattern(ppu, pattern_addr + 8),
        };
        self.bus_access = Some(pattern_addr + 8);

        if fetch.in_range {
            if attributes & 0x40 != 0 {
                info.low = info.low.reverse_bits();
                info.high = info.high.reverse_bits();
            }
            self.sprite_tiles[slot] = info;
            // The sweep runs by X, so the entry carries the dot the shifter goes
            // live on: one past the sprite's leftmost pixel.
            self.sprite_shifter_list[slot] = ((u16::from(sprite_x) + 1) << 4) | slot as u16;
            self.expired_sprite_shifters &= !(1u8 << slot);
            self.sprite_count += 1;
            self.process_sprites = true;
        } else {
            // Out of range: the bytes are thrown away and the shifter is marked
            // done, which is what keeps an empty slot — or one left over from the
            // previous line — from ever being drawn.
            self.sprite_tiles[slot] = SpriteInfo {
                low: 0,
                high: 0,
                ..info
            };
            self.sprite_shifter_list[slot] = SPRITE_SHIFTER_DONE;
        }
    }

    /// Mesen's `ProcessSpriteShifters`: the sweep that makes a sprite's shifters
    /// live on its own dot.
    ///
    /// The list is in X order, so this walks its head: every entry whose dot has
    /// arrived and which was still waiting becomes *active* — and counts as
    /// finished, because from here on it shifts.
    fn process_sprite_shifters(&mut self, cycle: u16) {
        if self.next_sprite_shifter_cycle != cycle {
            return;
        }
        while self.sprite_shifter_list[self.next_sprite_shifter] >> 4 == cycle {
            let bit = 1u8 << (self.sprite_shifter_list[self.next_sprite_shifter] & 0x07);
            if self.counting_sprite_shifters & bit != 0 {
                self.active_sprite_shifters |= bit;
                self.expired_sprite_shifters |= bit;
                self.counting_sprite_shifters &= !bit;
            }
            self.next_sprite_shifter += 1;
        }
        self.next_sprite_shifter_cycle = self.sprite_shifter_list[self.next_sprite_shifter] >> 4;
    }

    /// The line's shifter state at its first dot (Mesen's
    /// `ProcessScanlineFirstCycle`): the list is sorted by X and the sweep starts
    /// at its head.
    fn begin_sprite_line(&mut self) {
        self.sprite_shifter_list[..8].sort_unstable();
        self.next_sprite_shifter = 0;
        self.next_sprite_shifter_cycle = self.sprite_shifter_list[0] >> 4;
    }

    /// Hand the line's shifters to the next one (Mesen's dot-339 block).
    ///
    /// What has not gone out yet goes into counting mode, so the sweep activates
    /// it; what has already been counted is activated outright, so its pixels
    /// start from X 0 on the next line.
    fn finish_sprite_line(&mut self, rendering_delayed: bool) {
        self.active_sprite_shifters = 0;

        if rendering_delayed {
            for index in 0..8 {
                let entry = self.sprite_shifter_list[index];
                if entry == SPRITE_SHIFTER_DONE {
                    continue;
                }
                let bit = 1u8 << (entry & 0x07);
                if self.expired_sprite_shifters & bit == 0 {
                    self.counting_sprite_shifters |= bit;
                }
            }
        }

        for index in 0..8 {
            let entry = self.sprite_shifter_list[index];
            if entry == SPRITE_SHIFTER_DONE {
                continue;
            }
            let bit = 1u8 << (entry & 0x07);
            if self.counting_sprite_shifters & bit == 0 {
                self.active_sprite_shifters |= bit;
            }
        }

        self.process_sprites = self.sprite_count != 0 || self.active_sprite_shifters != 0;
    }

    /// The sprite half of the pixel: shift every live sprite's bytes, take the
    /// frontmost opaque one, and raise the sprite-0 hit on it.
    ///
    /// The shifters are shifted *here*, once per dot, and only the active ones —
    /// a sprite whose X has not come round must keep its bytes, or its pixels
    /// would start too early. That is the whole point of sweeping the list rather
    /// than picking a slot by X at draw time.
    fn draw_sprites(&mut self, ppu: &mut Ppu, cycle: u16, x: usize, rendering_delayed: bool) {
        if !self.process_sprites || !rendering_delayed {
            return;
        }

        let mut remaining = self.active_sprite_shifters;
        let mut chosen: Option<(usize, u8)> = None;
        while remaining != 0 {
            // Mesen sweeps from the highest slot *down*, so a lower slot — the
            // higher-priority sprite — is the last one kept.
            let slot = (7 - remaining.leading_zeros()) as usize;
            remaining &= !(1u8 << slot);

            let sprite = &mut self.sprite_tiles[slot];
            let color = ((sprite.high >> 6) & 0x02) | (sprite.low >> 7);
            if color != 0 {
                chosen = Some((slot, color));
            }
            sprite.high <<= 1;
            sprite.low <<= 1;
            if sprite.high | sprite.low == 0 {
                // The shifter has shifted its last pixel out.
                self.active_sprite_shifters &= !(1u8 << slot);
                self.process_sprites = self.sprite_count != 0 || self.active_sprite_shifters != 0;
            }
        }

        let Some((slot, color)) = chosen else {
            return;
        };
        // The sprite half only reaches the wire past the clip dot, and only when
        // `$2001` says sprites are on: the *fetch* runs either way.
        if cycle <= self.minimum_draw_sprite_cycle || !ppu.mask.contains(PpuMask::SHOW_SPRITES) {
            return;
        }

        let sprite = self.sprite_tiles[slot];
        let bg_opaque = self.bg_opaque[ppu.scanline as usize * WIDTH + x] != 0;

        // The hit needs an opaque background pixel *and* an opaque sprite-0 pixel,
        // the left edges unclipped, and it never fires in the last column.
        if self.sprite_0_visible
            && slot == 0
            && bg_opaque
            && cycle != 256
            && cycle > self.minimum_draw_sprite_standard_cycle
            && ppu.mask.contains(PpuMask::SHOW_BG)
            && !ppu.status.contains(PpuStatus::SPRITE_0_HIT)
        {
            ppu.status.insert(PpuStatus::SPRITE_0_HIT);
        }

        if !(sprite.background_priority && bg_opaque) {
            let index = sprite.palette_offset.wrapping_add(color) & 0x1F;
            let color = nes_color(ppu.palette[index as usize], ppu.mask, self.region);
            self.frame.set_pixel(x, ppu.scanline as usize, color);
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
            pipeline.step(ppu, &mut chr, 261, true);
        }
    }

    /// A line's secondary OAM as the walk leaves it: eight four-byte sprites,
    /// `$FF` in the slots nothing was found for.
    fn secondary_oam(sprites: &[(u8, u8, u8, u8)]) -> [u8; 0x20] {
        let mut bytes = [0xFF; 0x20];
        for (index, &(y, tile, attributes, x)) in sprites.iter().enumerate() {
            let at = index * 4;
            bytes[at] = y;
            bytes[at + 1] = tile;
            bytes[at + 2] = attributes;
            bytes[at + 3] = x;
        }
        bytes
    }

    /// Run one line's dots, letting `at_dot` change the registers as the line
    /// goes — which is how a test puts a `$2000`/`$2001` write on a chosen dot.
    fn run_line_with(
        pipeline: &mut PixelPipeline,
        ppu: &mut Ppu,
        scanline: u16,
        mut at_dot: impl FnMut(&mut Ppu, u16),
    ) {
        ppu.scanline = scanline;
        for dot in 0..=340u16 {
            ppu.cycle = dot;
            at_dot(ppu, dot);
            let mut chr = VramChr;
            pipeline.step(ppu, &mut chr, 261, true);
        }
    }

    /// The sprite phase's fetch takes `$2000` as it stands *on the fetch dot*.
    ///
    /// `Suddenly Resize Sprite` is the ROM that measures it: the sprite size bit
    /// flips between two slots' fetches, and what decides whether a slot reaches
    /// the shifters is the range test — `scanline - Y <= size_mask` — read with
    /// the size of the moment. A sprite ten lines above the line being fetched
    /// for is out of range at eight pixels high and in range at sixteen.
    #[test]
    fn test_the_fetch_takes_the_control_register_as_it_stands_on_the_fetch_dot() {
        let mut ppu = ppu_with(&[], &[]);
        ppu.mask = PpuMask::SHOW_SPRITES | PpuMask::SHOW_SPRITES_LEFT;
        ppu.palette[0x11] = 0x30;
        // Two sprites, both a Y of 0 on line 10: `range = 10 - 0 = 10`, which is
        // past the eighth pixel of an 8x8 sprite and inside a 16x16 one.
        let bytes = secondary_oam(&[(0, 2, 0, 0), (0, 2, 0, 8)]);

        let mut pipeline = PixelPipeline::new();
        pipeline.begin_sprite_fetch(bytes, false);
        run_line_with(&mut pipeline, &mut ppu, 10, |ppu, dot| {
            // The fetch for line 11 runs on line 10: slot 0 at dot 261, slot 1 at
            // 269. Eight-pixel sprites for the first, sixteen for the second.
            ppu.ctrl = if dot < 262 {
                crate::ppu::PpuCtrl::empty()
            } else {
                crate::ppu::PpuCtrl::SPRITE_SIZE
            };
        });

        let shifters = *pipeline.sprite_shifters();
        assert_eq!(
            shifters[0], SPRITE_SHIFTER_DONE,
            "slot 0 was fetched while $2000 said 8x8, so its Y is out of range"
        );
        assert!(
            shifters[1] != SPRITE_SHIFTER_DONE,
            "slot 1 was fetched after $2000 said 16x16, so the same Y is in range"
        );
    }

    /// The fetch window hangs off the rendering state, not off `$2001`'s sprite
    /// bit: with the background on and sprites off the chip still fetches, and
    /// Mesen says so by gating the whole of 257-320 on `_prevRenderingEnabled`
    /// alone.
    ///
    /// The bytes are then never *drawn*, because the drawing half is what asks
    /// the sprite bit — which is the second half of this test.
    #[test]
    fn test_the_fetch_runs_with_the_sprite_enable_bit_clear() {
        let mut ppu = ppu_with(&[], &[]);
        ppu.mask = PpuMask::SHOW_BG;
        let bytes = secondary_oam(&[(0, 2, 0, 0)]);

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        pipeline.begin_sprite_fetch(bytes, false);
        run_line(&mut pipeline, &mut ppu, 0);
        run_line(&mut pipeline, &mut ppu, 1);

        let shifter = pipeline.sprite_shifters()[0];
        assert_ne!(
            shifter, SPRITE_SHIFTER_DONE,
            "the fetch runs with the sprite bit clear"
        );
        let backdrop = nes_color(ppu.palette[0], ppu.mask, pipeline.region);
        assert_eq!(
            pipeline.frame_buffer().get_pixel(0, 1),
            backdrop,
            "and its pixels are not drawn, because $2001 says sprites are off"
        );
    }

    /// A shifter goes live on the dot its sprite's X names — one past the
    /// sprite's leftmost pixel — and not before: that is what sweeping the list
    /// buys over picking a slot by X at draw time.
    #[test]
    fn test_a_shifter_activates_at_sprite_x_plus_one() {
        let mut ppu = ppu_with(&[], &[]);
        ppu.mask = PpuMask::SHOW_SPRITES | PpuMask::SHOW_SPRITES_LEFT;
        ppu.palette[0x11] = 0x30;
        let bytes = secondary_oam(&[(0, 2, 0, 3)]);
        // Tile 2's row lights its leftmost pixel only.
        for row in 0..8 {
            ppu.vram[2 * 16 + row] = 0x80;
        }

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        pipeline.begin_sprite_fetch(bytes, false);
        run_line(&mut pipeline, &mut ppu, 0);

        assert_eq!(
            pipeline.sprite_shifters()[0],
            4 << 4,
            "the entry carries the dot the shifter goes live on"
        );
        // The fetch line draws nothing: the sprites it fetches belong to the line
        // after it, and the dot-339 handoff is what passes them on.
        let backdrop = nes_color(ppu.palette[0], ppu.mask, pipeline.region);
        assert_eq!(pipeline.frame_buffer().get_pixel(3, 0), backdrop);

        run_line(&mut pipeline, &mut ppu, 1);
        let sprite_color = nes_color(ppu.palette[0x11], ppu.mask, pipeline.region);
        assert_eq!(
            pipeline.frame_buffer().get_pixel(3, 1),
            sprite_color,
            "the sprite's pixel is at its X, on the line after the fetch"
        );
        assert_eq!(
            pipeline.frame_buffer().get_pixel(2, 1),
            backdrop,
            "and not one dot early"
        );
    }

    /// The list is sorted at dot 0, and that is load-bearing: the sweep walks it
    /// from the head, so an entry whose X sits *behind* a later one in the list
    /// would not go live until the later X came round — its pixels would simply be
    /// missing from the screen.
    #[test]
    fn test_the_shifter_list_is_sorted_at_dot_zero() {
        let mut ppu = ppu_with(&[], &[]);
        ppu.mask = PpuMask::SHOW_SPRITES | PpuMask::SHOW_SPRITES_LEFT;
        ppu.palette[0x11] = 0x30;
        for row in 0..8 {
            ppu.vram[2 * 16 + row] = 0x80;
        }
        // Slot 0 is the far sprite and slot 1 the near one, so the fetch writes
        // the list out of X order.
        let bytes = secondary_oam(&[(0, 2, 0, 100), (0, 2, 0, 10)]);

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        pipeline.begin_sprite_fetch(bytes, false);
        run_line(&mut pipeline, &mut ppu, 0);
        run_line(&mut pipeline, &mut ppu, 1);

        let sprite_color = nes_color(ppu.palette[0x11], ppu.mask, pipeline.region);
        assert_eq!(
            pipeline.frame_buffer().get_pixel(10, 1),
            sprite_color,
            "the near sprite's pixel lands on its own dot"
        );
        assert_eq!(
            pipeline.frame_buffer().get_pixel(100, 1),
            sprite_color,
            "and so does the far one's"
        );
    }

    /// The dot-339 handoff, and the dot-0 restart that goes with it: a shifter
    /// whose X never came round during the fetch line is carried into the next
    /// line and activated there.
    #[test]
    fn test_dot_339_hands_the_unreached_shifters_over() {
        let mut ppu = ppu_with(&[], &[]);
        ppu.mask = PpuMask::SHOW_SPRITES | PpuMask::SHOW_SPRITES_LEFT;
        ppu.palette[0x11] = 0x30;
        // X = 200: its dot (201) is far past the fetch dot, so the sweep never
        // reaches it while the line is being fetched.
        let bytes = secondary_oam(&[(0, 2, 0, 200)]);
        for row in 0..8 {
            ppu.vram[2 * 16 + row] = 0x80;
        }

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        pipeline.begin_sprite_fetch(bytes, false);
        run_line(&mut pipeline, &mut ppu, 0);
        run_line(&mut pipeline, &mut ppu, 1);

        let sprite_color = nes_color(ppu.palette[0x11], ppu.mask, pipeline.region);
        let backdrop = nes_color(ppu.palette[0], ppu.mask, pipeline.region);
        assert_eq!(
            pipeline.frame_buffer().get_pixel(200, 1),
            sprite_color,
            "the sprite reaches the wire on the line after its fetch"
        );
        assert_eq!(
            pipeline.frame_buffer().get_pixel(201, 1),
            backdrop,
            "and only the pixel its pattern lights, at its own X"
        );
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
            pipeline.step(&mut ppu, &mut chr, 261, true);
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
            let bytes = secondary_oam(&[(0, 2, 0, sprite_x)]);
            ppu.vram[2 * 16] = 0x80;
            pipeline.begin_sprite_fetch(bytes, true);

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
                pipeline.step(&mut ppu, &mut chr, 261, true);
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

        let mut pipeline = PixelPipeline::new();
        run_line(&mut pipeline, &mut ppu, 261);
        pipeline.begin_sprite_fetch(secondary_oam(&[(0, 2, 0, 0)]), false);
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
        for row in 0..8 {
            // Both planes, so the sprite's pixels are colour 3: an opaque pixel
            // is what the priority test needs, and the low plane alone would be
            // colour 1 (the high plane is the tile's second eight bytes).
            ppu.vram[4 * 16 + row] = 0xFF;
            ppu.vram[4 * 16 + 8 + row] = 0xFF;
        }
        pipeline.begin_sprite_fetch(
            secondary_oam(&[(0, 3, 0, 0), (0, 4, 0b0010_0000, 0)]),
            false,
        );
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
