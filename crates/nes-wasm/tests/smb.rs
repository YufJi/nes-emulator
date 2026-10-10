//! End-to-end smoke tests against the bundled Super Mario Bros ROM.
//!
//! These exercise the full pipeline (CPU interrupts, PPU background + sprite
//! rendering, mirroring, palette mirroring and the controller) by booting a
//! real game and checking that it reaches the title screen and reacts to input.

use nes_wasm::JsNes;

const ROM_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../roms/超级玛莉.NES");

const START: u8 = 3;
const A: u8 = 0;
const RIGHT: u8 = 7;

fn load_smb() -> Option<JsNes> {
    let data = std::fs::read(ROM_PATH).ok()?;
    let mut nes = JsNes::new();
    nes.load_rom(&data).expect("SMB ROM should load");
    Some(nes)
}

fn frame_stats(nes: &JsNes) -> (usize, usize) {
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
