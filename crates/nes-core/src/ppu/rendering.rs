//! PPU Background Rendering Pipeline
//!
//! This module implements the NES PPU background rendering pipeline.
//! The PPU generates a 256x240 pixel display by processing tiles from
//! nametables and pattern tables.
//!
//! Reference: https://www.nesdev.org/wiki/PPU_rendering

use crate::ppu::{Ppu, PpuCtrl, PpuMask, PpuStatus};
use crate::ppu::sprite::{SpriteRenderer, SecondaryOam};
use crate::ppu::sprite::SpriteEvaluator;
use crate::mapper::Mapper;

/// Supplies pattern-table bytes to the renderers.
///
/// Reading through the cartridge on every fetch keeps mid-frame CHR bank
/// switches correct: MMC3 games flip banks from their IRQ handler and MMC4
/// switches on PPU fetches, neither of which a once-per-frame snapshot can
/// represent. Tests that build the pattern tables directly in VRAM use
/// [`VramChr`].
pub trait ChrSource {
    /// Read a byte from the pattern tables ($0000-$1FFF).
    fn read_pattern(&mut self, ppu: &Ppu, addr: u16) -> u8;
}

/// Reads pattern data straight out of the PPU's VRAM.
///
/// This is the pre-refresh model used by unit tests and by carts where the
/// pattern data already lives in `Ppu::vram`.
pub struct VramChr;

impl ChrSource for VramChr {
    fn read_pattern(&mut self, ppu: &Ppu, addr: u16) -> u8 {
        ppu.vram[(addr & 0x1FFF) as usize]
    }
}

/// Reads pattern data through the cartridge mapper.
pub struct MapperChr<'a>(pub &'a mut dyn Mapper);

impl ChrSource for MapperChr<'_> {
    fn read_pattern(&mut self, _ppu: &Ppu, addr: u16) -> u8 {
        self.0.read(addr & 0x1FFF)
    }
}

/// NES color palette (64 colors, 6-bit values converted to ARGB)
/// These are the standard NES NTSC palette colors.
pub const NES_PALETTE: [u32; 64] = [
    0xFF666666, 0xFF002A88, 0xFF1412A7, 0xFF3B00A4, 0xFF5C007E, 0xFF6E0040, 0xFF6C0600, 0xFF561D00,
    0xFF333500, 0xFF0B4800, 0xFF005200, 0xFF004F08, 0xFF00404D, 0xFF000000, 0xFF000000, 0xFF000000,
    0xFFADADAD, 0xFF155FD9, 0xFF4240FF, 0xFF7527FE, 0xFFA01ACC, 0xFFB71E7B, 0xFFB53120, 0xFF994E00,
    0xFF6B6D00, 0xFF388700, 0xFF0C9300, 0xFF008F32, 0xFF007C8D, 0xFF000000, 0xFF000000, 0xFF000000,
    0xFFFFFEFF, 0xFF64B0FF, 0xFF9290FF, 0xFFC676FF, 0xFFF36AFF, 0xFFFE6ECC, 0xFFFE8170, 0xFFEA9E22,
    0xFFBCBE00, 0xFF88D800, 0xFF5CE430, 0xFF45E082, 0xFF48CDDE, 0xFF4F4F4F, 0xFF000000, 0xFF000000,
    0xFFFFFEFF, 0xFFC0DFFF, 0xFFD3D2FF, 0xFFE8C8FF, 0xFFFBC2FF, 0xFFFEC4EA, 0xFFFECCC5, 0xFFF7D8A5,
    0xFFE4E594, 0xFFCFEF96, 0xFFBDF4AB, 0xFFB3F3CC, 0xFFB5EBF2, 0xFFB8B8B8, 0xFF000000, 0xFF000000,
];

/// How much the two non-emphasised channels are darkened when a colour
/// emphasis bit is set.
const EMPHASIS_ATTENUATION: f32 = 0.75;

