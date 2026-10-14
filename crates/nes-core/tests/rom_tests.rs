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
        // The instruction-level ordering is right (CLI's delayed inhibition,
        // taken branches skipping their poll); what these need is the interrupt
        // to be vectored at an exact CPU cycle, which needs a cycle-stepped CPU
        // (§15.1).
        &[
            "2-nmi_and_brk.nes",
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
        // VBlank set/clear timing, NMI suppression and the odd-frame skip are
        // all still approximate (§15.1).
        &[
            "05-nmi_timing.nes",
            "06-suppression.nes",
            "07-nmi_on_timing.nes",
            "08-nmi_off_timing.nes",
            "10-even_odd_timing.nes",
        ],
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
    common::check_blargg_directory(
        "blargg/ppu_open_bus",
        BLARGG_FRAMES,
        // The PPU data bus latch does not decay over time yet.
        &["ppu_open_bus.nes"],
    );
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
        // The A12 edge filter and the dot-exact timing of the first rise are
        // not exact enough yet, and the "alternate" IRQ revision this board
        // variant uses needs a submapper this project does not parse (§15.1).
        &["4-scanline_timing.nes", "6-MMC3_alt.nes"],
    );
}

#[test]
fn blargg_apu() {
    common::check_blargg_directory(
        "blargg/apu_test/rom_singles",
        BLARGG_SLOW_FRAMES,
        // Frame-counter and DMC timing (jitter, length timing, IRQ flag
        // timing, DMC rates) are approximations.
        &[
            "4-jitter.nes",
            "5-len_timing.nes",
            "6-irq_flag_timing.nes",
            "7-dmc_basics.nes",
            "8-dmc_rates.nes",
        ],
    );
}

/// AccuracyCoin's baseline: the suite runs, and 50 of its tests fail.
///
/// The failure count is a ratchet in both directions — a *higher* one is a
/// regression, a *lower* one means the baseline needs updating — and it doubles
/// as the liveness check that matters for a menu-driven ROM: if the harness
/// stops booting the menu or pressing Start, the ROM stops reporting results
/// and this fails loudly instead of quietly measuring nothing.
#[test]
fn accuracy_coin_matches_its_baseline() {
    /// Tests AccuracyCoin reports as failing on this checkout.
    const BASELINE_FAILURES: usize = 50;
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
