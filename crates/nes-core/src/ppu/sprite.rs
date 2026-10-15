//! PPU Sprite Evaluation and Rendering
//!
//! This module implements the NES PPU sprite evaluation and rendering pipeline.
//! The PPU can display up to 64 sprites (256 bytes of OAM), with a maximum of
//! 8 sprites per scanline.
//!
//! Reference: https://www.nesdev.org/wiki/PPU_sprite_evaluation

use crate::ppu::PpuCtrl;

/// Sprite attributes byte layout
///
/// Bit 7: Vertical flip
/// Bit 6: Horizontal flip
/// Bit 5: Priority (0: in front, 1: behind background)
/// Bit 4-2: Palette (0-3, selects $3F10-$3F1F)
/// Bit 1-0: Unimplemented (read as 0)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteAttributes(u8);

impl SpriteAttributes {
    /// Create from raw byte
    pub fn new(byte: u8) -> Self {
        Self(byte)
    }

    /// Get raw byte value
    pub fn raw(&self) -> u8 {
        self.0
    }

    /// Vertical flip flag
    pub fn vertical_flip(&self) -> bool {
        (self.0 >> 7) & 1 != 0
    }

    /// Horizontal flip flag
    pub fn horizontal_flip(&self) -> bool {
        (self.0 >> 6) & 1 != 0
    }

    /// Priority: false = in front of background, true = behind background
    pub fn behind_background(&self) -> bool {
        (self.0 >> 5) & 1 != 0
    }

    /// Palette index (0-3)
    pub fn palette(&self) -> u8 {
        (self.0 >> 2) & 0x03
    }
}

/// A single sprite entry (4 bytes in OAM)
///
/// Layout:
/// - Byte 0: Y position (display line - 1)
/// - Byte 1: Tile index
/// - Byte 2: Attributes
/// - Byte 3: X position
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sprite {
    /// Y position (display line - 1)
    pub y: u8,
    /// Tile index
    pub tile: u8,
    /// Attributes byte
    pub attributes: u8,
    /// X position
    pub x: u8,
}

impl Sprite {
    /// Create a sprite from 4 bytes of OAM data
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            y: bytes[0],
            tile: bytes[1],
            attributes: bytes[2],
            x: bytes[3],
        }
    }

    /// Get attributes as SpriteAttributes struct
    pub fn attrs(&self) -> SpriteAttributes {
        SpriteAttributes::new(self.attributes)
    }

    /// Check if this sprite is on the given scanline
    ///
    /// # Arguments
    /// * `scanline` - The scanline to check (0-239)
    /// * `sprite_height` - 8 or 16 (depending on sprite size mode)
    pub fn is_on_scanline(&self, scanline: u16, sprite_height: u8) -> bool {
        // Sprite Y is display line - 1, so visible Y range is (y+1) to (y+sprite_height)
        let sprite_y = self.y as u16 + 1;
        scanline >= sprite_y && scanline < sprite_y + sprite_height as u16
    }

    /// Get the fine Y position within the sprite for a given scanline
    ///
    /// # Arguments
    /// * `scanline` - The current scanline
    /// * `sprite_height` - 8 or 16
    /// * `vertical_flip` - Whether to flip vertically
    ///
    /// # Returns
    /// The row within the sprite (0-7 for 8x8, 0-15 for 8x16)
    pub fn get_fine_y(&self, scanline: u16, sprite_height: u8, vertical_flip: bool) -> u8 {
        let sprite_y = self.y as u16 + 1;
        // Callers are expected to check `is_on_scanline` first; clamp anyway so
        // an out-of-range sprite can never underflow.
        let row = (scanline.saturating_sub(sprite_y)) as u8;

        if vertical_flip {
            (sprite_height - 1).saturating_sub(row)
        } else {
            row
        }
    }
}

/// Secondary OAM - holds up to 8 sprites for the current scanline
///
/// On real hardware, this is a separate 32-byte memory that gets loaded
/// during sprite evaluation (cycles 65-256 of visible scanlines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondaryOam {
    /// 32 bytes: 8 sprites × 4 bytes each
    data: [u8; 32],
    /// Number of sprites currently stored (0-8)
    count: usize,
    /// Whether sprite 0 is in this secondary OAM
    has_sprite_0: bool,
}