/// Convert a 6-bit NES colour index into an ARGB pixel.
///
/// Applies the PPUMASK grayscale bit, which clamps the colour index to the
/// `$x0` column of the palette, and the three colour-emphasis bits, which
/// darken the two channels that are not emphasised.
///
/// # Arguments
/// * `color_index` - Palette RAM value (only the low 6 bits are used)
/// * `mask` - PPUMASK register
pub fn nes_color(color_index: u8, mask: PpuMask) -> u32 {
    let mut index = color_index & 0x3F;
    if mask.contains(PpuMask::GRAYSCALE) {
        index &= 0x30;
    }

    let argb = NES_PALETTE[index as usize];
    let mut r = ((argb >> 16) & 0xFF) as f32;
    let mut g = ((argb >> 8) & 0xFF) as f32;
    let mut b = (argb & 0xFF) as f32;

    if mask.contains(PpuMask::EMPHASIZE_RED) {
        g *= EMPHASIS_ATTENUATION;
        b *= EMPHASIS_ATTENUATION;
    }
    if mask.contains(PpuMask::EMPHASIZE_GREEN) {
        r *= EMPHASIS_ATTENUATION;
        b *= EMPHASIS_ATTENUATION;
    }
    if mask.contains(PpuMask::EMPHASIZE_BLUE) {
        r *= EMPHASIS_ATTENUATION;
        g *= EMPHASIS_ATTENUATION;
    }

    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Background rendering state
///
/// The PPU uses shift registers to hold tile data for the current and next tile.
/// This allows for smooth horizontal scrolling by shifting out one pixel at a time.
#[derive(Debug, Clone)]
pub struct BgRenderState {
    /// Pattern table shift register (low byte) - holds 16 bits for 2 tiles
    pub pattern_shift_low: u16,
    /// Pattern table shift register (high byte) - holds 16 bits for 2 tiles
    pub pattern_shift_high: u16,
    /// Attribute shift register (low bit) - holds palette selection bits
    pub attr_shift_low: u8,
    /// Attribute shift register (high bit) - holds palette selection bits
    pub attr_shift_high: u8,
    /// Attribute latch (low bit) - holds next tile's palette selection
    pub attr_latch_low: u8,
    /// Attribute latch (high bit) - holds next tile's palette selection
    pub attr_latch_high: u8,
    /// Current tile data (low byte) - fetched from pattern table
    pub tile_data_low: u8,
    /// Current tile data (high byte) - fetched from pattern table
    pub tile_data_high: u8,
    /// Current nametable byte - tile index from nametable
    pub nametable_byte: u8,
    /// Current attribute byte - palette selection from attribute table
    pub attribute_byte: u8,
}

impl Default for BgRenderState {
    fn default() -> Self {
        Self::new()
    }
}

impl BgRenderState {
    /// Create new background rendering state
    pub fn new() -> Self {
        Self {
            pattern_shift_low: 0,
            pattern_shift_high: 0,
            attr_shift_low: 0,
            attr_shift_high: 0,
            attr_latch_low: 0,
            attr_latch_high: 0,
            tile_data_low: 0,
            tile_data_high: 0,
            nametable_byte: 0,
            attribute_byte: 0,
        }
    }

    /// Reset the background rendering state
    pub fn reset(&mut self) {
        self.pattern_shift_low = 0;
        self.pattern_shift_high = 0;
        self.attr_shift_low = 0;
        self.attr_shift_high = 0;
        self.attr_latch_low = 0;
        self.attr_latch_high = 0;
        self.tile_data_low = 0;
        self.tile_data_high = 0;
        self.nametable_byte = 0;
        self.attribute_byte = 0;
    }
    /// Shift the shift registers by one pixel (for each cycle)
    ///
    /// The shift registers are shifted right by one bit. The high bit (bit 15)
    /// is filled from the attribute latches during the reload cycle.
    pub fn shift(&mut self) {
        // Shift pattern registers right by 1
        self.pattern_shift_low >>= 1;
        self.pattern_shift_high >>= 1;

        // Shift attribute registers
        // Attribute bits are applied to groups of 8 pixels
        // We shift them along with the pattern data
        self.attr_shift_low = (self.attr_shift_low >> 1) | (self.attr_latch_low << 7);
        self.attr_shift_high = (self.attr_shift_high >> 1) | (self.attr_latch_high << 7);
    }

    /// Reload the shift registers with new tile data
    ///
    /// This is called every 8 cycles during the visible scanline.
    /// The fetched tile data is loaded into the upper 8 bits of the shift registers.
    pub fn reload(&mut self) {
        // Load tile data into upper 8 bits of shift registers
        self.pattern_shift_low = (self.pattern_shift_low & 0xFF00) | self.tile_data_low as u16;
        self.pattern_shift_high = (self.pattern_shift_high & 0xFF00) | self.tile_data_high as u16;

        // Load attribute data into latches
        // Attribute byte contains 4 palette selections (2 bits each)
        // We need to extract the correct 2 bits based on tile position
        self.attr_latch_low = self.attribute_byte & 0x01;
        self.attr_latch_high = (self.attribute_byte >> 1) & 0x01;
    }

    /// Get the current pixel color index (0-3)
    ///
    /// Combines the pattern bits and attribute bits to get the final palette index.
    /// Returns a value 0-3 representing the color within the palette.
    pub fn get_pixel_color_index(&self, fine_x: u8) -> u8 {
        // Extract the bit from the shift register at position fine_x
        let bit_pos = fine_x as u16;
        let low_bit = (self.pattern_shift_low >> bit_pos) & 0x01;
        let high_bit = (self.pattern_shift_high >> bit_pos) & 0x01;

        // Get attribute bits (they're shifted along with pattern)
        let attr_low = (self.attr_shift_low >> bit_pos) & 0x01;
        let attr_high = (self.attr_shift_high >> bit_pos) & 0x01;

        // Combine to get palette index: 0-3
        ((attr_high as u8) << 3) | ((attr_low as u8) << 2) | ((high_bit as u8) << 1) | (low_bit as u8)
    }
}

/// Frame buffer for PPU output
///
/// Stores the rendered frame as a Vec<u32> in ARGB format.
/// The frame is 256x240 pixels = 61,440 pixels.
#[derive(Debug, Clone)]
pub struct FrameBuffer {
    /// Pixel data in ARGB format (0xAARRGGBB)
    pub pixels: Vec<u32>,
    /// Width of the frame (256 pixels)
    pub width: usize,
    /// Height of the frame (240 pixels)
    pub height: usize,
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameBuffer {
    /// Create a new frame buffer with the NES resolution (256x240)
    pub fn new() -> Self {
        Self {
            pixels: vec![0; 256 * 240],
            width: 256,
            height: 240,
        }
    }

    /// Clear the frame buffer with a solid color
    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    /// Set a pixel at the given coordinates
    ///
    /// # Arguments
    /// * `x` - X coordinate (0-255)
    /// * `y` - Y coordinate (0-239)
    /// * `color` - ARGB color value
    pub fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = color;
        }
    }

    /// Get a pixel at the given coordinates
    ///
    /// # Arguments
    /// * `x` - X coordinate (0-255)
    /// * `y` - Y coordinate (0-239)
    ///
    /// # Returns
    /// The ARGB color value at the specified position
    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x]
        } else {
            0
        }
    }
}

/// Mirroring mode for nametables
///
/// The NES has 4 nametables but only 2KB of VRAM for them.
/// Mirroring determines how the 4 logical nametables map to the 2 physical ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    /// Horizontal mirroring: nametables 0 and 1 are the same, 2 and 3 are the same
    /// Used for vertical scrolling games
    Horizontal,
    /// Vertical mirroring: nametables 0 and 2 are the same, 1 and 3 are the same
    /// Used for horizontal scrolling games
    Vertical,
    /// Single screen mirroring: all nametables point to the lower physical
    /// nametable ($2000)
    SingleScreen,
    /// Single screen mirroring onto the upper physical nametable ($2400)
    SingleScreenUpper,
    /// Four screen mirroring: all 4 nametables are unique (requires extra VRAM on cartridge)
    FourScreen,
}

impl Default for Mirroring {
    fn default() -> Self {
        Mirroring::Horizontal
    }
}

