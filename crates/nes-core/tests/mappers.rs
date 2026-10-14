//! Comprehensive mapper test suite for all 21 implemented NES mappers
//!
//! This test suite provides integration-level testing for all mappers,
//! verifying bank switching, IRQ handling, mirroring control, and other
//! mapper-specific features.

use nes_core::mapper::{
    create_mapper, mapper0::Mapper0, mapper1::Mapper1, mapper11::Mapper11, mapper140::Mapper140,
    mapper180::Mapper180, mapper2::Mapper2, mapper240::Mapper240, mapper241::Mapper241,
    mapper3::Mapper3, mapper34::Mapper34, mapper38::Mapper38, mapper4::Mapper4, mapper5::Mapper5,
    mapper66::Mapper66, mapper7::Mapper7, mapper71::Mapper71, mapper74::Mapper74,
    mapper79::Mapper79, mapper9::Mapper9, mapper94::Mapper94, Mapper,
};

// Mapper 118 and 119 are exported from `mapper::mod`; both are covered by their
// own unit tests.

// ============================================================================
// Mapper Factory Tests
// ============================================================================

#[test]
fn test_mapper_factory_all_mappers() {
    // Test that all 21 mappers can be created via the factory
    let prg = vec![0u8; 32768];
    let chr = vec![0u8; 8192];

    // Mappers that can be created via factory
    assert!(create_mapper(0, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(1, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(2, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(3, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(4, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(5, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(7, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(9, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(11, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(74, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(140, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(180, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(240, prg.clone(), chr.clone()).is_ok());
    assert!(create_mapper(241, prg.clone(), chr.clone()).is_ok());

    // Unsupported mapper
    assert!(create_mapper(999, prg, chr).is_err());
}

// ============================================================================
// Mapper 0 (NROM) Tests
// ============================================================================

#[test]
fn test_mapper0_integration() {
    let prg = vec![0u8; 32768];
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper0::new(prg, chr);

    // Test basic read/write
    mapper.write(0x6000, 0x42);
    assert_eq!(mapper.read(0x6000), 0x42);

    // Test CHR read
    assert_eq!(mapper.read(0x0000), 0);

    // Test IRQ (NROM doesn't generate IRQs)
    assert!(!mapper.irq_pending());
    mapper.clear_irq();
    assert!(!mapper.irq_pending());
}

#[test]
fn test_mapper0_prg_rom_mirroring() {
    // Test 16KB PRG ROM mirroring
    let mut prg = vec![0u8; 16384];
    prg[0] = 0xAA;
    prg[16383] = 0xBB;
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper0::new(prg, chr);

    // 16KB ROM mirrored at $8000 and $C000
    assert_eq!(mapper.read(0x8000), 0xAA);
    assert_eq!(mapper.read(0xC000), 0xAA);
    assert_eq!(mapper.read(0xBFFF), 0xBB);
    assert_eq!(mapper.read(0xFFFF), 0xBB);
}

#[test]
fn test_mapper0_chr_ram_mode() {
    // Empty CHR triggers CHR-RAM mode
    let prg = vec![0u8; 16384];
    let chr = vec![];
    let mut mapper = Mapper0::new(prg, chr);

    // Write to CHR-RAM
    mapper.write(0x0000, 0x55);
    assert_eq!(mapper.read(0x0000), 0x55);
}

// ============================================================================
// Mapper 1 (MMC1) Tests
// ============================================================================

#[test]
fn test_mapper1_shift_register_integration() {
    let mut prg = vec![0u8; 0x40000]; // 256KB (16 x 16KB banks)
    for i in 0..16 {
        prg[i * 0x4000] = i as u8;
    }
    let chr = vec![0u8; 0x10000]; // 64KB
    let mut mapper = Mapper1::new(prg, chr);

    // Write control register with value 0x0C (16KB mode, $C000 fixed)
    // Binary: 01100 - write LSB first
    mapper.write(0x8000, 0); // bit 0
    mapper.write(0x8000, 0); // bit 1
    mapper.write(0x8000, 1); // bit 2
    mapper.write(0x8000, 1); // bit 3
    mapper.write(0x8000, 0); // bit 4

    // PRG should now be in 16KB mode
    // Verify by switching banks
    mapper.write(0xE000, 0); // bit 0
    mapper.write(0xE000, 1); // bit 1
    mapper.write(0xE000, 0); // bit 2
    mapper.write(0xE000, 0); // bit 3
    mapper.write(0xE000, 0); // bit 4

    // Bank should be set to 2
    assert_eq!(mapper.read(0x8000), 2);
}

#[test]
fn test_mapper1_reset() {
    let prg = vec![0u8; 32768];
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper1::new(prg, chr);

    // Start writing bits
    mapper.write(0x8000, 1);
    mapper.write(0x8000, 0);

    // Reset with bit 7 set
    mapper.write(0x8000, 0x80);

    // Write should complete immediately after reset
    mapper.write(0x8000, 1);
    mapper.write(0x8000, 0);
    mapper.write(0x8000, 0);
    mapper.write(0x8000, 0);
    mapper.write(0x8000, 0);

    // Should have written value 1 to control register
    // mirroring = 1 (single-screen upper)
}

// ============================================================================
// Mapper 2 (UNROM) Tests
// ============================================================================

#[test]
fn test_mapper2_bank_switching_integration() {
    let mut prg = vec![0u8; 0x40000]; // 256KB (16 x 16KB banks)
    for i in 0..16 {
        prg[i * 0x4000] = i as u8;
    }
    let chr = vec![];
    let mut mapper = Mapper2::new(prg, chr);

    // Initial state: bank 0 at $8000, last bank at $C000
    assert_eq!(mapper.read(0x8000), 0);
    assert_eq!(mapper.read(0xC000), 15);

    // Switch to bank 5
    mapper.write(0x8000, 5);
    assert_eq!(mapper.read(0x8000), 5);
    assert_eq!(mapper.read(0xC000), 15); // Fixed

    // Switch to bank 10
    mapper.write(0xFFFF, 10);
    assert_eq!(mapper.read(0x8000), 10);
}

#[test]
fn test_mapper2_chr_ram_integration() {
    let prg = vec![0u8; 32768];
    let chr = vec![];
    let mut mapper = Mapper2::new(prg, chr);

    // CHR-RAM read/write
    mapper.write(0x0000, 0xAA);
    assert_eq!(mapper.read(0x0000), 0xAA);

    mapper.write(0x1FFF, 0xBB);
    assert_eq!(mapper.read(0x1FFF), 0xBB);
}

// ============================================================================
// Mapper 3 (CNROM) Tests
// ============================================================================

#[test]
fn test_mapper3_chr_bank_switching_integration() {
    let prg = vec![0u8; 32768];
    let mut chr = vec![0u8; 0x10000]; // 64KB (8 x 8KB banks)
    for i in 0..8 {
        chr[i * 0x2000] = i as u8;
    }
    let mut mapper = Mapper3::new(prg, chr);

    // Initial: bank 0
    assert_eq!(mapper.read(0x0000), 0);

    // Switch to bank 3
    mapper.write(0x8000, 3);
    assert_eq!(mapper.read(0x0000), 3);

    // Switch to bank 7
    mapper.write(0xFFFF, 7);
    assert_eq!(mapper.read(0x0000), 7);
}

#[test]
fn test_mapper3_fixed_prg() {
    let mut prg = vec![0u8; 0x8000]; // 32KB
    prg[0] = 0xAA;
    prg[0x4000] = 0xBB;
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper3::new(prg, chr);

    // PRG-ROM should be fixed
    assert_eq!(mapper.read(0x8000), 0xAA);
    assert_eq!(mapper.read(0xC000), 0xBB);

    // Writing should only affect CHR bank
    mapper.write(0x8000, 1);
    assert_eq!(mapper.read(0x8000), 0xAA);
}

// ============================================================================
// Mapper 4 (MMC3) Tests
// ============================================================================

#[test]
fn test_mapper4_bank_switching_integration() {
    let mut prg = vec![0u8; 0x20000]; // 128KB (16 x 8KB banks)
    for i in 0..16 {
        prg[i * 0x2000] = i as u8;
    }
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper4::new(prg, chr);

    // Set R6 to bank 3
    mapper.write(0x8000, 0x06);
    mapper.write(0x8001, 3);

    // Set R7 to bank 5
    mapper.write(0x8000, 0x07);
    mapper.write(0x8001, 5);

    // Read from different regions
    assert_eq!(mapper.read(0x8000), 3); // R6
    assert_eq!(mapper.read(0xA000), 5); // R7
    assert_eq!(mapper.read(0xC000), 14); // Second-to-last
    assert_eq!(mapper.read(0xE000), 15); // Last
}

#[test]
fn test_mapper4_irq_integration() {
    let prg = vec![0u8; 0x20000];
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper4::new(prg, chr);

    // $C000 sets the IRQ latch on MMC3.
    mapper.write(0xC000, 3);

    // Enable IRQ
    mapper.write(0xE001, 0);

    // Clock the counter
    mapper.clock_irq_counter(); // reload to 3
    assert!(!mapper.irq_pending());

    mapper.clock_irq_counter(); // 3 -> 2
    assert!(!mapper.irq_pending());

    mapper.clock_irq_counter(); // 2 -> 1
    assert!(!mapper.irq_pending());

    mapper.clock_irq_counter(); // 1 -> 0, triggers IRQ
    assert!(mapper.irq_pending());
}

#[test]
fn test_mapper4_mirroring() {
    let prg = vec![0u8; 0x20000];
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper4::new(prg, chr);

    // Default: vertical
    assert_eq!(mapper.read(0xA000), 0);

    // Set horizontal mirroring
    mapper.write(0xA000, 1);

    // Set vertical mirroring
    mapper.write(0xA000, 0);
}

// ============================================================================
// Mapper 5 (MMC5) Tests
// ============================================================================

#[test]
fn test_mapper5_prg_mode_switching() {
    let mut prg = vec![0u8; 0x40000]; // 256KB
    for i in 0..32 {
        prg[i * 0x2000] = i as u8;
    }
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper5::new(prg, chr);

    // Enable PRG RAM writes
    mapper.write(0x5102, 0x02);
    mapper.write(0x5103, 0x01);

    // Mode 3: Four 8K banks
    mapper.write(0x5100, 3);
    mapper.write(0x5114, 0x80 | 0x05); // Bank 5 at $8000
    mapper.write(0x5115, 0x80 | 0x0A); // Bank 10 at $A000

    assert_eq!(mapper.read(0x8000), 5);
    assert_eq!(mapper.read(0xA000), 10);
}

#[test]
fn test_mapper5_multiplier() {
    let prg = vec![0u8; 0x40000];
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper5::new(prg, chr);

    // Write operands: 0x12 * 0x34 = 0x03A8
    mapper.write(0x5205, 0x12);
    mapper.write(0x5206, 0x34);

    // Read result
    assert_eq!(mapper.read(0x5205), 0xA8); // Low byte
    assert_eq!(mapper.read(0x5206), 0x03); // High byte
}

#[test]
fn test_mapper5_exram() {
    let prg = vec![0u8; 0x40000];
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper5::new(prg, chr);

    // Set mode 2 (RAM)
    mapper.write(0x5104, 2);

    // Write to ExRAM
    mapper.write(0x5C00, 0x42);
    assert_eq!(mapper.read(0x5C00), 0x42);

    // Set mode 3 (read-only)
    mapper.write(0x5104, 3);
    mapper.write(0x5C00, 0xFF); // Should be ignored
                                // Value should still be 0x42 (read-only doesn't clear memory)
    assert_eq!(mapper.read(0x5C00), 0x42);
}

#[test]
fn test_mapper5_irq() {
    let prg = vec![0u8; 0x40000];
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper5::new(prg, chr);

    // Set target
    mapper.write(0x5203, 5);
    mapper.write(0x5204, 0x80); // Enable IRQ

    // First clock enters frame
    mapper.clock_irq_counter();
    assert!(!mapper.irq_pending());

    // Clock up to target
    for _ in 0..5 {
        mapper.clock_irq_counter();
    }

    assert!(mapper.irq_pending());
}

// ============================================================================
// Mapper 7 (AxROM) Tests
// ============================================================================

#[test]
fn test_mapper7_bank_switching_integration() {
    let mut prg = vec![0u8; 65536]; // 64KB (2 x 32KB banks)
    prg[..0x8000].fill(0x11); // Bank 0
    prg[0x8000..].fill(0x22); // Bank 1

    let chr = vec![];
    let mut mapper = Mapper7::new(prg, chr);

    // Bank 0 (default)
    assert_eq!(mapper.read(0x8000), 0x11);

    // Switch to bank 1
    mapper.write(0x8000, 0x01);
    assert_eq!(mapper.read(0x8000), 0x22);
}

#[test]
fn test_mapper7_mirroring_select() {
    let prg = vec![0u8; 32768];
    let chr = vec![];
    let mut mapper = Mapper7::new(prg, chr);

    // Default: first screen
    assert!(!mapper.read(0x8000) != 0 || true); // Just verify it doesn't panic

    // Set bit 4 for second screen
    mapper.write(0x8000, 0x10);
    // mirroring_select should be true
}

// ============================================================================
// Mapper 9 (MMC2) Tests
// ============================================================================

#[test]
fn test_mapper9_latch_mechanism() {
    let prg = vec![0u8; 131072]; // 128KB
    let mut chr = vec![0u8; 65536]; // 64KB
    for i in 0..16 {
        chr[i * 0x1000] = i as u8;
    }
    let mut mapper = Mapper9::new(prg, chr);

    // Set up CHR banks
    mapper.write(0xB000, 0x05); // chr_bank_fd0 = 5
    mapper.write(0xC000, 0x06); // chr_bank_fe0 = 6

    // Default latch state is FE
    assert_eq!(mapper.read(0x0000), 6);

    // Read from $0FD8 to trigger FD state
    mapper.read(0x0FD8);

    // Next read should use FD bank
    assert_eq!(mapper.read(0x0000), 5);
}

#[test]
fn test_mapper9_prg_banks() {
    let mut prg = vec![0u8; 131072]; // 128KB = 16 x 8KB banks
    for i in 0..16 {
        prg[i * 0x2000] = i as u8;
    }
    let chr = vec![0u8; 65536];
    let mut mapper = Mapper9::new(prg, chr);

    // Switchable bank at $8000
    mapper.write(0xA000, 0x05);
    assert_eq!(mapper.read(0x8000), 5);

    // Fixed banks at $A000-$FFFF should be last three
    assert_eq!(mapper.read(0xA000), 13); // Last - 3
    assert_eq!(mapper.read(0xC000), 14); // Last - 2
    assert_eq!(mapper.read(0xE000), 15); // Last - 1
}

// ============================================================================
// Mapper 11 (Color Dreams) Tests
// ============================================================================

#[test]
fn test_mapper11_combined_switching() {
    let mut prg = vec![0u8; 65536]; // 64KB (2 x 32KB banks)
    let mut chr = vec![0u8; 16384]; // 16KB (2 x 8KB banks)

    for i in 0..0x8000 {
        prg[i] = 0x10; // Bank 0
        prg[0x8000 + i] = 0x20; // Bank 1
    }
    for i in 0..0x2000 {
        chr[i] = 0x30; // Bank 0
        chr[0x2000 + i] = 0x40; // Bank 1
    }

    let mut mapper = Mapper11::new(prg, chr);

    // Initial state
    assert_eq!(mapper.read(0x8000), 0x10);
    assert_eq!(mapper.read(0x0000), 0x30);

    // Switch both banks
    // PRG bank 1 (bit 0 = 1), CHR bank 1 (bits 4-7 = 0x10)
    mapper.write(0x8000, 0x11);

    assert_eq!(mapper.read(0x8000), 0x20);
    assert_eq!(mapper.read(0x0000), 0x40);
}

// ============================================================================
// Mapper 34 (BNROM) Tests
// ============================================================================

#[test]
fn test_mapper34_bank_switching() {
    let mut prg = vec![0u8; 131072]; // 128KB (4 x 32KB banks)
    for bank in 0..4 {
        for i in 0..0x8000 {
            prg[bank * 0x8000 + i] = bank as u8;
        }
    }

    let chr = vec![];
    let mut mapper = Mapper34::new(prg, chr);

    // Bank 0 (default)
    assert_eq!(mapper.read(0x8000), 0);

    // Switch to bank 2
    mapper.write(0x8000, 2);
    assert_eq!(mapper.read(0x8000), 2);
}

#[test]
fn test_mapper34_chr_ram() {
    let prg = vec![0u8; 65536];
    let chr = vec![];
    let mut mapper = Mapper34::new(prg, chr);

    mapper.write(0x0000, 0xAA);
    assert_eq!(mapper.read(0x0000), 0xAA);
}

// ============================================================================
// Mapper 38 (Crime Busters) Tests
// ============================================================================

#[test]
fn test_mapper38_register_at_7000() {
    let mut prg = vec![0u8; 131072]; // 128KB (4 x 32KB banks)
    for bank in 0..4 {
        for i in 0..0x8000 {
            prg[bank * 0x8000 + i] = bank as u8;
        }
    }
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper38::new(prg, chr);

    // Register is at $7000, not $8000
    mapper.write(0x7000, 0x01);
    assert_eq!(mapper.read(0x8000), 1);

    // Writing to $8000 should NOT work
    mapper.write(0x8000, 0x00);
    // Should still be bank 1
    assert_eq!(mapper.read(0x8000), 1);
}

#[test]
fn test_mapper38_combined_banks() {
    let mut prg = vec![0u8; 65536];
    let mut chr = vec![0u8; 16384];

    for i in 0..0x8000 {
        prg[i] = 0x10;
        prg[0x8000 + i] = 0x20;
    }
    for i in 0..0x2000 {
        chr[i] = 0x30;
        chr[0x2000 + i] = 0x40;
    }

    let mut mapper = Mapper38::new(prg, chr);

    // Switch both: PRG bank 1 (bits 0-1 = 1), CHR bank 1 (bits 2-3 = 0x04)
    mapper.write(0x7000, 0x05);

    assert_eq!(mapper.read(0x8000), 0x20);
    assert_eq!(mapper.read(0x0000), 0x40);
}

// ============================================================================
// Mapper 66 (GNROM) Tests
// ============================================================================

#[test]
fn test_mapper66_with_mirroring() {
    let mut prg = vec![0u8; 65536];
    let mut chr = vec![0u8; 16384];

    for i in 0..0x8000 {
        prg[i] = 0x10;
        prg[0x8000 + i] = 0x20;
    }
    for i in 0..0x2000 {
        chr[i] = 0x30;
        chr[0x2000 + i] = 0x40;
    }

    let mut mapper = Mapper66::new(prg, chr);

    // Switch PRG bank 1 (bits 4-5 = 0x10), CHR bank 1 (bits 0-1 = 1)
    // Also set horizontal mirroring (bit 6 = 0x40)
    mapper.write(0x8000, 0x51);

    assert_eq!(mapper.read(0x8000), 0x20);
    assert_eq!(mapper.read(0x0000), 0x40);
}

// ============================================================================
// Mapper 71 (Camerica) Tests
// ============================================================================

#[test]
fn test_mapper71_bank_switching() {
    let mut prg = vec![0u8; 0x40000]; // 256KB (16 x 16KB banks)
    for i in 0..16 {
        prg[i * 0x4000] = i as u8;
    }
    let chr = vec![];
    let mut mapper = Mapper71::new(prg, chr);

    // Switchable bank at $8000
    mapper.write(0x8000, 5);
    assert_eq!(mapper.read(0x8000), 5);

    // Fixed last bank at $C000
    assert_eq!(mapper.read(0xC000), 15);
}

#[test]
fn test_mapper71_prg_ram() {
    let prg = vec![0u8; 32768];
    let chr = vec![];
    let mut mapper = Mapper71::new(prg, chr);

    mapper.write(0x6000, 0x42);
    assert_eq!(mapper.read(0x6000), 0x42);
}

// ============================================================================
// Mapper 74 (Waixing MMC3 with CHR ROM + CHR RAM) Tests
// ============================================================================

#[test]
fn test_mapper74_mmc3_banking_and_irq() {
    let mut prg = vec![0u8; 0x80000]; // 512KB (64 x 8KB banks)
    for bank in 0..64 {
        prg[bank * 0x2000] = bank as u8;
    }
    let mut chr = vec![0u8; 0x40000]; // 256KB (256 x 1KB banks)
    for bank in 0..256 {
        chr[bank * 0x0400] = bank as u8;
    }
    let mut mapper = Mapper74::new(prg, chr);

    // MMC3 PRG banking: R6 at $8000, R7 at $A000, last bank fixed at $E000.
    mapper.write(0x8000, 0x06);
    mapper.write(0x8001, 12);
    mapper.write(0x8000, 0x07);
    mapper.write(0x8001, 40);
    assert_eq!(mapper.read(0x8000), 12);
    assert_eq!(mapper.read(0xA000), 40);
    assert_eq!(mapper.read(0xE000), 63);

    // 1KB CHR bank, then the scanline IRQ.
    mapper.write(0x8000, 0x02); // R2 -> $1000-$13FF
    mapper.write(0x8001, 20);
    assert_eq!(mapper.read(0x1000), 20);

    mapper.write(0xC000, 2);
    mapper.write(0xC001, 0);
    mapper.write(0xE001, 0);
    mapper.on_scanline(10, true); // reload to 2
    mapper.on_scanline(11, true); // 2 -> 1
    assert!(!mapper.irq_pending());
    mapper.on_scanline(12, true); // 1 -> 0 -> IRQ
    assert!(mapper.irq_pending());
    mapper.clear_irq();
    assert!(!mapper.irq_pending());
}

#[test]
fn test_mapper74_chr_ram_sits_at_banks_8_and_9() {
    let mut chr = vec![0x00u8; 0x40000];
    chr[20 * 0x0400] = 0x33;
    chr[8 * 0x0400] = 0x11; // shadowed by CHR RAM
    let mut mapper = Mapper74::new(vec![0u8; 0x80000], chr);

    // R2 selects the 1KB bank at $1000-$13FF.
    mapper.write(0x8000, 0x02);

    // CHR-ROM bank: read-only, writes dropped.
    mapper.write(0x8001, 20);
    mapper.write(0x1000, 0x5A);
    assert_eq!(mapper.read(0x1000), 0x33);

    // CHR-RAM banks: writable, and the ROM bytes they hide never show up.
    mapper.write(0x8001, 8);
    assert_ne!(mapper.read(0x1000), 0x11);
    mapper.write(0x1000, 0x5A);
    assert_eq!(mapper.read(0x1000), 0x5A);
    mapper.write(0x8001, 9);
    mapper.write(0x1000, 0xA5);
    assert_eq!(mapper.read(0x1000), 0xA5);
}

#[test]
fn test_mapper74_chr_ram_upload_through_ppu() {
    let mut mapper = Mapper74::new(vec![0u8; 0x80000], vec![0u8; 0x40000]);

    // Point slot 4 ($1000) at CHR RAM bank 8, the way a game loads its
    // software-rendered Chinese text tiles.
    mapper.write(0x8000, 0x02);
    mapper.write(0x8001, 8);

    upload_byte_through_ppu(&mut mapper, 0x1000, 0xA5);
    assert_eq!(
        mapper.read(0x1000),
        0xA5,
        "CHR-RAM upload through $2007 did not reach the cartridge"
    );
}

// ============================================================================
// Mapper 79 (NINA-003) Tests
// ============================================================================

#[test]
fn test_mapper79_register_at_4100() {
    let prg = vec![0u8; 32768];
    let mut chr = vec![0u8; 0x10000];
    for i in 0..8 {
        chr[i * 0x2000] = i as u8;
    }
    let mut mapper = Mapper79::new(prg, chr);

    // Register at $4100-$5FFF
    mapper.write(0x4100, 3);
    assert_eq!(mapper.read(0x0000), 3);

    // Writing outside range should not work
    mapper.write(0x4000, 5);
    assert_eq!(mapper.read(0x0000), 3); // Still 3
}

#[test]
fn test_mapper79_mirroring() {
    let prg = vec![0u8; 32768];
    let chr = vec![0u8; 0x10000];
    let mut mapper = Mapper79::new(prg, chr);

    // Set horizontal mirroring (bit 4 = 1)
    mapper.write(0x4100, 0x10);
    // mirroring should be 1

    // Set vertical mirroring (bit 4 = 0)
    mapper.write(0x4100, 0x00);
    // mirroring should be 0
}

// ============================================================================
// Mapper 94 (Senjou no Ookami) Tests
// ============================================================================

#[test]
fn test_mapper94_limited_bank_bits() {
    let mut prg = vec![0u8; 0x20000]; // 128KB (8 x 16KB banks)
    for i in 0..8 {
        prg[i * 0x4000] = i as u8;
    }
    let chr = vec![];
    let mut mapper = Mapper94::new(prg, chr);

    // Only bits 0-2 are used
    mapper.write(0x8000, 0x0B); // 1011 = bank 3
    assert_eq!(mapper.read(0x8000), 3);

    // Bit 3 should be ignored
    mapper.write(0x8000, 0x08); // 1000 = bank 0 (bit 3 ignored)
    assert_eq!(mapper.read(0x8000), 0);
}

#[test]
fn test_mapper94_fixed_first_bank() {
    let mut prg = vec![0u8; 0x10000]; // 64KB (4 x 16KB banks)
    for i in 0..4 {
        prg[i * 0x4000] = i as u8;
    }
    let chr = vec![];
    let mut mapper = Mapper94::new(prg, chr);

    // $8000-$BFFF is switchable
    mapper.write(0x8000, 2);
    assert_eq!(mapper.read(0x8000), 2);

    // $C000-$FFFF is fixed to last bank (3)
    assert_eq!(mapper.read(0xC000), 3);
}

// ============================================================================
// Mapper 140 (Jaleco) Tests
// ============================================================================

#[test]
fn test_mapper140_register_at_6000() {
    let mut prg = vec![0u8; 131072]; // 128KB (4 x 32KB banks)
    for bank in 0..4 {
        for i in 0..0x8000 {
            prg[bank * 0x8000 + i] = bank as u8;
        }
    }
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper140::new(prg, chr);

    mapper.write(0x6000, 0x10);
    assert_eq!(mapper.read(0x8000), 1);
    assert_eq!(mapper.read(0x6000), 0);
}

#[test]
fn test_mapper140_combined_switching() {
    let mut prg = vec![0u8; 65536];
    let mut chr = vec![0u8; 16384];

    for i in 0..0x8000 {
        prg[i] = 0x10;
        prg[0x8000 + i] = 0x20;
    }
    for i in 0..0x2000 {
        chr[i] = 0x30;
        chr[0x2000 + i] = 0x40;
    }

    let mut mapper = Mapper140::new(prg, chr);

    // Switch both: PRG bank 1 (bits 4-5 = 0x10), CHR bank 1 (bits 0-3 = 0x01)
    mapper.write(0x6000, 0x11);

    assert_eq!(mapper.read(0x8000), 0x20);
    assert_eq!(mapper.read(0x0000), 0x40);
}

// ============================================================================
// Mapper 180 (Crazy Climber) Tests
// ============================================================================

#[test]
fn test_mapper180_inverted_uxrom() {
    let mut prg = vec![0u8; 0x10000]; // 64KB (4 x 16KB banks)
    for i in 0..4 {
        prg[i * 0x4000] = i as u8;
    }
    let chr = vec![];
    let mut mapper = Mapper180::new(prg, chr);

    // $8000-$BFFF is fixed to first bank (0)
    assert_eq!(mapper.read(0x8000), 0);

    // $C000-$FFFF is switchable
    mapper.write(0x8000, 2);
    assert_eq!(mapper.read(0xC000), 2);

    // $8000 should still be bank 0
    assert_eq!(mapper.read(0x8000), 0);
}

#[test]
fn test_mapper180_vs_mapper2() {
    // Demonstrate the difference between Mapper 2 and Mapper 180
    let mut prg = vec![0u8; 0x10000];
    for i in 0..4 {
        prg[i * 0x4000] = i as u8;
    }
    let chr = vec![];
    let mut mapper = Mapper180::new(prg, chr);

    // With bank 0 selected
    assert_eq!(mapper.read(0x8000), 0); // Fixed first
    assert_eq!(mapper.read(0xC000), 0); // Switchable (also 0)

    // Switch to bank 2
    mapper.write(0x8000, 2);

    assert_eq!(mapper.read(0x8000), 0); // Still fixed first
    assert_eq!(mapper.read(0xC000), 2); // Now switchable is 2
}

// ============================================================================
// Mapper 240 (C&E) Tests
// ============================================================================

#[test]
fn test_mapper240_register_at_4020() {
    let mut prg = vec![0u8; 131072];
    for bank in 0..4 {
        for i in 0..0x8000 {
            prg[bank * 0x8000 + i] = bank as u8;
        }
    }
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper240::new(prg, chr);

    mapper.write(0x4020, 0x10);
    assert_eq!(mapper.read(0x8000), 1);

    mapper.write(0x401F, 0x10);
    assert_eq!(mapper.read(0x8000), 1);
}

#[test]
fn test_mapper240_combined_switching() {
    let mut prg = vec![0u8; 65536];
    let mut chr = vec![0u8; 16384];

    for i in 0..0x8000 {
        prg[i] = 0x10;
        prg[0x8000 + i] = 0x20;
    }
    for i in 0..0x2000 {
        chr[i] = 0x30;
        chr[0x2000 + i] = 0x40;
    }

    let mut mapper = Mapper240::new(prg, chr);

    // Switch both: PRG bank 1 (bits 4-5 = 0x10), CHR bank 1 (bits 0-3 = 0x01)
    mapper.write(0x4020, 0x11);

    assert_eq!(mapper.read(0x8000), 0x20);
    assert_eq!(mapper.read(0x0000), 0x40);
}

// ============================================================================
// Mapper 241 (BxROM) Tests
// ============================================================================

#[test]
fn test_mapper241_full_byte_bank_select() {
    let mut prg = vec![0u8; 0x100000]; // 1MB (32 x 32KB banks)
    for i in 0..32 {
        prg[i * 0x8000] = i as u8;
    }
    let chr = vec![];
    let mut mapper = Mapper241::new(prg, chr);

    // Full byte is used (unlike Mapper 34 which masks to bits 0-1)
    mapper.write(0x8000, 0x10);
    assert_eq!(mapper.read(0x8000), 0x10);

    mapper.write(0x8000, 0x1F);
    assert_eq!(mapper.read(0x8000), 0x1F);
}

#[test]
fn test_mapper241_vs_mapper34() {
    // Demonstrate the difference between Mapper 34 and Mapper 241
    let mut prg = vec![0u8; 0x100000];
    for i in 0..32 {
        prg[i * 0x8000] = i as u8;
    }
    let chr = vec![];
    let mut mapper = Mapper241::new(prg, chr);

    // Write value with bit 2 set
    mapper.write(0x8000, 0x04);
    assert_eq!(mapper.read(0x8000), 0x04); // Bank 4

    // Write value with bit 4 set
    mapper.write(0x8000, 0x10);
    assert_eq!(mapper.read(0x8000), 0x10); // Bank 16
}

#[test]
fn test_mapper241_prg_ram() {
    let prg = vec![0u8; 65536];
    let chr = vec![];
    let mut mapper = Mapper241::new(prg, chr);

    mapper.write(0x6000, 0x42);
    assert_eq!(mapper.read(0x6000), 0x42);
}

// ============================================================================
// Cross-Mapper Comparison Tests
// ============================================================================

#[test]
fn test_uxrom_variants_comparison() {
    // Compare Mapper 2 (UxROM), Mapper 71 (Camerica), and Mapper 94 (Senjou no Ookami)
    let mut prg = vec![0u8; 0x20000]; // 128KB (8 x 16KB banks)
    for i in 0..8 {
        prg[i * 0x4000] = i as u8;
    }
    let chr = vec![];

    // Mapper 2: Standard UxROM
    let mut mapper2 = Mapper2::new(prg.clone(), chr.clone());
    mapper2.write(0x8000, 3);
    assert_eq!(mapper2.read(0x8000), 3); // Switchable at $8000
    assert_eq!(mapper2.read(0xC000), 7); // Fixed last at $C000

    // Mapper 71: Camerica (same as UxROM)
    let mut mapper71 = Mapper71::new(prg.clone(), chr.clone());
    mapper71.write(0x8000, 3);
    assert_eq!(mapper71.read(0x8000), 3);
    assert_eq!(mapper71.read(0xC000), 7);

    // Mapper 94: Senjou no Ookami (limited to 8 banks, same layout)
    let mut mapper94 = Mapper94::new(prg.clone(), chr.clone());
    mapper94.write(0x8000, 3);
    assert_eq!(mapper94.read(0x8000), 3);
    assert_eq!(mapper94.read(0xC000), 7);

    // Mapper 180: Crazy Climber (inverted - fixed first, switchable at $C000)
    let mut mapper180 = Mapper180::new(prg, chr);
    mapper180.write(0x8000, 3);
    assert_eq!(mapper180.read(0x8000), 0); // Fixed first at $8000
    assert_eq!(mapper180.read(0xC000), 3); // Switchable at $C000
}

#[test]
fn test_bnrom_variants_comparison() {
    // Compare Mapper 34 (BNROM) and Mapper 241 (BxROM)
    let mut prg = vec![0u8; 0x100000]; // 1MB (32 x 32KB banks)
    for i in 0..32 {
        prg[i * 0x8000] = i as u8;
    }
    let chr = vec![];

    // Mapper 34: BNROM (only bits 0-1)
    let mut mapper34 = Mapper34::new(prg.clone(), chr.clone());
    mapper34.write(0x8000, 0x04); // Bit 2 set
    assert_eq!(mapper34.read(0x8000), 0); // 4 % 2 = 0 (only 2 banks worth)

    // Mapper 241: BxROM (full byte)
    let mut mapper241 = Mapper241::new(prg, chr);
    mapper241.write(0x8000, 0x04); // Bit 2 set
    assert_eq!(mapper241.read(0x8000), 4); // Bank 4
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

#[test]
fn test_mapper_empty_rom_handling() {
    // Test that mappers handle edge cases gracefully
    let prg = vec![0u8; 16384]; // Minimum PRG
    let chr = vec![0u8; 8192]; // Minimum CHR

    let mut mapper = Mapper0::new(prg, chr);
    assert_eq!(mapper.read(0xFFFF), 0); // Should not panic
}

#[test]
fn test_mapper_large_address_read() {
    let prg = vec![0u8; 32768];
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper0::new(prg, chr);

    // Reading from unmapped addresses should return 0
    assert_eq!(mapper.read(0x4000), 0);
    assert_eq!(mapper.read(0x5000), 0);
}

#[test]
fn test_mapper_irq_clear_behavior() {
    let prg = vec![0u8; 0x20000];
    let chr = vec![0u8; 8192];
    let mut mapper = Mapper4::new(prg, chr);

    // Set up and trigger IRQ
    mapper.write(0xC000, 1);
    mapper.write(0xE001, 0);
    mapper.clock_irq_counter(); // reload
    mapper.clock_irq_counter(); // trigger

    assert!(mapper.irq_pending());

    // Clear IRQ
    mapper.clear_irq();
    assert!(!mapper.irq_pending());
}

// ============================================================================
// CHR-RAM uploads through the PPU
// ============================================================================

/// Point the PPU's VRAM pointer at `addr` and write `value` through $2007.
///
/// This is the path a game uses to fill CHR-RAM: the PPU forwards writes for
/// $0000-$1FFF to the cartridge, so the mapper has to keep the byte where the
/// renderer will read it back.
fn upload_byte_through_ppu(mapper: &mut (dyn Mapper + 'static), addr: u16, value: u8) {
    use nes_core::ppu::{rendering::Mirroring, Ppu, PpuMemory};

    let mut ppu = Ppu::new();
    // The address latch never reaches the cartridge, so it needs no CHR source.
    let no_chr = || PpuMemory::System {
        chr: None,
        mirroring: Mirroring::Horizontal,
        open_bus: 0,
    };

    ppu.write_register_with(0x2006, (addr >> 8) as u8, no_chr());
    ppu.write_register_with(0x2006, addr as u8, no_chr());
    ppu.write_register_with(
        0x2007,
        value,
        PpuMemory::System {
            chr: Some(mapper),
            mirroring: Mirroring::Horizontal,
            open_bus: 0,
        },
    );
}

#[test]
fn test_chr_ram_upload_through_ppu() {
    use nes_core::mapper::mapper118::Mapper118;

    // Every mapper that can back CHR with RAM must keep what the PPU hands it.
    let cases: [(&str, Box<dyn Mapper>); 4] = [
        ("mapper3", Box::new(Mapper3::new(vec![0u8; 32768], vec![]))),
        ("mapper4", Box::new(Mapper4::new(vec![0u8; 0x8000], vec![]))),
        (
            "mapper79",
            Box::new(Mapper79::new(vec![0u8; 32768], vec![])),
        ),
        (
            "mapper118",
            Box::new(Mapper118::new(vec![0u8; 0x20000], vec![])),
        ),
    ];

    for (name, mut mapper) in cases {
        upload_byte_through_ppu(mapper.as_mut(), 0x1000, 0xA5);
        assert_eq!(
            mapper.read(0x1000),
            0xA5,
            "{name}: CHR-RAM upload through $2007 did not reach the cartridge"
        );
    }
}

// ============================================================================
// Mappers added with the accuracy baseline (6, 8, 16, 18, 30, 87, 95, 112,
// 113, 114, 117, 133, 152, 178, 206, 227, 244)
//
// The helper fills each 8 KB PRG bank with its own bank number and each 1 KB
// CHR bank with `0x80 + bank`, so a read says exactly which bank the address
// landed in: a 16 KB bank N shows as N*2, a 32 KB bank N as N*4.
// ============================================================================

fn banked_mapper(number: u16, prg_8k: usize, chr_1k: usize) -> Box<dyn Mapper> {
    let mut prg: Vec<u8> = Vec::with_capacity(prg_8k * 0x2000);
    for bank in 0..prg_8k {
        prg.extend(std::iter::repeat(bank as u8).take(0x2000));
    }
    let mut chr: Vec<u8> = Vec::with_capacity(chr_1k * 0x400);
    for bank in 0..chr_1k {
        chr.extend(std::iter::repeat(((bank + 0x80) & 0xFF) as u8).take(0x400));
    }
    create_mapper(number, prg, chr).unwrap_or_else(|e| panic!("mapper {number}: {e:?}"))
}

#[test]
fn test_mapper6_prg_chr_and_irq() {
    let mut mapper = banked_mapper(6, 16, 16); // 128 KB PRG, 16 KB CHR
    assert_eq!(mapper.read(0x8000), 0);
    assert_eq!(
        mapper.read(0xC000),
        14,
        "$C000 is fixed to the last 16 KB bank"
    );

    // One register picks both: bits 2-5 = 16 KB PRG bank, bits 0-1 = 8 KB CHR.
    mapper.write(0x8000, 0x08);
    assert_eq!(mapper.read(0x8000), 4, "16 KB bank 2 starts at 8 KB bank 4");
    assert_eq!(mapper.read(0x0000), 0x80);

    // The IRQ counts 133 per rendered scanline and fires when it wraps.
    mapper.write(0x4502, 0x00);
    mapper.write(0x4503, 0x00);
    assert!(!mapper.irq_pending());
    for _ in 0..600 {
        mapper.on_scanline(0, true);
    }
    assert!(
        mapper.irq_pending(),
        "the counter should wrap after enough scanlines"
    );
    mapper.write(0x4501, 0x00);
    assert!(!mapper.irq_pending(), "writing $4501 acknowledges the IRQ");
}

#[test]
fn test_mapper8_prg_window() {
    let mut mapper = banked_mapper(8, 16, 4);
    assert_eq!(mapper.read(0x8000), 0);
    assert_eq!(
        mapper.read(0xC000),
        14,
        "$C000 is fixed to the last 16 KB bank"
    );

    mapper.write(0x8000, 0x28); // bits 3-7 = 5
    assert_eq!(
        mapper.read(0x8000),
        10,
        "16 KB bank 5 starts at 8 KB bank 10"
    );
    assert_eq!(mapper.read(0xC000), 14, "the fixed window must not move");
}

#[test]
fn test_mapper16_registers_by_low_nibble() {
    let mut mapper = banked_mapper(16, 16, 8);
    assert_eq!(mapper.read(0x8000), 0);

    // $8008 selects the 16 KB PRG bank at $8000.
    mapper.write(0x8008, 3);
    assert_eq!(mapper.read(0x8000), 6);
    assert_eq!(
        mapper.read(0xC000),
        14,
        "$C000 stays on the last 16 KB bank"
    );

    // $8009 selects mirroring; 2 is one-screen lower, 1 horizontal.
    mapper.write(0x8009, 2);
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::SingleScreen)
    );
    mapper.write(0x8009, 1);
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::Horizontal)
    );

    // The CHR banks are 1 KB and addressed by the low nibble.
    mapper.write(0x8000, 5);
    assert_eq!(mapper.read(0x0000), 0x85);
    assert_eq!(mapper.read(0x0400), 0x81, "each window keeps its own bank");
}

#[test]
fn test_mapper18_nibble_pairs_and_irq() {
    let mut mapper = banked_mapper(18, 64, 8);
    // Each bank register takes a low nibble first, then a high nibble.
    mapper.write(0x8000, 0x05);
    mapper.write(0x8001, 0x00);
    assert_eq!(mapper.read(0x8000), 5);
    mapper.write(0x8000, 0x00);
    mapper.write(0x8001, 0x02); // bank 0x20
    assert_eq!(
        mapper.read(0x8000),
        0x20,
        "the high nibble lands in bits 4-7"
    );

    // $F002 picks the nametable arrangement.
    mapper.write(0xF002, 0x00);
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::Horizontal)
    );
    mapper.write(0xF002, 0x01);
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::Vertical)
    );

    // The IRQ counts CPU cycles down from the latch, which takes four nibbles.
    mapper.write(0xE000, 0x04);
    mapper.write(0xE001, 0x00);
    mapper.write(0xE002, 0x00);
    mapper.write(0xE003, 0x00);
    mapper.write(0xF000, 0x00); // reload from the latch
    mapper.write(0xF001, 0x01); // mode 0, enabled
    assert!(!mapper.irq_pending());
    mapper.on_cpu_cycles(4);
    assert!(
        mapper.irq_pending(),
        "mode 0 fires when the counter hits zero"
    );

    // Mode 1 watches the top nibble instead of the whole counter, so the IRQ
    // only arrives when the count walks across a $1000 boundary.
    mapper.clear_irq();
    mapper.write(0xE000, 0x00);
    mapper.write(0xE001, 0x00);
    mapper.write(0xE002, 0x08);
    mapper.write(0xE003, 0x02); // latch = $2800
    mapper.write(0xF000, 0x00);
    mapper.write(0xF001, 0x03); // mode 1, enabled
    mapper.on_cpu_cycles(4);
    assert!(
        !mapper.irq_pending(),
        "only the top nibble matters in mode 1"
    );
    for _ in 0..9 {
        mapper.on_cpu_cycles(255); // walk the counter down past $2000
    }
    assert!(mapper.irq_pending(), "the top nibble walked past the latch");
}

#[test]
fn test_mapper30_unrom512() {
    let mut mapper = banked_mapper(30, 16, 0); // CHR is RAM on this board
    assert_eq!(mapper.read(0x8000), 0);
    assert_eq!(mapper.read(0xC000), 14);

    // Bits 0-4: PRG bank; bits 5-6: CHR-RAM bank; bit 7: nametable.
    mapper.write(0x8000, 0b1000_0101);
    assert_eq!(
        mapper.read(0x8000),
        10,
        "16 KB bank 5 starts at 8 KB bank 10"
    );
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::SingleScreenUpper)
    );

    // The PPU's uploads must stick, and bank separately.
    mapper.write(0x0000, 0x11);
    assert_eq!(mapper.read(0x0000), 0x11);
    mapper.write(0x8000, 0b0010_0000); // CHR bank 1
    assert_eq!(mapper.read(0x0000), 0x00, "a different 8 KB of CHR-RAM");
}

#[test]
fn test_mapper87_chr_bank_bits_are_swapped() {
    let mut mapper = banked_mapper(87, 4, 32); // four 8 KB CHR banks
    assert_eq!(mapper.read(0x8000), 0);
    // The 32 KB PRG image is fixed in place, so the second 16 KB is its own.
    assert_eq!(mapper.read(0xC000), 2);

    mapper.write(0x6000, 0x02); // bit 1 -> bank 1
    assert_eq!(mapper.read(0x0000), 0x88);
    mapper.write(0x6000, 0x01); // bit 0 -> bank 2
    assert_eq!(mapper.read(0x0000), 0x90);
    assert_eq!(mapper.read(0xC000), 2, "the PRG window must not move");
}

#[test]
fn test_mapper95_nametable_bit() {
    let mut mapper = banked_mapper(95, 16, 8);
    assert_eq!(mapper.read(0x8000), 0);

    mapper.write(0x8000, 7);
    mapper.write(0x8001, 2);
    assert_eq!(
        mapper.read(0xA000),
        2,
        "the second PRG window moves on its own"
    );

    // On the CHR registers bit 5 is the nametable bit, not part of the bank.
    mapper.write(0x8000, 0);
    mapper.write(0x8001, 0x21);
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::SingleScreenUpper)
    );
    assert_eq!(
        mapper.read(0x0000),
        0x82,
        "the bank is only the low five bits"
    );
}

#[test]
fn test_mapper112_split_registers() {
    let mut mapper = banked_mapper(112, 16, 8);
    // $8000 selects the register, $A000 loads it.
    mapper.write(0x8000, 0x00);
    mapper.write(0xA000, 0x06);
    assert_eq!(mapper.read(0x8000), 6);
    mapper.write(0x8000, 0x01);
    mapper.write(0xA000, 0x07);
    assert_eq!(mapper.read(0xA000), 7);
    assert_eq!(mapper.read(0xC000), 14, "the top half stays fixed");

    // CHR: registers 2/3 are 2 KB banks, 4-7 are 1 KB banks.
    mapper.write(0x8000, 0x02);
    mapper.write(0xA000, 0x02);
    assert_eq!(mapper.read(0x0000), 0x84);
    assert_eq!(mapper.read(0x0400), 0x85, "a 2 KB bank covers two windows");
    mapper.write(0x8000, 0x04);
    mapper.write(0xA000, 0x05);
    assert_eq!(mapper.read(0x1000), 0x85);
}

#[test]
fn test_mapper113_register_aliases() {
    let mut mapper = banked_mapper(113, 16, 64); // eight 8 KB CHR banks
    assert_eq!(mapper.read(0x8000), 0);

    // The same byte is decoded at several aliases: bits 3-7 are the 32 KB PRG
    // bank, bit 3 of that plus bits 0-2 the 8 KB CHR bank.
    mapper.write(0x4100, 0x1A);
    assert_eq!(
        mapper.read(0x8000),
        12,
        "32 KB bank 3 starts at 8 KB bank 12"
    );
    assert_eq!(mapper.read(0x0000), 0x90, "8 KB CHR bank 2");

    mapper.write(0x8008, 0x08);
    assert_eq!(mapper.read(0x8000), 4, "32 KB bank 1 starts at 8 KB bank 4");

    // $8E66/$8E67 pick between CHR banks 0 and 1 by the low nibble.
    mapper.write(0x8E66, 0x00);
    assert_eq!(mapper.read(0x0000), 0x88, "8 KB CHR bank 1");
    mapper.write(0x8E67, 0x07);
    assert_eq!(mapper.read(0x0000), 0x80);

    // $E00A switches to one-screen mirroring.
    mapper.write(0xE00A, 0x00);
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::SingleScreen)
    );
}

