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
/// The suites that measure a DMC fetch against a CPU read: their own
/// synchronisation loop runs for a couple of thousand frames before the verdict
/// is on screen or in `$6000`.
const DMC_FRAMES: u32 = 1800;

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
    // These print `PASSED` and write nothing to `$6000`, so their verdict is
    // read off the picture (2026-10-16: 3/3 `PASSED`) and the goldens pin it.
    common::check_blargg_directory("blargg/branch_timing_tests", BLARGG_FRAMES, &[]);
}

#[test]
fn blargg_cpu_interrupts() {
    // All five are green. The last three were one or two cycles each, and what
    // moved them was where the APU sits inside the CPU cycle: it clocks at the
    // *start* of one (Mesen's `ProcessCpuClock`), before the cycle's own access,
    // so a `$4017` restart's steps — the frame IRQ among them — land one cycle
    // after the access sees them rather than one before. The tables in
    // `3-nmi_and_irq`, `4-irq_and_dma` and `5-branch_delays_irq` are all read
    // off that one cycle.
    common::check_blargg_directory(
        "blargg/cpu_interrupts_v2/rom_singles",
        BLARGG_SLOW_FRAMES,
        &[],
    );
}

#[test]
fn blargg_ppu_vbl_nmi() {
    common::check_blargg_directory(
        "blargg/ppu_vbl_nmi/rom_singles",
        BLARGG_SLOW_FRAMES,
        // All ten are green: the VBlank flag's dot, the NMI line and the
        // `$2002` reads that race them are placed per dot, and `10-even_odd_timing`
        // moved once the odd-frame skip was decided on pre-render dot 339
        // (§7): it now prints the `08 08 09 07` its own comment asks for.
        &[],
    );
}

#[test]
fn blargg_ppu_tests() {
    common::check_blargg_directory(
        "blargg/blargg_ppu_tests_2005.09.15b",
        BLARGG_FRAMES,
        // These print a result code on screen — `$01` is "all tests passed" —
        // and write nothing to `$6000`. Read on 2026-10-16: `palette_ram`,
        // `sprite_ram`, `vbl_clear_time` and `vram_access` show `$01`;
        // `power_up_palette` shows `$02`, "palette differs from table", which is
        // the power-on content this core changed on purpose (`RamPowerOnState`
        // is `Random` in Mesen and all zeroes here, §4 of the alignment
        // document — it is what keeps the determinism invariant).
        &["power_up_palette.nes — prints $02: the power-on palette is all zeroes here by design"],
    );
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
    // The pixel-verdict suites run their whole frame budget, so it is kept to
    // what the ROMs need: their picture settles about thirty frames in.
    // Read off the pictures on 2026-10-16: all eleven print `PASSED`.
    common::check_blargg_directory("blargg/sprite_hit_tests_2005.10.05", BLARGG_FRAMES, &[]);
}

#[test]
fn blargg_sprite_overflow() {
    common::check_blargg_directory(
        "blargg/sprite_overflow_tests",
        BLARGG_FRAMES,
        // These print `FAILED: #n` and write nothing to `$6000`. The three
        // entries were read off their screens on 2026-10-16, and they correct
        // the 5/5 the alignment document used to record: that number came from
        // goldens recorded while the pictures already showed these failures.
        //
        // S1 of `docs/sprite-phase-plan.md` (2026-10-16) gated the walk on the
        // rendering state and took it off the pre-render line, and that is what
        // `1.Basics` (check 6, "must not be set when all rendering is off") and
        // `5.Emulator` (check 3, "disabling rendering did not recalculate the
        // flag time") were waiting for: both print `PASSED` now, and their
        // entries and goldens moved with them. `3.Timing` is left, and it is
        // S2's: its check 4 wants the flag to appear on the dot the walk judges
        // the ninth sprite on, where this core still raises it at dot 65.
        &["3.Timing.nes — prints FAILED: #4 (flag raised at dot 65, not the judging dot)"],
    );
}

#[test]
fn blargg_mmc3() {
    common::check_blargg_directory(
        "blargg/mmc3_test_2/rom_singles",
        BLARGG_SLOW_FRAMES,
        // What is left is the "alternate" IRQ revision `6-MMC3_alt`, which
        // wants MMC3 revision A. Mesen selects that from its game database
        // (`MMC3.h`), which has no entry for blargg's ROMs — so Mesen fails
        // this one too, and the honest entry is this one rather than a
        // per-ROM table with no oracle (§10.6). The A12 half is done: the
        // cartridge is handed the addresses the pipeline *really read*, on the
        // dot it read them, and that took `4-scanline_timing` green.
        &["6-MMC3_alt.nes — wants MMC3 revision A; Mesen fails it too (no database entry)"],
    );
}