impl Mirroring {
    /// Map a PPU address in $2000-$3EFF to its physical VRAM location.
    ///
    /// Addresses outside the nametable range are returned unchanged (masked to
    /// 14 bits). This is the single source of truth for nametable mirroring,
    /// used both when the renderer reads nametables and when the CPU writes
    /// them through PPUDATA.
    ///
    /// # Arguments
    /// * `addr` - PPU address ($2000-$3EFF for nametables)
    ///
    /// # Returns
    /// The physical VRAM address
    pub fn mirror_vram_addr(&self, addr: u16) -> u16 {
        let addr = addr & 0x3FFF;

        if !(0x2000..0x3F00).contains(&addr) {
            return addr;
        }

        // $3000-$3EFF mirrors $2000-$2EFF.
        let addr = if addr >= 0x3000 { addr - 0x1000 } else { addr };

        let nametable = ((addr - 0x2000) >> 10) as u8;
        let offset = addr & 0x3FF;

        let physical_nt = match self {
            Mirroring::Horizontal => {
                if nametable < 2 { 0 } else { 1 }
            }
            Mirroring::Vertical => {
                if nametable == 0 || nametable == 2 { 0 } else { 1 }
            }
            Mirroring::SingleScreen => 0,
            Mirroring::SingleScreenUpper => 1,
            Mirroring::FourScreen => nametable,
        };

        0x2000 + (physical_nt as u16 * 0x400) + offset
    }

    /// Get the physical nametable address for a logical nametable index
    ///
    /// # Arguments
    /// * `nametable` - Logical nametable index (0-3)
    ///
    /// # Returns
    /// The physical VRAM address offset (0x2000, 0x2400, 0x2800, or 0x2C00)
    pub fn get_nametable_address(&self, nametable: u8) -> u16 {
        let nametable = nametable & 0x03;
        match self {
            Mirroring::Horizontal => {
                // Nametables 0,1 -> $2000; Nametables 2,3 -> $2400
                if nametable < 2 {
                    0x2000
                } else {
                    0x2400
                }
            }
            Mirroring::Vertical => {
                // Nametables 0,2 -> $2000; Nametables 1,3 -> $2400
                if nametable == 0 || nametable == 2 {
                    0x2000
                } else {
                    0x2400
                }
            }
            Mirroring::SingleScreen => {
                // All nametables point to $2000
                0x2000
            }
            Mirroring::SingleScreenUpper => {
                // All nametables point to $2400
                0x2400
            }
            Mirroring::FourScreen => {
                // Each nametable is unique
                0x2000 + (nametable as u16 * 0x400)
            }
        }
    }
}

/// Background renderer
///
/// Handles all background rendering operations including:
/// - Tile fetching from pattern tables
/// - Nametable lookups
/// - Attribute table lookups for palette selection
/// - Shift register management
/// - Pixel output generation
#[derive(Debug, Clone)]
pub struct BackgroundRenderer {
    /// Background rendering state (shift registers, etc.)
    pub state: BgRenderState,
    /// Frame buffer for output
    pub frame_buffer: FrameBuffer,
    /// Current mirroring mode
    pub mirroring: Mirroring,
    /// Internal buffer for tracking which pixels have background rendered
    /// Used for sprite-0 hit detection and sprite priority
    pub bg_pixels: Vec<u8>,
    /// Sprite renderer for rendering sprites
    pub sprite_renderer: SpriteRenderer,
    /// Secondary OAM for the current scanline
    pub secondary_oam: SecondaryOam,
}

impl Default for BackgroundRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl BackgroundRenderer {
    /// Create a new background renderer
    pub fn new() -> Self {
        Self {
            state: BgRenderState::new(),
            frame_buffer: FrameBuffer::new(),
            mirroring: Mirroring::Horizontal,
            bg_pixels: vec![0; 256 * 240],
            sprite_renderer: SpriteRenderer::new(),
            secondary_oam: SecondaryOam::new(),
        }
    }

    /// Reset the background renderer
    pub fn reset(&mut self) {
        self.state.reset();
        self.frame_buffer.clear(0);
        self.bg_pixels.fill(0);
        self.sprite_renderer.reset();
        self.secondary_oam.clear();
    }

    /// Set the mirroring mode
    pub fn set_mirroring(&mut self, mirroring: Mirroring) {
        self.mirroring = mirroring;
    }

    /// Get the base address for the background pattern table
    ///
    /// # Arguments
    /// * `ctrl` - PPUCTRL register value
    ///
    /// # Returns
    /// The base address ($0000 or $1000)
    fn get_bg_pattern_table_addr(&self, ctrl: PpuCtrl) -> u16 {
        if ctrl.contains(PpuCtrl::BACKGROUND_PATTERN) {
            0x1000
        } else {
            0x0000
        }
    }

    /// Get the base nametable address
    ///
    /// # Arguments
    /// * `ctrl` - PPUCTRL register value
    ///
    /// # Returns
    /// The base nametable address ($2000, $2400, $2800, or $2C00)
    fn get_nametable_base_addr(&self, ctrl: PpuCtrl) -> u16 {
        let nt_select = ctrl.bits() & 0x03;
        0x2000 + (nt_select as u16 * 0x400)
    }

    /// Read a byte from VRAM with mirroring applied
    ///
    /// # Arguments
    /// * `ppu` - PPU instance for VRAM access
    /// * `addr` - Address to read (should be in $2000-$2FFF range for nametables)
    ///
    /// # Returns
    /// The byte at the mirrored address
    fn read_vram_mirrored(&self, ppu: &Ppu, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;

        if addr < 0x2000 {
            // Pattern table access - direct
            ppu.vram[addr as usize]
        } else if addr < 0x3F00 {
            // Nametable access - apply mirroring
            let mirrored_addr = self.mirror_address(addr);
            ppu.vram[mirrored_addr as usize]
        } else {
            // Palette RAM
            ppu.palette[(addr & 0x1F) as usize]
        }
    }

    /// Apply mirroring to a nametable address
    ///
    /// # Arguments
    /// * `addr` - Address in $2000-$2FFF range
    ///
    /// # Returns
    /// The mirrored address in VRAM
    fn mirror_address(&self, addr: u16) -> u16 {
        self.mirroring.mirror_vram_addr(addr)
    }

