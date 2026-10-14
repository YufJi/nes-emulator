//! Contracts the host layer is built on.
//!
//! These are not emulation behaviour but promises a front end relies on. A
//! native app owns the console on a dedicated thread so the UI thread and the
//! audio callback stay free, and that only works while the types stay `Send`.
//! Sharing `&Nes` between threads is deliberately *not* supported: the machine
//! has a single owner, and the host moves audio across the boundary itself
//! (drain with `fill_audio` on the emulation thread, hand it to the audio
//! thread through the host's own queue).

use nes_core::{FrameClock, Nes, NesError};

#[test]
fn the_console_can_be_moved_to_another_thread() {
    fn assert_send<T: Send>() {}
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send::<Nes>();
    assert_send::<FrameClock>();
    assert_send_sync::<NesError>();
}

#[test]
fn a_console_really_runs_on_another_thread() {
    // Owning the machine on a worker thread is the shape every native front end
    // wants, so exercise it for real rather than only at the type level.
    let frames = std::thread::spawn(|| {
        let mut nes = Nes::new();
        nes.frame();
        nes.frame_number()
    })
    .join()
    .expect("the emulation thread must not panic");

    assert_eq!(frames, 1, "frame() should complete exactly one frame");
}

/// A cartridge that runs into a KIL opcode hangs the CPU, and the host survives.
///
/// The twelve KIL/JAM opcodes are what a corrupted ROM or a jump into data
/// lands on. The hardware stops fetching there, so the core does too: frames
/// still complete and only an interrupt or a reset restarts the CPU. This used
/// to be a `panic!`, which on `wasm32` aborts the whole instance — the page
/// then had to throw the emulator away (see `docs/architecture.md` §15.1).
#[test]
fn a_kil_opcode_halts_the_cpu_without_taking_the_host_down() {
    let mut rom = vec![0u8; 16 + 16 * 1024];
    rom[0..4].copy_from_slice(b"NES\x1a");
    rom[4] = 1; // one PRG bank
    rom[5] = 0; // CHR-RAM
    for byte in rom[16..].iter_mut() {
        *byte = 0x02; // KIL/JAM
    }
    rom[16 + 0x3FFC] = 0x00;
    rom[16 + 0x3FFD] = 0x80; // RESET -> $8000

    let mut nes = Nes::new();
    nes.load_rom(&rom).expect("the header itself is valid");

    // The frame completes and the machine reports the hang instead of dying.
    nes.frame();
    assert_eq!(nes.frame_number(), 1);
    assert!(nes.is_cpu_halted());

    // A reset releases the halt and starts over from the vector — which is
    // another KIL in this cartridge, so the hang simply happens again.
    nes.reset();
    assert!(!nes.is_cpu_halted(), "reset releases the halt");
    nes.frame();
    assert!(nes.is_cpu_halted());
}