impl crate::state::StateField for SecondaryOam {
    fn write(&self, w: &mut crate::state::StateWriter) {
        self.data.write(w);
        self.count.write(w);
        self.has_sprite_0.write(w);
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        self.data.read(r);
        self.count.read(r);
        self.has_sprite_0.read(r);
    }
}

impl Default for SecondaryOam {
    fn default() -> Self {
        Self::new()
    }
}

impl SecondaryOam {
    /// Create a new empty secondary OAM
    pub fn new() -> Self {
        Self {
            data: [0xFF; 32], // Hardware initializes to $FF
            count: 0,
            has_sprite_0: false,
        }
    }

    /// Clear the secondary OAM (sets all bytes to $FF)
    pub fn clear(&mut self) {
        self.data.fill(0xFF);
        self.count = 0;
        self.has_sprite_0 = false;
    }

    /// Get the number of sprites
    pub fn count(&self) -> usize {
        self.count
    }

    /// Check if sprite 0 is present
    pub fn has_sprite_0(&self) -> bool {
        self.has_sprite_0
    }

    /// Get a sprite at the given index
    ///
    /// # Arguments
    /// * `index` - Sprite index (0-7)
    ///
    /// # Returns
    /// Some(Sprite) if index is valid, None otherwise
    pub fn get_sprite(&self, index: usize) -> Option<Sprite> {
        if index >= self.count {
            return None;
        }
        let offset = index * 4;
        Some(Sprite::from_bytes(&self.data[offset..offset + 4]))
    }

    /// Add a sprite to secondary OAM
    ///
    /// # Arguments
    /// * `sprite` - The sprite to add
    /// * `is_sprite_0` - Whether this is OAM sprite 0
    ///
    /// # Returns
    /// true if added successfully, false if secondary OAM is full
    pub fn add_sprite(&mut self, sprite: &Sprite, is_sprite_0: bool) -> bool {
        if self.count >= 8 {
            return false;
        }

        let offset = self.count * 4;
        self.data[offset] = sprite.y;
        self.data[offset + 1] = sprite.tile;
        self.data[offset + 2] = sprite.attributes;
        self.data[offset + 3] = sprite.x;

        if is_sprite_0 {
            self.has_sprite_0 = true;
        }

        self.count += 1;
        true
    }

    /// Get raw data at offset
    pub fn get_byte(&self, offset: usize) -> u8 {
        self.data[offset]
    }
}

/// Sprite evaluation state
///
/// Implements the sprite evaluation algorithm that runs during cycles 65-256
/// of visible scanlines to find sprites for the NEXT scanline.
#[derive(Debug, Clone)]
pub struct SpriteEvaluator {
    /// Secondary OAM for the current scanline
    pub secondary_oam: SecondaryOam,
    /// Sprite overflow flag (more than 8 sprites on scanline)
    pub overflow: bool,
    /// Current sprite height (8 or 16)
    sprite_height: u8,
}

crate::state::state_fields!(SpriteEvaluator {
    secondary_oam,
    overflow,
    sprite_height,
});

impl Default for SpriteEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl SpriteEvaluator {
    /// Create a new sprite evaluator
    pub fn new() -> Self {
        Self {
            secondary_oam: SecondaryOam::new(),
            overflow: false,
            sprite_height: 8,
        }
    }

    /// Reset the evaluator
    pub fn reset(&mut self) {
        self.secondary_oam.clear();
        self.overflow = false;
    }