    /// Fetch a tile index from the nametable
    ///
    /// # Arguments
    /// * `ppu` - PPU instance
    /// * `nametable_addr` - Base nametable address
    /// * `tile_x` - Tile X coordinate (0-31)
    /// * `tile_y` - Tile Y coordinate (0-29)
    ///
    /// # Returns
    /// The tile index (0-255) at the specified position
    pub fn fetch_nametable_byte(
        &self,
        ppu: &Ppu,
        nametable_addr: u16,
        tile_x: u8,
        tile_y: u8,
    ) -> u8 {
        // Nametable entry offset: tile_y * 32 + tile_x
        let offset = (tile_y as u16 * 32) + tile_x as u16;
        let addr = nametable_addr + offset;
        self.read_vram_mirrored(ppu, addr)
    }

    /// Fetch an attribute byte from the attribute table
    ///
    /// The attribute table is at the end of each nametable ($23C0-$23FF, etc.)
    /// Each byte controls a 32x32 pixel (4x4 tile) area and contains
    /// 2-bit palette selections for four 16x16 pixel quadrants.
    ///
    /// # Arguments
    /// * `ppu` - PPU instance
    /// * `nametable_addr` - Base nametable address
    /// * `tile_x` - Tile X coordinate (0-31)
    /// * `tile_y` - Tile Y coordinate (0-29)
    ///
    /// # Returns
    /// The attribute byte for the specified tile position
    pub fn fetch_attribute_byte(
        &self,
        ppu: &Ppu,
        nametable_addr: u16,
        tile_x: u8,
        tile_y: u8,
    ) -> u8 {
        // Attribute table starts at $23C0 within each nametable
        // Attribute table is 64 bytes (8x8 groups of 4x4 tiles)
        let attr_x = tile_x >> 2; // Divide by 4
        let attr_y = tile_y >> 2; // Divide by 4
        let attr_offset = (attr_y * 8) + attr_x;
        let addr = nametable_addr + 0x3C0 + attr_offset as u16;

        self.read_vram_mirrored(ppu, addr)
    }

    /// Extract palette index from attribute byte for a specific tile
    ///
    /// # Arguments
    /// * `attr_byte` - The attribute byte value
    /// * `tile_x` - Tile X coordinate within the 4x4 group (0-3)
    /// * `tile_y` - Tile Y coordinate within the 4x4 group (0-3)
    ///
    /// # Returns
    /// The 2-bit palette index (0-3) for the specified tile
    pub fn extract_palette_index(attr_byte: u8, tile_x: u8, tile_y: u8) -> u8 {
        // Determine which quadrant within the 4x4 tile group
        // Quadrant layout: 0 1
        //                  2 3
        let quadrant = ((tile_y & 0x02) << 1) | (tile_x & 0x02);
        // Extract the 2-bit palette index for this quadrant
        (attr_byte >> quadrant) & 0x03
    }

    /// Fetch tile pattern data from the pattern table
    ///
    /// Each tile is 16 bytes: 8 bytes for the low bit plane, 8 bytes for the high bit plane.
    /// The low plane is at offset +0, the high plane is at offset +8.
    ///
    /// # Arguments
    /// * `ppu` - PPU instance
    /// * `pattern_table_addr` - Base address of pattern table ($0000 or $1000)
    /// * `tile_index` - Tile index (0-255)
    /// * `fine_y` - Fine Y scroll (0-7, which row of the tile)
    ///
    /// # Returns
    /// Tuple of (low_byte, high_byte) for the specified tile row
    pub fn fetch_tile_pattern(
        &self,
        ppu: &Ppu,
        chr: &mut dyn ChrSource,
        pattern_table_addr: u16,
        tile_index: u8,
        fine_y: u8,
    ) -> (u8, u8) {
        let tile_addr = pattern_table_addr + (tile_index as u16 * 16);
        let row_offset = (fine_y & 0x07) as u16;

        let low_byte = chr.read_pattern(ppu, tile_addr + row_offset);
        let high_byte = chr.read_pattern(ppu, tile_addr + row_offset + 8);

        (low_byte, high_byte)
    }

    /// Get the palette color for a background pixel
    ///
    /// # Arguments
    /// * `ppu` - PPU instance
    /// * `palette_index` - Which palette to use (0-3)
    /// * `color_index` - Color within palette (0-3, from pattern data)
    ///
    /// # Returns
    /// ARGB color value
    fn get_bg_palette_color(&self, ppu: &Ppu, palette_index: u8, color_index: u8) -> u32 {
        // Background palettes are at $3F00, $3F04, $3F08, $3F0C
        // Each palette has 4 entries
        let palette_addr = (palette_index & 0x03) * 4;
        let color_addr = palette_addr + (color_index & 0x03);

        // $3F00 is the universal background color
        let color_value = if color_index == 0 {
            ppu.palette[0]
        } else {
            ppu.palette[color_addr as usize]
        };

        nes_color(color_value, ppu.mask)
    }

    /// Render a single scanline of background
    ///
    /// This is the main background rendering function that processes one scanline.
    /// It fetches tiles from the nametable, gets pattern data, and renders pixels.
    ///
    /// This is the scanline-accurate entry point used while the PPU runs: the
    /// scroll position (coarse X/Y, fine X/Y and nametable) comes from
    /// `ppu.render_v`, which the rendering pipeline reloads from `t` on the
    /// pre-render scanline and advances once per visible scanline.
    ///
    /// # Arguments
    /// * `ppu` - PPU instance
    /// * `scanline` - Screen row to write into the frame buffer (0-239)
    /// * `mask` - PPUMASK register value
    pub fn render_scanline(
        &mut self,
        ppu: &Ppu,
        chr: &mut dyn ChrSource,
        scanline: u16,
        mask: PpuMask,
    ) {
        let tile_y = ((ppu.render_v >> 5) & 0x1F) as u8;
        let row_fine_y = ((ppu.render_v >> 12) & 0x07) as u8;
        let nametable = ((ppu.render_v >> 10) & 0x03) as u8;
        self.render_row(ppu, chr, scanline, nametable, tile_y, row_fine_y, mask);
    }

