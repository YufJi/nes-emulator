//! End-to-end smoke tests against the bundled Super Mario Bros ROM.
//!
//! These exercise the full pipeline (CPU interrupts, PPU background + sprite
//! rendering, mirroring, palette mirroring and the controller) by booting a
//! real game and checking that it reaches the title screen and reacts to input.

use nes_wasm::Nes;

const ROM_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../roms/超级玛莉.NES");

const START: u8 = 3;
const A: u8 = 0;
const RIGHT: u8 = 7;

fn load_smb() -> Option<Nes> {
    let data = std::fs::read(ROM_PATH).ok()?;
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("SMB ROM should load");
    Some(nes)
}

fn frame_stats(nes: &Nes) -> (usize, usize) {
    let fb = nes.get_frame_buffer();
    let distinct = fb.iter().collect::<std::collections::HashSet<_>>().len();
    let non_backdrop = fb.iter().filter(|p| **p != fb[0]).count();
    (distinct, non_backdrop)
}

#[test]
fn smb_boots_to_a_drawn_title_screen() {
    let Some(mut nes) = load_smb() else {
        eprintln!("Skipping: {} not found", ROM_PATH);
        return;
    };

    for _ in 0..240 {
        nes.frame();
    }

    // The title screen is not a flat fill: it has the HUD text, the
    // "1 PLAYER GAME" menu, the hill/bushes and the ground tiles.
    let (distinct, non_backdrop) = frame_stats(&nes);
    assert!(
        distinct >= 6,
        "title screen should use several colours, got {distinct}"
    );
    assert!(
        non_backdrop > 5_000,
        "title screen looks blank: only {non_backdrop} non-backdrop pixels"
    );

    // Background colour must be the game's sky blue, not the power-on black.
    assert_ne!(
        nes.get_frame_buffer()[0],
        0xFF000000,
        "backdrop should have been programmed by the game"
    );
}

#[test]
fn smb_reacts_to_the_start_button() {
    let Some(mut nes) = load_smb() else {
        eprintln!("Skipping: {} not found", ROM_PATH);
        return;
    };

    for _ in 0..240 {
        nes.frame();
    }
    let title = nes.get_frame_buffer();

    // Press START and confirm the frame actually changes.
    nes.button_down(1, START);
    for _ in 0..4 {
        nes.frame();
    }
    nes.button_up(1, START);

    let mut changed = false;
    for _ in 0..60 {
        nes.frame();
        if nes.get_frame_buffer() != title {
            changed = true;
            break;
        }
    }
    assert!(changed, "pressing START had no visible effect");
}

/// The game must reach World 1-1 and actually render it: sky, ground, pipes
/// and sprites. This is the end-to-end check that the PPU pipeline, the
/// nametable writes and sprite rendering all work together.
#[test]
fn smb_renders_world_1_1() {
    let Some(mut nes) = load_smb() else {
        eprintln!("Skipping: {} not found", ROM_PATH);
        return;
    };

    for _ in 0..240 {
        nes.frame();
    }
    nes.button_down(1, START);
    for _ in 0..4 {
        nes.frame();
    }
    nes.button_up(1, START);

    // Walk right for a while so the level scrolls and scenery appears.
    nes.button_down(1, RIGHT);
    let mut best = 0usize;
    for _ in 0..900 {
        nes.frame();
        let fb = nes.get_frame_buffer();
        best = best.max(fb.iter().collect::<std::collections::HashSet<_>>().len());
    }
    nes.button_up(1, RIGHT);

    assert!(
        best >= 8,
        "World 1-1 never rendered a full scene (best {best} colours)"
    );
}

/// The APU must actually produce audio rather than silence.
#[test]
fn smb_produces_audio() {
    let Some(mut nes) = load_smb() else {
        eprintln!("Skipping: {} not found", ROM_PATH);
        return;
    };

    for _ in 0..240 {
        nes.frame();
    }
    nes.button_down(1, START);
    for _ in 0..4 {
        nes.frame();
    }
    nes.button_up(1, START);

    let mut audible_frames = 0usize;
    let mut peak = 0.0f32;
    let mut period = vec![0.0f32; 8192];
    for _ in 0..600 {
        nes.frame();
        // Pull the way a device callback would, then look at what came back.
        let written = nes.fill_audio(&mut period);
        let samples = &period[..written];
        if samples.iter().any(|sample| sample.abs() > 1e-3) {
            audible_frames += 1;
        }
        for sample in samples {
            peak = peak.max(sample.abs());
        }
    }

    assert!(peak > 0.05, "APU produced no audible output (peak {peak})");
    assert!(
        audible_frames > 60,
        "only {audible_frames}/600 frames contained audio"
    );
}

#[test]
fn smb_save_state_rewinds_the_whole_machine() {
    let Some(mut nes) = load_smb() else {
        eprintln!("Skipping: {} not found", ROM_PATH);
        return;
    };

    // Let the game reach a live, animated screen before snapshotting.
    for _ in 0..240 {
        nes.frame();
    }
    let snapshot = nes.save_state();
    assert!(!snapshot.is_empty(), "save state should not be empty");

    for _ in 0..30 {
        nes.frame();
    }
    let expected = nes.get_frame_buffer();

    // Rewinding and replaying the same number of frames must reproduce the
    // exact picture: a strong check that CPU, PPU, APU and cartridge state all
    // round-trip.
    nes.load_state(&snapshot);
    for _ in 0..30 {
        nes.frame();
    }
    assert_eq!(
        nes.get_frame_buffer(),
        expected,
        "the rewound machine rendered a different frame"
    );
}