    /// Evaluate sprites for a scanline
    ///
    /// This implements the sprite evaluation algorithm from the NES PPU.
    /// It scans through primary OAM and finds up to 8 sprites that are
    /// visible on the given scanline.
    ///
    /// # Arguments
    /// * `oam` - Primary OAM (256 bytes)
    /// * `scanline` - The scanline to evaluate for (0-239)
    /// * `ctrl` - PPUCTRL register value (for sprite size)
    /// * `oam_addr` - Current OAMADDR value (determines starting sprite)
    ///
    /// # Returns
    /// true if sprite overflow occurred
    pub fn evaluate(
        &mut self,
        oam: &[u8; 256],
        scanline: u16,
        ctrl: PpuCtrl,
        oam_addr: u8,
    ) -> bool {
        // Clear secondary OAM at the start of evaluation
        self.secondary_oam.clear();
        self.overflow = false;

        // Determine sprite height
        self.sprite_height = if ctrl.contains(PpuCtrl::SPRITE_SIZE) {
            16
        } else {
            8
        };

        // Start scanning from sprite n = OAMADDR / 4
        let start_n = (oam_addr >> 2) & 0x3F;
        let start_m = oam_addr & 0x03;

        let mut n = start_n as usize;
        let mut first_sprite = true;
        let mut evaluated = 0;
        // Byte index used while hunting for a 9th sprite. The hardware bug
        // increments this alongside the sprite index, so the Y comparisons
        // drift diagonally through OAM.
        let mut overflow_m = 0usize;

        // Scan through OAM looking for sprites on this scanline
        loop {
            let m = if self.secondary_oam.count >= 8 {
                // Overflow detection mode: use the buggy counter.
                overflow_m
            } else if first_sprite {
                start_m as usize
            } else {
                0
            };
            first_sprite = false;

            // Read Y coordinate (or whatever byte m points to if misaligned)
            let oam_index = (n * 4 + m) & 0xFF;
            let y_byte = oam[oam_index];

            // Check if sprite is on this scanline
            // Sprite Y is display line - 1, so visible range is (y+1) to (y+height)
            let sprite_y = y_byte as u16 + 1;

            if scanline >= sprite_y && scanline < sprite_y + self.sprite_height as u16 {
                // Sprite is on this scanline
                if self.secondary_oam.count < 8 {
                    // Copy the four bytes starting at the address we actually
                    // read, so a misaligned OAMADDR copies garbled data just
                    // like the hardware.
                    let sprite = Sprite {
                        y: oam[(n * 4 + m) & 0xFF],
                        tile: oam[(n * 4 + m + 1) & 0xFF],
                        attributes: oam[(n * 4 + m + 2) & 0xFF],
                        x: oam[(n * 4 + m + 3) & 0xFF],
                    };

                    // Check if this is sprite 0 (first in evaluation order)
                    let is_sprite_0 = evaluated == 0;
                    self.secondary_oam.add_sprite(&sprite, is_sprite_0);
                } else {
                    // 9th sprite found - set overflow flag
                    self.overflow = true;
                    break;
                }
            } else if self.secondary_oam.count >= 8 {
                // Sprite overflow bug: once eight sprites have been found,
                // the hardware increments the byte index as well as the sprite
                // index on every failed check, reading tile, attribute and X
                // bytes as if they were Y coordinates. This produces both
                // false positives and false negatives for the overflow flag.
                overflow_m = (overflow_m + 1) & 0x03;
            }

            n = (n + 1) & 0x3F;
            evaluated += 1;

            // Stop when we've wrapped around to sprite 0
            if n == 0 {
                break;
            }
        }

        self.overflow
    }

    /// Get the sprite height (8 or 16)
    pub fn sprite_height(&self) -> u8 {
        self.sprite_height
    }
}