    /// Render the entire visible frame from a single VRAM address.
    ///
    /// Treats `ppu.render_v` as the top-left corner of the screen and walks down the
    /// 240 visible rows. Used by callers that render a whole frame in one pass
    /// (and by the tests).
    ///
    /// # Arguments
    /// * `ppu` - PPU instance
    /// * `mask` - PPUMASK register value
    pub fn render_frame(&mut self, ppu: &Ppu, chr: &mut dyn ChrSource, mask: PpuMask) {
        // Clear background pixel tracking
        self.bg_pixels.fill(0);

        let base_y = (((ppu.render_v >> 5) & 0x1F) * 8 + ((ppu.render_v >> 12) & 0x07)) as u16;
        let nametable = ((ppu.render_v >> 10) & 0x03) as u8;

        // Render all 240 visible scanlines
        for scanline in 0..240u16 {
            let effective_y = base_y + scanline;
            let tile_y = (effective_y / 8) as u8;
            let row_fine_y = (effective_y % 8) as u8;
            self.render_row(ppu, chr, scanline, nametable, tile_y, row_fine_y, mask);
        }
    }

    /// Render one row of background tiles into the frame buffer.
    ///
    /// # Arguments
    /// * `ppu` - PPU instance
    /// * `scanline` - Screen row to write (0-239)
    /// * `nametable` - Nametable index (0-3) of the leftmost tile
    /// * `tile_y` - Nametable row to fetch tiles from (0-31)
    /// * `row_fine_y` - Pixel row within the tile (0-7)
    /// * `mask` - PPUMASK register value
    fn render_row(
        &mut self,
        ppu: &Ppu,
        chr: &mut dyn ChrSource,
        scanline: u16,
        nametable: u8,
        tile_y: u8,
        row_fine_y: u8,
        mask: PpuMask,
    ) {
        let row_start = scanline as usize * 256;
        self.bg_pixels[row_start..row_start + 256].fill(0);

        // Skip if background rendering is disabled
        if !mask.contains(PpuMask::SHOW_BG) {
            // Fill with backdrop color
            let backdrop_color = self.get_bg_palette_color(ppu, 0, 0);
            for x in 0..256 {
                self.frame_buffer.set_pixel(x, scanline as usize, backdrop_color);
            }
            return;
        }

        let pattern_table_addr = self.get_bg_pattern_table_addr(ppu.ctrl);

        // Get horizontal scroll position
        let fine_x = ppu.x; // Fine X scroll (0-7)
        let coarse_x = (ppu.render_v & 0x001F) as u8; // Coarse X tile (0-31)

        // Render 33 tiles to cover the full 256 pixels plus potential scroll offset
        let mut pixel_x: i16 = -(fine_x as i16);

        for tile_offset in 0..33 {
            let tile_x = coarse_x.wrapping_add(tile_offset);

            // Handle horizontal nametable wrapping
            let (effective_tile_x, effective_nametable) = if tile_x >= 32 {
                (tile_x - 32, nametable ^ 0x01) // Toggle horizontal nametable
            } else {
                (tile_x, nametable)
            };

            // Get nametable address for this nametable
            let nt_addr = self.mirroring.get_nametable_address(effective_nametable);

            // Fetch tile index from nametable
            let tile_index = self.fetch_nametable_byte(ppu, nt_addr, effective_tile_x, tile_y);

            // Fetch attribute byte
            let attr_byte = self.fetch_attribute_byte(ppu, nt_addr, effective_tile_x, tile_y);

            // Extract palette index for this specific tile
            let palette_idx = Self::extract_palette_index(
                attr_byte,
                effective_tile_x & 0x03,
                tile_y & 0x03,
            );

            // Fetch pattern data for this tile
            let (pattern_low, pattern_high) =
                self.fetch_tile_pattern(ppu, chr, pattern_table_addr, tile_index, row_fine_y);

            // Render 8 pixels of this tile
            for px in 0..8 {
                let screen_x = pixel_x + px;

                if screen_x >= 0 && screen_x < 256 {
                    // Extract pixel color from pattern data
                    // Pattern data is stored with bit 7 being the leftmost pixel
                    let bit = 7 - px;
                    let low_bit = (pattern_low >> bit) & 0x01;
                    let high_bit = (pattern_high >> bit) & 0x01;
                    let color_idx = (high_bit << 1) | low_bit;

                    // Apply left-edge clipping if enabled
                    let clipped = screen_x < 8 && !mask.contains(PpuMask::SHOW_BG_LEFT);

                    let color = if clipped {
                        self.get_bg_palette_color(ppu, 0, 0)
                    } else {
                        self.get_bg_palette_color(ppu, palette_idx, color_idx)
                    };
                    self.frame_buffer
                        .set_pixel(screen_x as usize, scanline as usize, color);

                    // Mark this pixel as having background (for sprite priority)
                    if color_idx != 0 && !clipped {
                        self.bg_pixels[row_start + screen_x as usize] = 1;
                    }
                }
            }

            pixel_x += 8;
        }
    }

    /// Get the frame buffer as a slice of ARGB pixels
    pub fn get_frame(&self) -> &[u32] {
        &self.frame_buffer.pixels
    }

    /// Check if a background pixel exists at the given position
    ///
    /// Used for sprite-0 hit detection
    pub fn has_bg_pixel(&self, x: usize, y: usize) -> bool {
        if x < 256 && y < 240 {
            self.bg_pixels[y * 256 + x] != 0
        } else {
            false
        }
    }

    /// Render sprites for the current scanline
    ///
    /// This should be called after render_scanline to composite sprites
    /// on top of the background.
    ///
    /// # Arguments
    /// * `ppu` - PPU instance
    /// * `scanline` - Current scanline (0-239)
    /// * `mask` - PPUMASK register value
    pub fn render_sprites_for_scanline(
        &mut self,
        ppu: &Ppu,
        chr: &mut dyn ChrSource,
        scanline: u16,
        mask: PpuMask,
    ) {
        self.sprite_renderer.render_scanline(
            ppu,
            chr,
            &mut self.frame_buffer,
            &self.bg_pixels,
            &self.secondary_oam,
            scanline,
            mask,
        );
    }

    /// Check if sprite 0 hit occurred
    pub fn check_sprite_0_hit(&self) -> bool {
        self.sprite_renderer.check_sprite_0_hit()
    }

    /// Get sprite 0 hit coordinates
    pub fn get_sprite_0_hit_coords(&self) -> (i16, i16) {
        (self.sprite_renderer.sprite_0_hit_x, self.sprite_renderer.sprite_0_hit_y)
    }
}

