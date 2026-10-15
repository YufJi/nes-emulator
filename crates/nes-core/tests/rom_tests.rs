//! Real test-ROM suites: blargg's CPU/PPU/APU/MMC3 tests and AccuracyCoin.
//!
//! These are the accuracy baseline the docs point at (§13): each suite runs
//! every ROM it finds under its `roms/` directory and fails naming the file
//! that failed. On a checkout without ROMs they print a skip note instead —
//! ROMs are never committed, so the layout and the download sources live in
//! `docs/architecture.md`.
//!
//! Run with `--nocapture` to see the per-ROM results.

//! Each suite lists the ROMs it is known not to pass yet. Those are the
//! accuracy backlog; anything failing *outside* the list is a regression, and
//! anything on the list that starts passing demands the list be updated. See
//! `docs/architecture.md` §13.1 for the baseline this encodes.

mod common;

/// Frames a blargg ROM gets before it is considered hung. The suites are short
/// (most finish in well under a second of emulated time); the slow ones are the
/// interrupt-timing suites, which wait on real NMI/IRQ windows.
const BLARGG_FRAMES: u32 = 600;
/// The interrupt suites need room for their per-frame waits.
const BLARGG_SLOW_FRAMES: u32 = 3600;

#[test]
fn blargg_instr_test_v5() {
    common::check_blargg_directory("blargg/instr_test-v5/rom_singles", BLARGG_FRAMES, &[]);
}

#[test]
fn blargg_instr_timing() {
    common::check_blargg_directory("blargg/instr_timing/rom_singles", BLARGG_SLOW_FRAMES, &[]);
}

#[test]
fn blargg_branch_timing() {
    common::check_blargg_directory("blargg/branch_timing_tests", BLARGG_SLOW_FRAMES, &[]);
}

#[test]
fn blargg_cpu_interrupts() {
    common::check_blargg_directory(
        "blargg/cpu_interrupts_v2/rom_singles",
        BLARGG_SLOW_FRAMES,
        // The CPU now drives its own cycles the way Mesen's does, which took
        // `2-nmi_and_brk` green: BRK's vector is chosen at the status push, so
        // an NMI asserted before it takes the vector over. What is left of the
        // other three is one or two cycles each, and `4-irq_and_dma`'s offset
        // still tracks the frame counter's phase (§4.3).
        &[
            "3-nmi_and_irq.nes",
            "4-irq_and_dma.nes",
            "5-branch_delays_irq.nes",
        ],
    );
}

#[test]
fn blargg_ppu_vbl_nmi() {
    common::check_blargg_directory(
        "blargg/ppu_vbl_nmi/rom_singles",
        BLARGG_SLOW_FRAMES,
        // The VBlank flag's dot, the NMI line and the `$2002` reads that race
        // them are placed per dot now; what is left needs per-dot rendering -
        // the odd-frame skip, which is decided on pre-render dot 339 (§15.1).
        &["10-even_odd_timing.nes"],
    );
}

#[test]
fn blargg_ppu_tests() {
    common::check_blargg_directory("blargg/blargg_ppu_tests_2005.09.15b", BLARGG_FRAMES, &[]);
}

#[test]
fn blargg_oam_read() {
    common::check_blargg_directory("blargg/oam_read", BLARGG_FRAMES, &[]);
}

#[test]
fn blargg_oam_stress() {
    // Runs long by design: it re-writes OAM for the better part of a minute.
    common::check_blargg_directory("blargg/oam_stress", BLARGG_SLOW_FRAMES, &[]);
}

#[test]
fn blargg_ppu_open_bus() {
    common::check_blargg_directory("blargg/ppu_open_bus", BLARGG_FRAMES, &[]);
}

#[test]
fn blargg_sprite_hit() {
    common::check_blargg_directory(
        "blargg/sprite_hit_tests_2005.10.05",
        BLARGG_SLOW_FRAMES,
        &[],
    );
}

#[test]
fn blargg_sprite_overflow() {
    common::check_blargg_directory("blargg/sprite_overflow_tests", BLARGG_FRAMES, &[]);
}

#[test]
fn blargg_mmc3() {
    common::check_blargg_directory(
        "blargg/mmc3_test_2/rom_singles",
        BLARGG_SLOW_FRAMES,
        // What is left is the "alternate" IRQ revision `6-MMC3_alt`, which
        // needs a mapper-4 submapper this project does not parse (§15.1). The
        // A12 half is done: the cartridge is handed the addresses the pipeline
        // *really read*, on the dot it read them, and that took
        // `4-scanline_timing` green.
        &["6-MMC3_alt.nes"],
    );
}

#[test]
fn blargg_apu() {
    common::check_blargg_directory(
        "blargg/apu_test/rom_singles",
        BLARGG_SLOW_FRAMES,
        // The frame counter's phase is now Mesen's — a `$4017` restart is three
        // cycles late (four on the other APU clock phase) and the IRQ flag goes
        // up for three cycles in a row — which is what `4-jitter`,
        // `5-len_timing` and `6-irq_flag_timing` measure. What is left is the
        // DMC itself.
        &["7-dmc_basics.nes", "8-dmc_rates.nes"],
    );
}

/// AccuracyCoin's baseline: the suite runs, and 47 of its tests fail.
///
/// The failure count is a ratchet in both directions — a *higher* one is a
/// regression, a *lower* one means the baseline needs updating — and it doubles
/// as the liveness check that matters for a menu-driven ROM: if the harness
/// stops booting the menu or pressing Start, the ROM stops reporting results
/// and this fails loudly instead of quietly measuring nothing.
#[test]
fn accuracy_coin_matches_its_baseline() {
    /// Tests AccuracyCoin reports as failing on this checkout.
    const BASELINE_FAILURES: usize = 46;
    /// The ROM runs its suite one test per frame; this is the floor that says
    /// it really got going (the counter itself is reset whenever the results
    /// screen redraws, so its final value is timing-dependent).
    const MINIMUM_TESTS: u8 = 100;

    // A generous budget: the ROM's own vblank waits get longer as emulation
    // gets more faithful, and a slow run must not look like a broken one.
    let report = common::run_accuracy_coin(3600);
    if report.tests_run == 0 {
        println!("skipped: no roms/accuracycoin/AccuracyCoin.nes");
        return;
    }

    println!("AccuracyCoin: {}", report.summary());
    for (addr, value) in &report.failures {
        println!("  fail at {addr:#06X} = {value:#04X}");
    }

    assert!(
        report.tests_run >= MINIMUM_TESTS,
        "AccuracyCoin should run its whole suite: {}",
        report.summary()
    );
    assert_eq!(
        report.failures.len(),
        BASELINE_FAILURES,
        "AccuracyCoin's failing count moved; update the baseline and \
         docs/architecture.md §13.1: {}",
        report.summary()
    );
}