#[test]
fn blargg_apu() {
    common::check_blargg_directory(
        "blargg/apu_test/rom_singles",
        BLARGG_SLOW_FRAMES,
        // The frame counter's phase is now blargg's own measured one — a
        // `$4017` restart is three cycles late (four on the other APU clock
        // phase) and the IRQ flag goes up for three cycles in a row — which is
        // what `4-jitter`, `5-len_timing` and `6-irq_flag_timing` measure.
        // The DMC's own two are green as well: the length counter moves at the
        // fetch, the fetched byte waits in the buffer until the output unit
        // takes it, and the timer's period is the table's value — which is what
        // `7-dmc_basics` check 19 wants of the one-byte buffer and what puts
        // `8-dmc_rates`'s status transition inside every rate's window.
        &[],
    );
}

// The suites below are the ones Mesen's own NES core names in its comments
// (`docs/mesen-alignment.md` §10.6): they are what its DMA, `$2007`-buffer and
// controller code was developed against. They live under `roms/nes-test-roms/`,
// the layout of the `christopherpow/nes-test-roms` collection.

#[test]
fn nesdev_dmc_dma_during_read4() {
    common::check_blargg_directory(
        "nes-test-roms/dmc_dma_during_read4",
        DMC_FRAMES,
        // Screen verdicts at 1800 frames (2026-10-16): `dma_2007_write` shows
        // "Passed"; `dma_4016_read` and `read_write_2007` show "Failed"; and
        // `dma_2007_read` / `double_2007_read` print a checksum instead of a
        // verdict — the values they accept are in §7, and ours is one cycle of
        // phase out (§9's C5 row).
        &[
            "dma_2007_read.nes — prints a checksum one cycle off the accepted values",
            "dma_4016_read.nes — prints Failed",
            "double_2007_read.nes — prints a checksum one cycle off the accepted values",
            "read_write_2007.nes — prints Failed",
        ],
    );
}

#[test]
fn nesdev_sprdma_and_dmc_dma() {
    common::check_blargg_directory(
        "nes-test-roms/sprdma_and_dmc_dma",
        DMC_FRAMES,
        // Both print their clock table and "Failed": the sprite/DMC overlap
        // phase is off by a cycle (§9's C5 row).
        &[
            "sprdma_and_dmc_dma.nes — prints Failed (overlap phase)",
            "sprdma_and_dmc_dma_512.nes — prints Failed (overlap phase)",
        ],
    );
}

#[test]
fn nesdev_dmc_tests() {
    common::check_blargg_directory(
        "nes-test-roms/dmc_tests",
        BLARGG_FRAMES,
        // None of the four ever writes `$6000` or reaches a verdict: each ends
        // up in a short loop waiting for DMC behaviour that is not modelled yet
        // (PC by frame 9000 is in the entry). They are listed with no goldens on
        // purpose — a picture of a hang is not a verdict, and pinning one would
        // claim a pass nobody measured.
        &[
            "buffer_retained.nes — waits forever on the DMC (PC $E149)",
            "latency.nes — waits forever on the DMC (PC $E162)",
            "status.nes — waits forever on the DMC (PC $E14E)",
            "status_irq.nes — waits forever on the DMC (PC $E154)",
        ],
    );
}

#[test]
fn nesdev_read_joy3() {
    common::check_blargg_directory(
        "nes-test-roms/read_joy3",
        BLARGG_FRAMES,
        // These ask the player to press buttons; with no scripted input they
        // sit on their prompt, so they are listed rather than left to burn a
        // slow-suite budget.
        &[
            "count_errors.nes — waits for scripted input",
            "count_errors_fast.nes — waits for scripted input",
            "test_buttons.nes — waits for scripted input",
            "thorough_test.nes — waits for scripted input",
        ],
    );
}

#[test]
fn nesdev_cpu_dummy_reads() {
    // Prints "Passed" on screen and writes nothing to `$6000`; 600 frames was
    // too short for it to get there (2026-10-16).
    common::check_blargg_directory("nes-test-roms/cpu_dummy_reads", DMC_FRAMES, &[]);
}

#[test]
fn nesdev_cpu_dummy_writes() {
    // Both report `$6000 = 0`; 600 frames was too short for either to finish.
    common::check_blargg_directory("nes-test-roms/cpu_dummy_writes", DMC_FRAMES, &[]);
}