/// PPU scanline types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanlineType {
    /// Pre-render scanline (-1 or 261): Prepares for next frame
    PreRender,
    /// Visible scanlines (0-239): Rendering actual pixels
    Visible,
    /// Post-render scanline (240): Idle, no rendering
    PostRender,
    /// VBlank scanlines (241-260): Vertical blanking period
    VBlank,
}

impl ScanlineType {
    /// Get the scanline type for a given scanline number
    ///
    /// # Arguments
    /// * `scanline` - Scanline number (0-261)
    ///
    /// # Returns
    /// The type of scanline
    pub fn from_scanline(scanline: u16) -> Self {
        match scanline {
            261 => ScanlineType::PreRender,
            0..=239 => ScanlineType::Visible,
            240 => ScanlineType::PostRender,
            241..=260 => ScanlineType::VBlank,
            _ => ScanlineType::VBlank, // Should not happen
        }
    }
}

/// Scanline processor
///
/// Handles the different phases of PPU operation across scanlines:
/// - Visible scanlines (0-239): Render pixels
/// - Post-render (240): Idle
/// - VBlank (241-260): Set VBLANK flag, trigger NMI
/// - Pre-render (261): Reset flags, prepare for next frame
pub struct ScanlineProcessor;

impl ScanlineProcessor {
    /// Process a scanline
    ///
    /// # Arguments
    /// * `ppu` - PPU instance (mutable)
    /// * `renderer` - Background renderer
    /// * `scanline` - Current scanline number
    ///
    /// # Returns
    /// True if VBlank NMI should be triggered this scanline
    pub fn process_scanline(
        ppu: &mut Ppu,
        renderer: &mut BackgroundRenderer,
        chr: &mut dyn ChrSource,
        scanline: u16,
        sprite_evaluator: &mut SpriteEvaluator,
    ) -> bool {
        let scanline_type = ScanlineType::from_scanline(scanline);

        match scanline_type {
            ScanlineType::PreRender => {
                // Pre-render scanline: prepare for next frame
                // Clear VBLANK flag, sprite overflow, and sprite 0 hit
                if ppu.cycle == 1 {
                    ppu.status.remove(PpuStatus::VBLANK);
                    ppu.status.remove(PpuStatus::SPRITE_OVERFLOW);
                    ppu.status.remove(PpuStatus::SPRITE_0_HIT);
                }

                // Reload vertical scroll bits from t to v
                // This happens at cycle 304 of pre-render scanline
                if ppu.cycle == 304 {
                    // v: IHGF.ED CBA..... = t: IHGF.ED CBA.....
                    ppu.v = (ppu.v & !0x7BE0) | (ppu.t & 0x7BE0);
                }

                false
            }
            ScanlineType::Visible => {
                // Visible scanline: render background
                // Rendering happens during cycles 1-256
                if ppu.cycle >= 1 && ppu.cycle <= 256 {
                    // Render one pixel per cycle
                    // For efficiency, we render the whole scanline at cycle 256
                    if ppu.cycle == 256 {
                        renderer.render_scanline(ppu, chr, scanline, ppu.mask);

                        // Render sprites for this scanline
                        renderer.render_sprites_for_scanline(ppu, chr, scanline, ppu.mask);
                        
                        // Check for sprite 0 hit and update status
                        if renderer.check_sprite_0_hit() {
                            ppu.status.insert(PpuStatus::SPRITE_0_HIT);
                        }
                    }
                }
                
                // Sprite evaluation happens during cycles 65-256
                // For efficiency, we do it all at cycle 256 for the NEXT scanline
                if ppu.cycle == 256 {
                    let next_scanline = scanline + 1;
                    if next_scanline < 240 {
                        let overflow = sprite_evaluator.evaluate(
                            &ppu.oam,
                            next_scanline,
                            ppu.ctrl,
                            ppu.oam_addr,
                        );
                        if overflow {
                            ppu.status.insert(PpuStatus::SPRITE_OVERFLOW);
                        }
                        // Copy evaluated sprites to renderer's secondary OAM
                        renderer.secondary_oam = sprite_evaluator.secondary_oam.clone();
                    }
                }

                // Increment Y scroll at cycle 256
                if ppu.cycle == 256 {
                    Self::increment_y(ppu);
                }

                // Reload X scroll at cycle 257
                if ppu.cycle == 257 {
                    // v: ....F.. ...EDCBA = t: ....F.. ...EDCBA
                    ppu.v = (ppu.v & !0x041F) | (ppu.t & 0x041F);
                }

                false
            }
            ScanlineType::PostRender => {
                // Post-render scanline: idle, no rendering
                false
            }
            ScanlineType::VBlank => {
                // VBlank scanlines
                // Set VBLANK flag at cycle 1 of scanline 241
                if scanline == 241 && ppu.cycle == 1 {
                    ppu.status.insert(PpuStatus::VBLANK);

                    // Check if NMI should be triggered
                    return ppu.ctrl.contains(PpuCtrl::NMI_ENABLE);
                }
                false
            }
        }
    }

    /// Increment the Y scroll (coarse Y and fine Y)
    ///
    /// This is called at the end of each visible scanline.
    fn increment_y(ppu: &mut Ppu) {
        ppu.v = Self::increment_scroll_y(ppu.v);
    }

    /// Advance the vertical scroll position by one scanline.
    ///
    /// Increments fine Y, and on overflow resets it and increments coarse Y.
    /// Wrapping past coarse Y 29 toggles the vertical nametable.
    ///
    /// # Arguments
    /// * `v` - Current scroll VRAM address
    ///
    /// # Returns
    /// The scroll address for the next scanline
    pub fn increment_scroll_y(v: u16) -> u16 {
        let mut v = v;

        if (v & 0x7000) != 0x7000 {
            // Fine Y < 7, just increment
            v += 0x1000;
        } else {
            // Fine Y overflow, reset to 0 and increment coarse Y
            v &= !0x7000; // Clear fine Y

            let mut coarse_y = (v >> 5) & 0x1F;

            if coarse_y == 29 {
                // Coarse Y = 29, wrap to 0 and switch vertical nametable
                coarse_y = 0;
                v ^= 0x0800; // Toggle vertical nametable
            } else if coarse_y == 31 {
                // Coarse Y = 31, wrap to 0 without switching nametable
                coarse_y = 0;
            } else {
                // Normal increment
                coarse_y += 1;
            }

            // Write coarse Y back to v
            v = (v & !0x03E0) | (coarse_y << 5);
        }

        v
    }

