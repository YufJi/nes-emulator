//! Helpers shared by the suites that run real test ROMs.
//!
//! ROMs are never committed (see `AGENTS.md`), so every test using these
//! helpers *skips* when the file it needs is absent: `cargo test --workspace`
//! stays green on a fresh clone and turns into a real accuracy check as soon as
//! the ROMs are dropped into `roms/` (the expected layout is in
//! `docs/architecture.md` §13).

use std::path::{Path, PathBuf};

use nes_core::nes::BUTTON_START;
use nes_core::Nes;

/// Path of a ROM under `roms/`, or `None` when it is not present locally.
///
/// `relative` is relative to `roms/`, e.g. `blargg/instr_test-v5/rom_singles/01-basics.nes`.
pub fn rom(relative: &str) -> Option<PathBuf> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    // CARGO_MANIFEST_DIR is <repo>/crates/nes-core; climb back to <repo>.
    let mut path = PathBuf::from(manifest_dir);
    path.pop();
    path.pop();
    path.push("roms");
    path.push(relative);
    path.exists().then_some(path)
}

/// Every `.nes` file in a `roms/` directory, sorted so a run is reproducible.
///
/// Returns an empty list when the directory is absent, which is how the suites
/// skip themselves on a checkout without ROMs.
pub fn rom_directory(relative: &str) -> Vec<PathBuf> {
    let Some(dir) = rom(relative) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "nes"))
        .collect();
    found.sort();
    found
}

/// A console with `path` loaded.
pub fn console(path: &Path) -> Nes {
    let data =
        std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut nes = Nes::new();
    nes.load_rom(&data)
        .unwrap_or_else(|e| panic!("{} was rejected by the core: {e:?}", path.display()));
    nes
}

/// What a blargg-style test ROM reported.
///
/// The protocol (used by every suite in `roms/blargg/`): `$6000` holds `0x80`
/// while the test runs and the result code afterwards — `0` means it passed —
/// and a null-terminated ASCII report follows at `$6004`, ending in `Passed` or
/// `Failed #n`.
#[derive(Debug)]
pub struct BlarggReport {
    /// Result code from `$6000`; `0` is a pass.
    pub status: u8,
    /// Report text from `$6004` (`Failed #3` and friends).
    pub text: String,
    /// Frames emulated before the test finished.
    pub frames: u32,
    /// Set when the test never finished inside the frame budget.
    pub timed_out: bool,
}

impl BlarggReport {
    pub fn passed(&self) -> bool {
        !self.timed_out && self.status == 0
    }
}

/// Run a blargg test ROM until it reports a result.
pub fn run_blargg(nes: &mut Nes, max_frames: u32) -> BlarggReport {
    const STATUS: u16 = 0x6000;
    const TEXT: u16 = 0x6004;
    const RUNNING: u8 = 0x80;

    // Every suite boots the same way: a few frames of setup before it starts
    // reporting, then `status` flips away from `0x80` when it is done.
    let mut frames = 0;
    while frames < max_frames {
        nes.frame();
        frames += 1;
        if frames > 2 && nes.peek(STATUS) != RUNNING {
            break;
        }
    }

    let status = nes.peek(STATUS);
    let timed_out = status == RUNNING || frames >= max_frames;

    let mut text = String::new();
    for offset in 0..512u16 {
        let byte = nes.peek(TEXT + offset);
        if byte == 0 {
            break;
        }
        text.push(byte as char);
    }

    BlarggReport {
        status,
        text,
        frames,
        timed_out,
    }
}