#[test]
fn nesdev_cpu_exec_space() {
    common::check_blargg_directory(
        "nes-test-roms/cpu_exec_space",
        DMC_FRAMES,
        // `ppuio` reports `$6000 = 0`. `apu` stops at `$6000 = 2`, its own code
        // for the open-bus check on the APU/IO page (§10.3 item 4).
        &["test_cpu_exec_space_apu.nes — reports $6000 = 2 (the APU page's open bus)"],
    );
}

#[test]
fn nesdev_cpu_reset() {
    common::check_blargg_directory(
        "nes-test-roms/cpu_reset",
        BLARGG_FRAMES,
        // Both print the values they wanted — `registers` reads
        // `A X Y P S = 00 00 00 34 FD`, the expected power-on state — and then
        // ask the *player* to press reset, which this harness cannot do.
        &[
            "registers.nes — waits for a reset button",
            "ram_after_reset.nes — waits for a reset button",
        ],
    );
}

#[test]
fn nesdev_full_palette() {
    // No verdict text, and no still picture either: these three are palette
    // *animations* — `full_palette.s` ends in `jmp begin_frame` — so the
    // per-frame hashes in their goldens are the whole net. What they showed
    // when the register state machine landed (§7, C7) is that the sweep now
    // draws the palette grid the ROM is written to draw instead of flat colour
    // bars.
    common::check_blargg_directory("nes-test-roms/full_palette", BLARGG_FRAMES, &[]);
}

#[test]
fn nesdev_ppu_read_buffer() {
    common::check_blargg_directory("nes-test-roms/ppu_read_buffer", BLARGG_SLOW_FRAMES, &[]);
}

#[test]
fn blargg_apu_2005() {
    // The 2005 pack, which Mesen's own APU comments cite: `NesApu::Run`'s
    // ordering of the frame counter and the length counters exists for
    // `11.len_reload_timing`'s tests 4 and 5.
    //
    // They print a result code on screen — `$01` is "all tests passed" — and
    // write nothing to `$6000`. Read on 2026-10-16: nine show `$01`, and the
    // two below show the length counter's reload/halt timing, which is
    // §10.3 item 8.
    common::check_blargg_directory(
        "nes-test-roms/blargg_apu_2005.07.30",
        BLARGG_SLOW_FRAMES,
        &[
            "10.len_halt_timing.nes — prints $03 (length clocked while halted at 14915)",
            "11.len_reload_timing.nes — prints $04 (reload during the length clock at ctr 0)",
        ],
    );
}

/// AccuracyCoin's baseline: the suite runs, and 17 of its tests fail.
///
/// The failure count is a ratchet in both directions — a *higher* one is a
/// regression, a *lower* one means the baseline needs updating — and it doubles
/// as the liveness check that matters for a menu-driven ROM: if the harness
/// stops booting the menu or pressing Start, the ROM stops reporting results
/// and `MINIMUM_TESTS` fails loudly instead of measuring nothing. (The ROM
/// hanging before the menu is a real failure mode here: it happened when the
/// DMC's fetch was charged to the CPU twice, and the counter stayed at zero.)
#[test]
fn accuracy_coin_matches_its_baseline() {
    /// Tests AccuracyCoin reports as failing on this checkout.
    const BASELINE_FAILURES: usize = 17;
    /// The ROM runs its suite one test per frame; this is the floor that says
    /// it really got going (the counter itself is reset whenever the results
    /// screen redraws, so its final value is timing-dependent).
    const MINIMUM_TESTS: u8 = 100;

    // A generous budget: the ROM's own vblank waits get longer as emulation
    // gets more faithful, and a slow run must not look like a broken one.
    let Some(report) = common::run_accuracy_coin(3600) else {
        println!("skipped: no roms/accuracycoin/AccuracyCoin.nes");
        return;
    };

    println!("AccuracyCoin: {}", report.summary());
    for (addr, value) in &report.failures {
        println!("  fail at {addr:#06X} = {value:#04X}");
    }

    assert!(
        report.golden.is_none(),
        "the frames do not match their goldens"
    );
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

/// The hash function is part of the golden format: Mesen's `.mtp` tests store
/// `SHA1::GetHash(frame.FrameBuffer, frame.FrameBufferSize)` per frame, so ours
/// has to be SHA1 for the two sets to be comparable at all.
#[test]
fn test_sha1_matches_the_known_digests() {
    common::sha1::sha1_matches_the_known_digests();
}