    /// Get the number of cycles per scanline
    pub const CYCLES_PER_SCANLINE: u16 = 341;

    /// Get the total number of scanlines per frame
    pub const SCANLINES_PER_FRAME: u16 = 262;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nes_color_without_effects_matches_palette() {
        assert_eq!(nes_color(0x21, PpuMask::empty()), NES_PALETTE[0x21]);
        assert_eq!(nes_color(0x21, PpuMask::SHOW_BG), NES_PALETTE[0x21]);
    }

    /// The renderer must fetch pattern data through the `ChrSource` so that
    /// mid-frame CHR bank switches take effect.
    #[test]
    fn test_render_scanline_reads_patterns_through_chr_source() {
        struct CountingChr {
            calls: Vec<u16>,
        }
        impl ChrSource for CountingChr {
            fn read_pattern(&mut self, _ppu: &Ppu, addr: u16) -> u8 {
                self.calls.push(addr);
                0
            }
        }

        let mut ppu = Ppu::new();
        let mut renderer = BackgroundRenderer::new();
        let mut chr = CountingChr { calls: Vec::new() };

        // A nametable full of tile 0, background rendering enabled.
        ppu.ctrl = PpuCtrl::empty();
        let mask = PpuMask::SHOW_BG | PpuMask::SHOW_BG_LEFT;
        renderer.render_scanline(&ppu, &mut chr, 0, mask);

        assert!(!chr.calls.is_empty(), "no pattern fetches went through ChrSource");
        assert!(
            chr.calls.iter().all(|addr| *addr < 0x2000),
            "pattern fetches must stay inside the pattern tables"
        );
    }

    #[test]
    fn test_nes_color_grayscale_masks_to_grey_column() {
        // Grayscale forces the low four bits of the index to zero.
        assert_eq!(
            nes_color(0x2A, PpuMask::GRAYSCALE),
            NES_PALETTE[0x20]
        );
        assert_eq!(
            nes_color(0x15, PpuMask::GRAYSCALE),
            NES_PALETTE[0x10]
        );
    }

    #[test]
    fn test_nes_color_emphasis_darkens_the_other_channels() {
        let base = NES_PALETTE[0x30]; // white
        let emphasized = nes_color(0x30, PpuMask::EMPHASIZE_RED);

        let base_g = (base >> 8) & 0xFF;
        let base_b = base & 0xFF;
        let em_g = (emphasized >> 8) & 0xFF;
        let em_b = emphasized & 0xFF;

        // Red stays at full intensity, green and blue drop to 75%.
        assert_eq!((emphasized >> 16) & 0xFF, (base >> 16) & 0xFF);
        assert_eq!(em_g, (base_g as f32 * 0.75) as u32);
        assert_eq!(em_b, (base_b as f32 * 0.75) as u32);
    }

    #[test]
    fn test_nes_color_all_emphasis_bits_compound() {
        // All three bits set: every channel is attenuated twice.
        let white = NES_PALETTE[0x30];
        let mask = PpuMask::EMPHASIZE_RED | PpuMask::EMPHASIZE_GREEN | PpuMask::EMPHASIZE_BLUE;
        let color = nes_color(0x30, mask);

        let expected = ((white >> 16) & 0xFF) as f32 * 0.75 * 0.75;
        assert_eq!((color >> 16) & 0xFF, expected as u32);
    }
    #[test]
    fn test_bg_render_state_new() {
        let state = BgRenderState::new();
        assert_eq!(state.pattern_shift_low, 0);
        assert_eq!(state.pattern_shift_high, 0);
        assert_eq!(state.nametable_byte, 0);
    }

    #[test]
    fn test_bg_render_state_shift() {
        let mut state = BgRenderState::new();
        state.pattern_shift_low = 0xFF00;
        state.pattern_shift_high = 0xAA00;
        state.attr_latch_low = 1;
        state.attr_latch_high = 0;

        state.shift();

        assert_eq!(state.pattern_shift_low, 0x7F80);
        assert_eq!(state.pattern_shift_high, 0x5500);
    }

    #[test]
    fn test_bg_render_state_reload() {
        let mut state = BgRenderState::new();
        state.tile_data_low = 0x55;
        state.tile_data_high = 0xAA;
        state.attribute_byte = 0x03;

        state.reload();

        assert_eq!(state.pattern_shift_low, 0x0055);
        assert_eq!(state.pattern_shift_high, 0x00AA);
        assert_eq!(state.attr_latch_low, 1);
        assert_eq!(state.attr_latch_high, 1);
    }

    #[test]
    fn test_frame_buffer_new() {
        let fb = FrameBuffer::new();
        assert_eq!(fb.width, 256);
        assert_eq!(fb.height, 240);
        assert_eq!(fb.pixels.len(), 256 * 240);
    }

    #[test]
    fn test_frame_buffer_set_get_pixel() {
        let mut fb = FrameBuffer::new();
        fb.set_pixel(10, 20, 0xFF123456);
        assert_eq!(fb.get_pixel(10, 20), 0xFF123456);
    }

    #[test]
    fn test_frame_buffer_clear() {
        let mut fb = FrameBuffer::new();
        fb.set_pixel(10, 20, 0xFF123456);
        fb.clear(0);
        assert_eq!(fb.get_pixel(10, 20), 0);
    }

    #[test]
    fn test_mirroring_horizontal() {
        let mirroring = Mirroring::Horizontal;
        // Nametables 0,1 -> $2000; Nametables 2,3 -> $2400
        assert_eq!(mirroring.get_nametable_address(0), 0x2000);
        assert_eq!(mirroring.get_nametable_address(1), 0x2000);
        assert_eq!(mirroring.get_nametable_address(2), 0x2400);
        assert_eq!(mirroring.get_nametable_address(3), 0x2400);
    }