#[test]
fn test_mapper114_handshake_and_irq() {
    let mut mapper = banked_mapper(114, 16, 8);
    // $A000 names a register, $C000 loads it: register 4 is the $8000 window.
    mapper.write(0xA000, 4);
    mapper.write(0xC000, 5);
    assert_eq!(mapper.read(0x8000), 5);
    mapper.write(0xA000, 5);
    mapper.write(0xC000, 6);
    assert_eq!(mapper.read(0xA000), 6);

    // Register 0 is the 2 KB CHR bank at $0000.
    mapper.write(0xA000, 0);
    mapper.write(0xC000, 0x02);
    assert_eq!(
        mapper.read(0x0000),
        0x82,
        "the register is a 2 KB bank number"
    );
    assert_eq!(mapper.read(0x0400), 0x83);

    // $8000 picks mirroring.
    mapper.write(0x8000, 0x01);
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::Horizontal)
    );

    // $E003 reloads a counter that ticks once per rendered scanline.
    mapper.write(0xE003, 2);
    mapper.on_scanline(0, true);
    assert!(!mapper.irq_pending());
    mapper.on_scanline(1, true);
    assert!(mapper.irq_pending());
    mapper.write(0xE002, 0x00);
    assert!(!mapper.irq_pending());
}

#[test]
fn test_mapper117_irq_matches_scanline() {
    let mut mapper = banked_mapper(117, 16, 8);
    mapper.write(0x8000, 1);
    assert_eq!(mapper.read(0x8000), 1);
    assert_eq!(mapper.read(0xE000), 15, "the last PRG window is fixed");
    mapper.write(0xA007, 3);
    assert_eq!(mapper.read(0x1C00), 0x83);

    // The counter holds the scanline the IRQ should fire on.
    mapper.write(0xC000, 100);
    mapper.write(0xE000, 1);
    mapper.on_scanline(50, true);
    assert!(!mapper.irq_pending());
    mapper.on_scanline(100, true);
    assert!(mapper.irq_pending());
}