/// Run every ROM in a `roms/` directory as a blargg-style suite.
///
/// `expected_failures` is the baseline: the ROMs this suite is known not to
/// pass yet. Anything that fails and is not on the list is a regression, and
/// anything on the list that passes is an improvement the list should lose —
/// both are reported, so the baseline cannot drift unnoticed in either
/// direction. That is what keeps `cargo test --workspace` green while the
/// accuracy work is still in progress.
pub fn check_blargg_directory(relative: &str, max_frames: u32, expected_failures: &[&str]) {
    let roms = rom_directory(relative);
    if roms.is_empty() {
        println!("skipped: no ROMs in roms/{relative}");
        return;
    }

    let mut regressions = Vec::new();
    let mut fixed = Vec::new();
    for path in &roms {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut nes = console(path);
        let report = run_blargg(&mut nes, max_frames);
        let expected = expected_failures.contains(&name.as_str());
        let passing = report.passed();

        if passing && expected {
            println!("FIXED: {name} now passes — drop it from the baseline");
            fixed.push(name);
        } else if passing {
            println!("pass: {name} ({} frames)", report.frames);
        } else if expected {
            println!("known: {name} -> {}", describe(&report));
        } else {
            println!("FAIL: {name} -> {report:?}");
            regressions.push(format!("{name}: {}", describe(&report)));
        }
    }

    assert!(
        regressions.is_empty() && fixed.is_empty(),
        "roms/{relative} no longer matches its baseline:\n{}\n{}",
        if regressions.is_empty() {
            String::new()
        } else {
            format!(
                "{} ROMs failed that the baseline expects to pass:\n  {}",
                regressions.len(),
                regressions.join("\n  ")
            )
        },
        if fixed.is_empty() {
            String::new()
        } else {
            format!(
                "{} baseline entries now pass, so the list is stale:\n  {}",
                fixed.len(),
                fixed.join("\n  ")
            )
        },
    );
    println!(
        "roms/{relative}: {} ROMs, {} passing, {} on the baseline",
        roms.len(),
        roms.len() - expected_failures.len(),
        expected_failures.len()
    );
}

/// One readable line for a failing report.
pub fn describe(report: &BlarggReport) -> String {
    if report.timed_out {
        return format!(
            "timed out after {} frames (status {:#04X})",
            report.frames, report.status
        );
    }
    let text = report.text.trim();
    if text.is_empty() {
        format!("status {:#04X}", report.status)
    } else {
        text.replace('\n', " | ")
    }
}

/// What AccuracyCoin reported, read out of the cartridge's result table.
///
/// The ROM walks its own result table the same way this does: one byte per
/// test, `$FF` for "marked to skip", everything else judged by bit 0 — set
/// means pass, clear means the value is a failure's error code. Results are
/// written to page `$0400` (`$0300` is the ROM's own scratch page for tests it
/// auto-skips), so that is the page read here.
///
/// The ROM's zero-page counters are deliberately *not* used for the verdict:
/// they are reset per redraw of the results screen, so their values depend on
/// how long the ROM has been sitting on that screen. `tests_run` is still read,
/// but only as a liveness check that the suite got going — the harness asserts
/// it is in the right ballpark rather than a specific number, and the callers
/// give the ROM a generous frame budget so a slow but healthy run is not
/// mistaken for a broken one.
#[derive(Debug, Default)]
pub struct AccuracyCoinReport {
    /// Tests the ROM says it executed (`$37`, its own counter).
    pub tests_run: u8,
    /// Result bytes with bit 0 set.
    pub passed: usize,
    /// Result bytes of `$FF` (marked to skip).
    pub skipped: usize,
    /// `(address, value)` for each recorded failure, sorted by address.
    pub failures: Vec<(u16, u8)>,
}

/// Boot AccuracyCoin, run every test, and read the result table.
///
/// The ROM is menu-driven, so this presses Start — which runs the whole suite
/// from the initial cursor position — and then waits for it to finish.
pub fn run_accuracy_coin(max_frames: u32) -> AccuracyCoinReport {
    const MENU_FRAMES: u32 = 90;
    const TESTS_RUN: u16 = 0x37;
    const RESULTS: std::ops::Range<u16> = 0x0400..0x0500;

    let Some(path) = rom("accuracycoin/AccuracyCoin.nes") else {
        return AccuracyCoinReport::default();
    };
    let mut nes = console(&path);

    // Let the menu come up, then press and release Start.
    for _ in 0..MENU_FRAMES {
        nes.frame();
    }
    nes.set_button(1, BUTTON_START, true);
    nes.frame();
    nes.frame();
    nes.set_button(1, BUTTON_START, false);
    for _ in 0..max_frames {
        nes.frame();
    }

    let mut passed = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();
    for addr in RESULTS {
        match nes.peek(addr) {
            0 => {}
            0xFF => skipped += 1,
            value if value & 1 == 1 => passed += 1,
            value => failures.push((addr, value)),
        }
    }

    AccuracyCoinReport {
        tests_run: nes.peek(TESTS_RUN),
        passed,
        skipped,
        failures,
    }
}

/// One line for the test output.
impl AccuracyCoinReport {
    pub fn summary(&self) -> String {
        format!(
            "{} tests ran, {} passed, {} skipped, {} failing",
            self.tests_run,
            self.passed,
            self.skipped,
            self.failures.len()
        )
    }
}
