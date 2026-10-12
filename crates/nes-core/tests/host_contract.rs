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