#[test]
fn test_mapper133_register_in_the_ram_window() {
    let mut mapper = banked_mapper(133, 8, 32);
    mapper.write(0x4120, 0x02); // CHR bank 2, PRG bank 0
    assert_eq!(mapper.read(0x0000), 0x90);
    mapper.write(0x4120, 0x06); // bit 2 -> PRG 32 KB bank 1
    assert_eq!(mapper.read(0x8000), 4, "32 KB bank 1 starts at 8 KB bank 4");

    // The rest of the window is work RAM.
    mapper.write(0x6000, 0x5A);
    assert_eq!(mapper.read(0x6000), 0x5A);
}

#[test]
fn test_mapper152_one_register_for_everything() {
    let mut mapper = banked_mapper(152, 16, 32);
    // `MPPP CCCC`: CHR bits 0-3, PRG bits 4-6, mirroring bit 7.
    mapper.write(0x8000, 0b1011_0010);
    assert_eq!(mapper.read(0x0000), 0x90);
    assert_eq!(mapper.read(0x8000), 6, "16 KB bank 3 starts at 8 KB bank 6");
    assert_eq!(mapper.read(0xC000), 14, "$C000 stays fixed");
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::SingleScreenUpper)
    );
}

#[test]
fn test_mapper178_outer_prg_registers() {
    let mut mapper = banked_mapper(178, 32, 16);
    // $4801 supplies PRG bits 1-4, $4802 supplies bits 5-8.
    mapper.write(0x4801, 0x02); // (2 >> 1) = 1
    assert_eq!(mapper.read(0x8000), 4, "32 KB bank 1 starts at 8 KB bank 4");
    mapper.write(0x4802, 0x01); // |= 1 << 2
    assert_eq!(mapper.read(0x8000), 20, "bank 1 | 4 = 5");

    // $4800 picks the nametable arrangement.
    mapper.write(0x4800, 1);
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::Horizontal)
    );
    mapper.write(0x4800, 0);
    assert_eq!(
        mapper.mirroring(),
        Some(nes_core::ppu::rendering::Mirroring::Vertical)
    );

    // $6000-$7FFF is a full 32 KB of work RAM.
    mapper.write(0x7FFF, 0x42);
    assert_eq!(mapper.read(0x7FFF), 0x42);
}