    #[test]
    fn test_mirroring_vertical() {
        let mirroring = Mirroring::Vertical;
        // Nametables 0,2 -> $2000; Nametables 1,3 -> $2400
        assert_eq!(mirroring.get_nametable_address(0), 0x2000);
        assert_eq!(mirroring.get_nametable_address(1), 0x2400);
        assert_eq!(mirroring.get_nametable_address(2), 0x2000);
        assert_eq!(mirroring.get_nametable_address(3), 0x2400);
    }

    #[test]
    fn test_mirroring_single_screen() {
        let mirroring = Mirroring::SingleScreen;
        assert_eq!(mirroring.get_nametable_address(0), 0x2000);
        assert_eq!(mirroring.get_nametable_address(1), 0x2000);
        assert_eq!(mirroring.get_nametable_address(2), 0x2000);
        assert_eq!(mirroring.get_nametable_address(3), 0x2000);
    }

    #[test]
    fn test_mirroring_four_screen() {
        let mirroring = Mirroring::FourScreen;
        assert_eq!(mirroring.get_nametable_address(0), 0x2000);
        assert_eq!(mirroring.get_nametable_address(1), 0x2400);
        assert_eq!(mirroring.get_nametable_address(2), 0x2800);
        assert_eq!(mirroring.get_nametable_address(3), 0x2C00);
    }

    #[test]
    fn test_extract_palette_index() {
        // Attribute byte layout: [bottom-right:2][bottom-left:2][top-right:2][top-left:2]
        // Each 2-bit value selects one of 4 background palettes
        let attr = 0b00_01_10_11; // TL=3, TR=2, BL=1, BR=0

        // Top-left quadrant (tiles 0-1 in both X and Y)
        assert_eq!(BackgroundRenderer::extract_palette_index(attr, 0, 0), 3);
        assert_eq!(BackgroundRenderer::extract_palette_index(attr, 1, 1), 3);

        // Top-right quadrant (tiles 2-3 in X, 0-1 in Y)
        assert_eq!(BackgroundRenderer::extract_palette_index(attr, 2, 0), 2);
        assert_eq!(BackgroundRenderer::extract_palette_index(attr, 3, 1), 2);

        // Bottom-left quadrant (tiles 0-1 in X, 2-3 in Y)
        assert_eq!(BackgroundRenderer::extract_palette_index(attr, 0, 2), 1);
        assert_eq!(BackgroundRenderer::extract_palette_index(attr, 1, 3), 1);

        // Bottom-right quadrant (tiles 2-3 in both X and Y)
        assert_eq!(BackgroundRenderer::extract_palette_index(attr, 2, 2), 0);
        assert_eq!(BackgroundRenderer::extract_palette_index(attr, 3, 3), 0);
    }

    #[test]
    fn test_scanline_type_from_scanline() {
        assert_eq!(ScanlineType::from_scanline(261), ScanlineType::PreRender);
        assert_eq!(ScanlineType::from_scanline(0), ScanlineType::Visible);
        assert_eq!(ScanlineType::from_scanline(239), ScanlineType::Visible);
        assert_eq!(ScanlineType::from_scanline(240), ScanlineType::PostRender);
        assert_eq!(ScanlineType::from_scanline(241), ScanlineType::VBlank);
        assert_eq!(ScanlineType::from_scanline(260), ScanlineType::VBlank);
    }

    #[test]
    fn test_background_renderer_new() {
        let renderer = BackgroundRenderer::new();
        assert_eq!(renderer.frame_buffer.width, 256);
        assert_eq!(renderer.frame_buffer.height, 240);
        assert_eq!(renderer.mirroring, Mirroring::Horizontal);
    }

    #[test]
    fn test_fetch_nametable_byte() {
        let mut ppu = Ppu::new();
        let renderer = BackgroundRenderer::new();

        // Set up a tile in the nametable
        ppu.vram[0x2000] = 0x42;

        let tile_index = renderer.fetch_nametable_byte(&ppu, 0x2000, 0, 0);
        assert_eq!(tile_index, 0x42);
    }

    #[test]
    fn test_fetch_attribute_byte() {
        let mut ppu = Ppu::new();
        let renderer = BackgroundRenderer::new();

        // Set up an attribute byte at $23C0 (start of attribute table)
        ppu.vram[0x23C0] = 0xAA;

        let attr = renderer.fetch_attribute_byte(&ppu, 0x2000, 0, 0);
        assert_eq!(attr, 0xAA);
    }

    #[test]
    fn test_fetch_tile_pattern() {
        let mut ppu = Ppu::new();
        let renderer = BackgroundRenderer::new();

        // Set up a tile at pattern table 0, tile 0
        // Tile pattern: each tile is 16 bytes
        // Bytes 0-7: low bit plane
        // Bytes 8-15: high bit plane
        ppu.vram[0x0000] = 0xFF; // Low plane, row 0: all pixels have bit 0 set
        ppu.vram[0x0008] = 0x00; // High plane, row 0: no pixels have bit 1 set

        let (low, high) = renderer.fetch_tile_pattern(&ppu, &mut VramChr, 0x0000, 0, 0);
        assert_eq!(low, 0xFF);
        assert_eq!(high, 0x00);
    }

    #[test]
    fn test_scanline_processor_increment_y() {
        let mut ppu = Ppu::new();

        // Test fine Y increment
        ppu.v = 0x1000; // Fine Y = 1
        ScanlineProcessor::increment_y(&mut ppu);
        assert_eq!(ppu.v & 0x7000, 0x2000); // Fine Y = 2

        // Test fine Y overflow
        ppu.v = 0x7000; // Fine Y = 7
        ScanlineProcessor::increment_y(&mut ppu);
        assert_eq!(ppu.v & 0x7000, 0x0000); // Fine Y = 0
        assert_eq!((ppu.v >> 5) & 0x1F, 1); // Coarse Y = 1
    }

    #[test]
    fn test_nes_palette_values() {
        // Verify palette has 64 entries
        assert_eq!(NES_PALETTE.len(), 64);

        // Verify some known colors
        // Color 0x00 is dark gray (0xFF666666)
        assert_eq!(NES_PALETTE[0], 0xFF666666);

        // Color 0x0F is black
        assert_eq!(NES_PALETTE[0x0F], 0xFF000000);

        // Color 0x20 is white
        assert_eq!(NES_PALETTE[0x20], 0xFFFFFEFF);
    }
}
