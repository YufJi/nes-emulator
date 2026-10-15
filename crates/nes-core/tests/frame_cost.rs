//! Frame cost of the whole machine, in release, for before/after comparisons.
//!
//! Every accuracy phase is measured against the one before it, and the number
//! that matters is milliseconds per frame on a real cartridge — `benches/`
//! exercises the units directly rather than through `Nes::frame()`. This is a
//! measurement, not a check, so it is `#[ignore]`d and only asserts that the
//! ROM was there:
//!
//! ```text
//! cargo test -p nes-core --release --test frame_cost -- --ignored --nocapture
//! ```
//!
//! Run it a few times: single runs vary by a few percent, so compare medians.

use std::path::PathBuf;

use nes_core::Nes;

/// Frames measured after a warm-up, matching the numbers in
/// `docs/cycle-accuracy-plan.md`.
const FRAMES: u32 = 600;

#[test]
#[ignore = "a measurement, not a check"]
fn frame_cost() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("roms/nestest/nestest.nes");
    let data = std::fs::read(&path).expect("nestest ROM (see docs/architecture.md 13.1)");

    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load nestest");
    for _ in 0..60 {
        nes.frame();
    }

    let start = std::time::Instant::now();
    for _ in 0..FRAMES {
        nes.frame();
    }
    let elapsed = start.elapsed();
    println!(
        "{:.3} ms/frame ({FRAMES} frames of nestest in {:?})",
        elapsed.as_secs_f64() * 1000.0 / FRAMES as f64,
        elapsed
    );
}
