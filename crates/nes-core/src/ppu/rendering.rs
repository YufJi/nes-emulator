//! PPU Background Rendering Pipeline
//!
//! This module implements the NES PPU background rendering pipeline.
//! The PPU generates a 256x240 pixel display by processing tiles from
//! nametables and pattern tables.
//!
//! Reference: https://www.nesdev.org/wiki/PPU_rendering

use crate::mapper::Mapper;
use crate::ppu::{Ppu, PpuMask};
use crate::prelude::*;
use crate::Region;

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

    /// Byte the cartridge supplies for a nametable address, if it overrides
    /// the PPU's own memory.
    ///
    /// MMC5 maps nametable windows onto its ExRAM or onto a fill tile, so the
    /// fetch has to ask the cartridge before falling back to VRAM.
    fn read_nametable(&mut self, _ppu: &Ppu, _addr: u16) -> Option<u8> {
        None
    }
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

    fn read_nametable(&mut self, _ppu: &Ppu, addr: u16) -> Option<u8> {
        self.0.nametable_read(addr)
    }
}

/// NES color palette (64 colors, 6-bit values converted to ARGB).
///
/// Mesen's 2C02 table, entry for entry (`NesDefaultVideoFilter.cpp`,
/// `_ppuPaletteArgb[0]`), which is the palette its default video filter starts
/// from.
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

/// What the two non-emphasised channels keep of their value, in percent, when
/// an emphasis bit is set.
///
/// Mesen's `GenerateFullColorPalette` multiplies by 0.84 and truncates; the
/// integer form below is that same truncation — `v * 84 / 100` equals
/// `(uint8_t)(v * 0.84)` for every 8-bit `v`.
const EMPHASIS_ATTENUATION_PERCENT: u32 = 84;

/// The 512-entry palette the PPU's nine colour bits index into.
///
/// Mesen builds it in `NesDefaultVideoFilter::GenerateFullColorPalette`: the
/// low six bits are the palette RAM value, and the top three are the emphasis
/// bits (red in bit 6, green in bit 7, blue in bit 8 — Mesen's
/// `_intensifyColorBits`). Setting one damps the *other* two channels to 84%,
/// and — the part worth copying exactly — the three black columns `$xE` and
/// `$xF` are left alone: emphasis does not tint them.
const fn emphasis_palette() -> [u32; 512] {
    let mut table = [0u32; 512];
    let mut index = 0;
    while index < 64 {
        let base = NES_PALETTE[index];
        let r = (base >> 16) & 0xFF;
        let g = (base >> 8) & 0xFF;
        let b = base & 0xFF;
        // Emphasis leaves the black column alone, and entry 0 is the plain
        // colour.
        let affected = (index & 0x0F) <= 0x0D;
        let mut emphasis = 0;
        while emphasis < 8 {
            let (r, g, b) = if affected {
                let red_on = emphasis & 0b001 != 0;
                let green_on = emphasis & 0b010 != 0;
                let blue_on = emphasis & 0b100 != 0;
                (
                    if green_on || blue_on {
                        r * EMPHASIS_ATTENUATION_PERCENT / 100
                    } else {
                        r
                    },
                    if red_on || blue_on {
                        g * EMPHASIS_ATTENUATION_PERCENT / 100
                    } else {
                        g
                    },
                    if red_on || green_on {
                        b * EMPHASIS_ATTENUATION_PERCENT / 100
                    } else {
                        b
                    },
                )
            } else {
                (r, g, b)
            };
            table[(emphasis << 6) | index] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
            emphasis += 1;
        }
        index += 1;
    }
    table
}

/// The nine colour bits — six of palette RAM plus three of emphasis — resolved
/// to ARGB, exactly as Mesen's default video filter resolves them.
pub const NES_EMPHASIS_PALETTE: [u32; 512] = emphasis_palette();