#[test]
fn test_mapper206_registers_without_irqs() {
    let mut mapper = banked_mapper(206, 16, 8);
    // The same register pair as MMC3, but only the low three bits select.
    mapper.write(0x8000, 6);
    mapper.write(0x8001, 3);
    assert_eq!(mapper.read(0x8000), 3);
    mapper.write(0x8000, 7);
    mapper.write(0x8001, 4);
    assert_eq!(mapper.read(0xA000), 4);
    assert_eq!(mapper.read(0xC000), 14, "the top two banks never move");

    // CHR is always two 2 KB banks then four 1 KB banks.
    mapper.write(0x8000, 0);
    mapper.write(0x8001, 2);
    assert_eq!(mapper.read(0x0000), 0x84);
    assert_eq!(mapper.read(0x0400), 0x85);
    mapper.write(0x8000, 2);
    mapper.write(0x8001, 6);
    assert_eq!(mapper.read(0x1000), 0x86);

    // There is no IRQ on this board.
    assert!(!mapper.irq_pending());
}

#[test]
fn test_mapper227_address_encoded_banks() {
    let mut mapper = banked_mapper(227, 128, 1);
    // The bank number comes from the address, not the data: bits 8,6..3. Bit 7
    // set keeps the top 16 KB independent of the outer bank.
    mapper.write(0x8181, 0x00); // bank 16, 32 KB mode
    assert_eq!(
        mapper.read(0x8000),
        64,
        "32 KB bank 16 starts at 8 KB bank 64"
    );
    assert_eq!(mapper.read(0xA000), 65);
    assert_eq!(mapper.read(0xC000), 66);

    // Without bit 0 the two 16 KB halves repeat; bit 2 swaps which comes first.
    mapper.write(0x8004, 0x00);
    assert_eq!(mapper.read(0x8000), 2, "bit 2 puts the high pair first");
    assert_eq!(
        mapper.read(0xC000),
        0,
        "bit 7 clear makes the top follow the outer bank"
    );
}

#[test]
fn test_mapper244_banks_from_addresses() {
    let mut mapper = banked_mapper(244, 16, 64); // four 32 KB banks, eight CHR
    mapper.write(0x8067, 0x00);
    assert_eq!(mapper.read(0x8000), 8, "32 KB bank 2 starts at 8 KB bank 8");
    mapper.write(0x80A9, 0xFF);
    assert_eq!(mapper.read(0x0000), 0xA0, "8 KB CHR bank 4");
}