/// Sprite renderer
///
/// Renders sprites to the frame buffer, handling priority and palette selection.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_attributes() {
        // Test attribute byte parsing
        // Bits: V H P P P - - -
        // V = vertical flip, H = horizontal flip, P = palette

        let attrs = SpriteAttributes::new(0b1100_0000); // V=1, H=1, P=0
        assert!(attrs.vertical_flip());
        assert!(attrs.horizontal_flip());
        assert!(!attrs.behind_background());
        assert_eq!(attrs.palette(), 0);

        let attrs = SpriteAttributes::new(0b0010_1000); // V=0, H=0, P=2, priority=1
        assert!(!attrs.vertical_flip());
        assert!(!attrs.horizontal_flip());
        assert!(attrs.behind_background());
        assert_eq!(attrs.palette(), 2);
    }

    #[test]
    fn test_sprite_from_bytes() {
        let bytes = [0x10, 0x42, 0x80, 0x20]; // Y=16, tile=66, attrs=128, X=32
        let sprite = Sprite::from_bytes(&bytes);

        assert_eq!(sprite.y, 0x10);
        assert_eq!(sprite.tile, 0x42);
        assert_eq!(sprite.attributes, 0x80);
        assert_eq!(sprite.x, 0x20);
    }

    #[test]
    fn test_sprite_is_on_scanline_8x8() {
        // Sprite at Y=10 means visible on scanlines 11-18 (Y+1 to Y+8)
        let sprite = Sprite {
            y: 10,
            tile: 0,
            attributes: 0,
            x: 0,
        };

        assert!(!sprite.is_on_scanline(10, 8)); // Scanline 10: just before
        assert!(sprite.is_on_scanline(11, 8)); // Scanline 11: first visible
        assert!(sprite.is_on_scanline(18, 8)); // Scanline 18: last visible
        assert!(!sprite.is_on_scanline(19, 8)); // Scanline 19: just after
    }

    #[test]
    fn test_sprite_is_on_scanline_8x16() {
        // Sprite at Y=10 means visible on scanlines 11-26 (Y+1 to Y+16)
        let sprite = Sprite {
            y: 10,
            tile: 0,
            attributes: 0,
            x: 0,
        };

        assert!(!sprite.is_on_scanline(10, 16)); // Scanline 10: just before
        assert!(sprite.is_on_scanline(11, 16)); // Scanline 11: first visible
        assert!(sprite.is_on_scanline(26, 16)); // Scanline 26: last visible
        assert!(!sprite.is_on_scanline(27, 16)); // Scanline 27: just after
    }

    #[test]
    fn test_sprite_get_fine_y() {
        let sprite = Sprite {
            y: 10,
            tile: 0,
            attributes: 0,
            x: 0,
        };

        // 8x8 sprite, no flip
        assert_eq!(sprite.get_fine_y(11, 8, false), 0); // First row
        assert_eq!(sprite.get_fine_y(15, 8, false), 4); // Middle row
        assert_eq!(sprite.get_fine_y(18, 8, false), 7); // Last row

        // 8x8 sprite, with vertical flip
        assert_eq!(sprite.get_fine_y(11, 8, true), 7); // First row (flipped)
        assert_eq!(sprite.get_fine_y(15, 8, true), 3); // Middle row (flipped)
        assert_eq!(sprite.get_fine_y(18, 8, true), 0); // Last row (flipped)
    }

    #[test]
    fn test_secondary_oam() {
        let mut soam = SecondaryOam::new();

        // Initially empty
        assert_eq!(soam.count(), 0);
        assert!(!soam.has_sprite_0());

        // Add some sprites
        let sprite1 = Sprite {
            y: 10,
            tile: 1,
            attributes: 0,
            x: 20,
        };
        let sprite2 = Sprite {
            y: 30,
            tile: 2,
            attributes: 0x80,
            x: 40,
        };

        assert!(soam.add_sprite(&sprite1, false));
        assert!(soam.add_sprite(&sprite2, true)); // This is sprite 0

        assert_eq!(soam.count(), 2);
        assert!(soam.has_sprite_0());

        // Retrieve sprites
        let retrieved1 = soam.get_sprite(0).unwrap();
        assert_eq!(retrieved1.y, 10);
        assert_eq!(retrieved1.tile, 1);
        assert_eq!(retrieved1.x, 20);

        let retrieved2 = soam.get_sprite(1).unwrap();
        assert_eq!(retrieved2.y, 30);
        assert_eq!(retrieved2.attributes, 0x80);

        // Clear and verify
        soam.clear();
        assert_eq!(soam.count(), 0);
        assert!(!soam.has_sprite_0());
    }

    #[test]
    fn test_secondary_oam_limit() {
        let mut soam = SecondaryOam::new();

        // Try to add 9 sprites
        for i in 0..9 {
            let sprite = Sprite {
                y: (i * 10) as u8,
                tile: i as u8,
                attributes: 0,
                x: 0,
            };

            if i < 8 {
                assert!(soam.add_sprite(&sprite, false));
            } else {
                assert!(!soam.add_sprite(&sprite, false)); // 9th should fail
            }
        }

        assert_eq!(soam.count(), 8);
    }

    #[test]
    fn test_sprite_evaluator_basic() {
        let mut evaluator = SpriteEvaluator::new();
        let mut oam = [0u8; 256];

        // Set up 3 sprites on scanline 20
        // Sprite 0: Y=19 (visible on scanline 20)
        oam[0] = 19;
        oam[1] = 0x01;
        oam[2] = 0x00;
        oam[3] = 10;

        // Sprite 1: Y=25 (not on scanline 20)
        oam[4] = 25;
        oam[5] = 0x02;
        oam[6] = 0x00;
        oam[7] = 20;

        // Sprite 2: Y=15 (visible on scanline 20)
        oam[8] = 15;
        oam[9] = 0x03;
        oam[10] = 0x00;
        oam[11] = 30;

        let ctrl = PpuCtrl::empty(); // 8x8 sprites
        let overflow = evaluator.evaluate(&oam, 20, ctrl, 0);

        assert!(!overflow);
        assert_eq!(evaluator.secondary_oam.count(), 2);
        assert!(evaluator.secondary_oam.has_sprite_0());

        // First sprite should be sprite 0 (Y=19)
        let first = evaluator.secondary_oam.get_sprite(0).unwrap();
        assert_eq!(first.y, 19);
        assert_eq!(first.tile, 0x01);
    }

    #[test]
    fn test_sprite_evaluator_overflow() {
        let mut evaluator = SpriteEvaluator::new();
        let mut oam = [0u8; 256];

        // Set up 10 sprites on scanline 20
        for i in 0..10 {
            oam[i * 4] = 19; // All on scanline 20
            oam[i * 4 + 1] = i as u8;
            oam[i * 4 + 2] = 0x00;
            oam[i * 4 + 3] = (i * 10) as u8;
        }

        let ctrl = PpuCtrl::empty();
        let overflow = evaluator.evaluate(&oam, 20, ctrl, 0);

        assert!(overflow);
        assert_eq!(evaluator.secondary_oam.count(), 8); // Max 8 sprites
    }

    /// Eight sprites are actually in range, but the overflow scan reads a tile
    /// byte as a Y coordinate and reports a false overflow.
    #[test]
    fn test_sprite_overflow_bug_reports_false_positive() {
        let mut evaluator = SpriteEvaluator::new();
        let mut oam = [0u8; 256];

        // Scanline 20 with 8x8 sprites: in range for Y in 0..=19.
        for i in 0..8 {
            oam[i * 4] = 19; // Y -> visible on scanline 20
        }
        // Sprite 8 is off-screen, so the buggy scan moves on to sprite 9 and
        // reads its *tile* byte, which we make look like an in-range Y.
        oam[8 * 4] = 0x00; // sprite 8 Y: out of range
        oam[9 * 4 + 1] = 19; // sprite 9 tile, read as Y by the buggy scan

        let overflow = evaluator.evaluate(&oam, 20, PpuCtrl::empty(), 0);

        assert!(
            overflow,
            "the sprite overflow bug should report a false positive"
        );
        assert_eq!(evaluator.secondary_oam.count(), 8);
    }

    /// Nine sprites are genuinely in range, but the buggy scan drifts past the
    /// ninth one and never reports overflow.
    #[test]
    fn test_sprite_overflow_bug_can_miss_a_real_overflow() {
        let mut evaluator = SpriteEvaluator::new();
        let mut oam = [0u8; 256];

        for i in 0..8 {
            oam[i * 4] = 19;
        }
        // Sprite 8 is off-screen; sprite 9 *is* on the scanline but the scan
        // reads its tile byte (0x00) instead of its Y.
        oam[8 * 4] = 0x00;
        oam[9 * 4] = 19; // sprite 9 Y: in range, never examined correctly
        oam[9 * 4 + 1] = 0x00; // tile byte read instead

        let overflow = evaluator.evaluate(&oam, 20, PpuCtrl::empty(), 0);

        assert!(
            !overflow,
            "the sprite overflow bug should be able to miss a real overflow"
        );
        assert_eq!(evaluator.secondary_oam.count(), 8);
    }
}