/// Convert a palette RAM value into an ARGB pixel.
///
/// This is the nine bits Mesen hands to its video filter: the low six are the
/// palette RAM value (clamped to the `$x0` column by the PPUMASK grayscale
/// bit, Mesen's `_paletteRamMask`), and the top three are the emphasis bits —
/// red, green and blue, in that order — which index
/// [`NES_EMPHASIS_PALETTE`].
///
/// # Arguments
/// * `color_index` - Palette RAM value (only the low 6 bits are used)
/// * `mask` - PPUMASK register
pub fn nes_color(color_index: u8, mask: PpuMask, region: Region) -> u32 {
    let mut index = color_index & 0x3F;
    if mask.contains(PpuMask::GRAYSCALE) {
        index &= 0x30;
    }

    // "Note that on the Dendy and PAL NES, the green and red bits swap
    // meaning" (Mesen's `SetMaskRegister`), so the register's bit 5 is the
    // green emphasis on those and the red one on NTSC.
    let (red_bit, green_bit) = match region {
        Region::Ntsc => (PpuMask::EMPHASIZE_RED, PpuMask::EMPHASIZE_GREEN),
        Region::Pal => (PpuMask::EMPHASIZE_GREEN, PpuMask::EMPHASIZE_RED),
    };

    // Mesen's `_intensifyColorBits`: emphasis is the top three bits of the
    // value the video filter looks up, red lowest of the three.
    let emphasis = usize::from(mask.contains(red_bit))
        | (usize::from(mask.contains(green_bit)) << 1)
        | (usize::from(mask.contains(PpuMask::EMPHASIZE_BLUE)) << 2);

    NES_EMPHASIS_PALETTE[(emphasis << 6) | index as usize]
}

/// Background rendering state
///
/// The PPU uses shift registers to hold tile data for the current and next tile.
/// This allows for smooth horizontal scrolling by shifting out one pixel at a time.
pub const WIDTH: usize = 256;

/// Height of the rendered frame in pixels.
pub const HEIGHT: usize = 240;

/// Layout of the 32-bit pixels the PPU writes into a [`FrameBuffer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// One `u32` per pixel in `0xAARRGGBB` order, alpha in the high byte.
    ///
    /// On a little-endian host those bytes land in memory as `B, G, R, A`,
    /// which is exactly what `MTLPixelFormatBGRA8Unorm` (Metal) and
    /// `kCGBitmapByteOrder32Little | kCGImageAlphaNoneSkipFirst` (CoreGraphics)
    /// expect: a native front end can upload the buffer as-is. The web needs a
    /// swizzle only because `ImageData` is RGBA.
    Argb8888,
}

/// Pixel layout of [`FrameBuffer::pixels`].
pub const PIXEL_FORMAT: PixelFormat = PixelFormat::Argb8888;

impl PixelFormat {
    /// Stable identifier for host bindings and diagnostics, e.g. `"argb8888"`.
    pub fn id(self) -> &'static str {
        match self {
            PixelFormat::Argb8888 => "argb8888",
        }
    }

    /// Size of one pixel in bytes.
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Argb8888 => 4,
        }
    }

    /// Bit position of each channel inside one pixel, as `[red, green, blue, alpha]`.
    pub fn channel_shifts(self) -> [u32; 4] {
        match self {
            PixelFormat::Argb8888 => [16, 8, 0, 24],
        }
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

crate::state::state_fields!(FrameBuffer {
    pixels,
    width,
    height
});

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameBuffer {
    /// Create a new frame buffer with the NES resolution (256x240)
    pub fn new() -> Self {
        Self {
            pixels: vec![0; WIDTH * HEIGHT],
            width: WIDTH,
            height: HEIGHT,
        }
    }

    /// Clear the frame buffer with a solid color
    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    /// Exchange this buffer's pixels with another's.
    ///
    /// The renderer draws into a working buffer and the machine shows it by
    /// swapping the two at the frame boundary: the host keeps reading a
    /// complete frame while the next one is being drawn, and nothing copies
    /// 245 KB per frame to make that true.
    pub fn swap_pixels(&mut self, other: &mut FrameBuffer) {
        debug_assert_eq!(self.pixels.len(), other.pixels.len());
        core::mem::swap(&mut self.pixels, &mut other.pixels);
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mirroring {
    /// Horizontal mirroring: nametables 0 and 1 are the same, 2 and 3 are the same
    /// Used for vertical scrolling games
    #[default]
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

impl crate::state::StateField for Mirroring {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u8(match self {
            Mirroring::Horizontal => 0,
            Mirroring::Vertical => 1,
            Mirroring::SingleScreen => 2,
            Mirroring::SingleScreenUpper => 3,
            Mirroring::FourScreen => 4,
        });
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        *self = match r.u8() {
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreen,
            3 => Mirroring::SingleScreenUpper,
            4 => Mirroring::FourScreen,
            _ => Mirroring::Horizontal,
        };
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
                if nametable < 2 {
                    0
                } else {
                    1
                }
            }
            Mirroring::Vertical => {
                if nametable == 0 || nametable == 2 {
                    0
                } else {
                    1
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nes_color_without_effects_matches_palette() {
        assert_eq!(
            nes_color(0x21, PpuMask::empty(), Region::Ntsc),
            NES_PALETTE[0x21]
        );
        assert_eq!(
            nes_color(0x21, PpuMask::SHOW_BG, Region::Ntsc),
            NES_PALETTE[0x21]
        );
    }

    /// PAL and Dendy swap what the emphasis bits mean: the register's bit 5 is
    /// the red emphasis on NTSC and the green one on those (Mesen's
    /// `SetMaskRegister`).
    #[test]
    fn test_pal_swaps_the_emphasis_bits() {
        let bit_five = PpuMask::EMPHASIZE_RED;

        let ntsc = nes_color(0x21, bit_five, Region::Ntsc);
        let plain = nes_color(0x21, PpuMask::empty(), Region::Ntsc);
        assert!(
            (ntsc >> 8) & 0xFF < (plain >> 8) & 0xFF,
            "on NTSC the bit damps green"
        );
        assert_eq!(ntsc >> 16 & 0xFF, plain >> 16 & 0xFF, "red is untouched");

        let pal = nes_color(0x21, bit_five, Region::Pal);
        assert!(
            (pal >> 16) & 0xFF < (plain >> 16) & 0xFF,
            "on PAL the same bit damps red instead"
        );
        assert_eq!(pal >> 8 & 0xFF, plain >> 8 & 0xFF, "green is untouched");
    }

    #[test]
    fn test_nes_color_grayscale_masks_to_grey_column() {
        assert_eq!(
            nes_color(0x2F, PpuMask::GRAYSCALE, Region::Ntsc),
            NES_PALETTE[0x20]
        );
        assert_eq!(
            nes_color(0x0F, PpuMask::GRAYSCALE, Region::Ntsc),
            NES_PALETTE[0x00]
        );
    }

    #[test]
    fn test_nes_color_emphasis_darkens_the_other_channels() {
        let plain = nes_color(0x21, PpuMask::empty(), Region::Ntsc);
        let emphasised = nes_color(0x21, PpuMask::EMPHASIZE_RED, Region::Ntsc);
        let plain_green = (plain >> 8) & 0xFF;
        let emphasised_green = (emphasised >> 8) & 0xFF;
        assert!(emphasised_green < plain_green);
        assert_eq!(emphasised >> 16 & 0xFF, plain >> 16 & 0xFF);
    }

    /// The emphasis bits are the top three bits of the value the palette is
    /// looked up with, and setting one damps the other two channels to 84% of
    /// their value — truncating, exactly as Mesen's
    /// `GenerateFullColorPalette` does. `$21` is `0xFF64B0FF`, so red emphasis
    /// leaves red at `0x64` and takes green `0xB0` → `0x93` and blue `0xFF` →
    /// `0xD6`.
    #[test]
    fn test_nes_color_emphasis_matches_mesens_generation() {
        let plain = NES_PALETTE[0x21];
        assert_eq!(plain, 0xFF64_B0FF);

        let red = nes_color(0x21, PpuMask::EMPHASIZE_RED, Region::Ntsc);
        assert_eq!(red, 0xFF64_93D6, "green and blue keep 84%");
        assert_eq!(
            red,
            NES_EMPHASIS_PALETTE[0x40 | 0x21],
            "red emphasis is bit 6 of the lookup"
        );

        let green = nes_color(0x21, PpuMask::EMPHASIZE_GREEN, Region::Ntsc);
        assert_eq!(green, 0xFF54_B0D6, "red and blue keep 84%");
        assert_eq!(green, NES_EMPHASIS_PALETTE[0x80 | 0x21]);

        let blue = nes_color(0x21, PpuMask::EMPHASIZE_BLUE, Region::Ntsc);
        assert_eq!(blue, 0xFF54_93FF, "red and green keep 84%");
        assert_eq!(blue, NES_EMPHASIS_PALETTE[0x100 | 0x21]);
    }

    /// Mesen's generator skips the `$xE`/`$xF` columns: emphasis does not tint
    /// them. On the 2C02 table those entries are black, so the rule changes
    /// nothing here — but the table is built the way Mesen builds it.
    #[test]
    fn test_emphasis_leaves_the_black_columns_alone() {
        for black in [0x0E, 0x0F, 0x1E, 0x1F, 0x2F, 0x3F] {
            assert_eq!(NES_PALETTE[black], 0xFF00_0000);
            for bit in [
                PpuMask::EMPHASIZE_RED,
                PpuMask::EMPHASIZE_GREEN,
                PpuMask::EMPHASIZE_BLUE,
            ] {
                assert_eq!(
                    nes_color(black as u8, bit, Region::Ntsc),
                    NES_PALETTE[black],
                    "{black:#04X} keeps its colour"
                );
            }
        }
    }

    /// Every entry of the 512-value table is reachable from a palette RAM
    /// value and a mask, and the plain row is the base palette.
    #[test]
    fn test_the_plain_row_of_the_table_is_the_base_palette() {
        for index in 0..64u8 {
            assert_eq!(
                NES_EMPHASIS_PALETTE[index as usize],
                NES_PALETTE[index as usize]
            );
            assert_eq!(
                nes_color(index, PpuMask::empty(), Region::Ntsc),
                NES_PALETTE[index as usize]
            );
        }
    }

    #[test]
    fn test_frame_buffer_set_get_pixel() {
        let mut buffer = FrameBuffer::new();
        buffer.set_pixel(10, 20, 0xFF00_FF00);
        assert_eq!(buffer.get_pixel(10, 20), 0xFF00_FF00);
        assert_eq!(buffer.get_pixel(0, 0), 0);
    }

    #[test]
    fn test_frame_buffer_swap_exchanges_the_pictures() {
        let mut a = FrameBuffer::new();
        let mut b = FrameBuffer::new();
        a.set_pixel(0, 0, 0xFF11_2233);
        b.set_pixel(1, 1, 0xFF44_5566);
        a.swap_pixels(&mut b);
        assert_eq!(a.get_pixel(1, 1), 0xFF44_5566);
        assert_eq!(b.get_pixel(0, 0), 0xFF11_2233);
    }

    #[test]
    fn test_mirroring_maps_the_nametables() {
        // Horizontal: $2000/$2400 are one physical nametable, $2800/$2C00 the
        // other (the pair stacks vertically).
        assert_eq!(Mirroring::Horizontal.mirror_vram_addr(0x2400), 0x2000);
        assert_eq!(Mirroring::Horizontal.mirror_vram_addr(0x2800), 0x2400);
        // Vertical is the same idea sideways.
        assert_eq!(Mirroring::Vertical.mirror_vram_addr(0x2800), 0x2000);
        assert_eq!(Mirroring::Vertical.mirror_vram_addr(0x2400), 0x2400);
        // Single-screen puts everything in one table, and $3000 mirrors $2000.
        assert_eq!(Mirroring::SingleScreen.mirror_vram_addr(0x2C00), 0x2000);
        assert_eq!(Mirroring::Horizontal.mirror_vram_addr(0x3000), 0x2000);
        // Pattern tables and palette RAM are not nametables.
        assert_eq!(Mirroring::Horizontal.mirror_vram_addr(0x1234), 0x1234);
    }
}
