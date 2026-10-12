//! Host-agnostic frame pacing.
//!
//! Every front end drives the console from its own scheduler — `requestAnimationFrame`
//! on the web, `CADisplayLink` on iOS, `Choreographer` on Android, a display link
//! or a plain timer on the desktop — and every one of them needs the same three
//! things: turn wall-clock time into a whole number of frames, absorb a burst of
//! catch-up frames without spiralling, and report the achieved frame rate.
//!
//! [`FrameClock`] owns exactly that logic and nothing else: it has no notion of
//! where timestamps come from, holds no platform handle and allocates nothing,
//! so the same rules apply on every host.

use crate::Region;

/// Frames a single update may emulate before dropping the backlog.
const DEFAULT_MAX_CATCH_UP_FRAMES: u32 = 4;
/// Largest timestamp gap that is still simulated.
///
/// A suspended process (backgrounded tab, locked phone) can hand back a delta of
/// seconds; simulating it would fast-forward the game on the next frame.
const DEFAULT_MAX_DELTA_MS: f64 = 250.0;
/// Window over which the reported frame rate is averaged.
const DEFAULT_FPS_WINDOW_MS: f64 = 500.0;

/// Fixed-timestep pacer: turns host timestamps into whole frames.
///
/// ```no_run
/// # use nes_core::{FrameClock, Region};
/// let mut clock = FrameClock::for_region(Region::Ntsc);
/// // Called once per host tick with a monotonic timestamp in milliseconds.
/// let frames = clock.frames_due(1234.5);
/// for _ in 0..frames {
///     // nes.frame();
/// }
/// ```
///
/// Timestamps must be monotonic and in milliseconds. The first call after
/// construction (or [`FrameClock::reset`]) only seeds the clock and reports no
/// frames, because there is no gap to measure yet.
#[derive(Debug, Clone)]
pub struct FrameClock {
    frame_ms: f64,
    accumulator_ms: f64,
    max_catch_up_frames: u32,
    max_delta_ms: f64,
    fps_window_ms: f64,
    last_ms: Option<f64>,
    window_start_ms: Option<f64>,
    frames_in_window: u32,
    fps: f64,
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::for_region(Region::Ntsc)
    }
}

impl FrameClock {
    /// A clock pacing at an explicit frame rate in Hz.
    ///
    /// A rate that is not finite and positive falls back to the NTSC rate, so a
    /// bad host value cannot make the console run at zero or infinite speed.
    pub fn new(frame_rate: f64) -> Self {
        Self {
            frame_ms: 0.0,
            accumulator_ms: 0.0,
            max_catch_up_frames: DEFAULT_MAX_CATCH_UP_FRAMES,
            max_delta_ms: DEFAULT_MAX_DELTA_MS,
            fps_window_ms: DEFAULT_FPS_WINDOW_MS,
            last_ms: None,
            window_start_ms: None,
            frames_in_window: 0,
            fps: 0.0,
        }
        .with_frame_rate(frame_rate)
    }

    /// A clock pacing at the console's own rate for `region`.
    ///
    /// Cartridges can be NTSC or PAL (see [`crate::Nes::region`]), and the two
    /// really do run at different speeds, so hosts should rebuild the clock
    /// whenever the loaded cartridge changes.
    pub fn for_region(region: Region) -> Self {
        Self::new(region.fps())
    }

    /// Frames per second this clock paces at.
    pub fn frame_rate(&self) -> f64 {
        1000.0 / self.frame_ms
    }

    /// Change the pacing rate, dropping any partially accumulated frame so the
    /// next update starts from a clean boundary.
    pub fn set_frame_rate(&mut self, frame_rate: f64) -> &mut Self {
        let frame_rate = if frame_rate.is_finite() && frame_rate > 0.0 {
            frame_rate
        } else {
            Region::Ntsc.fps()
        };
        self.frame_ms = 1000.0 / frame_rate;
        self.accumulator_ms = 0.0;
        self
    }

    /// Builder form of [`FrameClock::set_frame_rate`].
    pub fn with_frame_rate(mut self, frame_rate: f64) -> Self {
        self.set_frame_rate(frame_rate);
        self
    }

    /// How many frames one update may emulate before the backlog is dropped.
    pub fn max_catch_up_frames(&self) -> u32 {
        self.max_catch_up_frames
    }

    /// Set the catch-up budget; zero is treated as one so the clock keeps running.
    pub fn set_max_catch_up_frames(&mut self, frames: u32) {
        self.max_catch_up_frames = frames.max(1);
    }

    /// Forget the seeded timestamp and any accumulated time.
    ///
    /// Call this whenever the host clock jumps for a reason that is not elapsed
    /// emulation time: loading a cartridge, resuming from pause, or returning
    /// from the background.
    pub fn reset(&mut self) {
        self.last_ms = None;
        self.window_start_ms = None;
        self.accumulator_ms = 0.0;
        self.frames_in_window = 0;
        self.fps = 0.0;
    }

    /// Frames the host should emulate now, given a monotonic timestamp in
    /// milliseconds.
    ///
    /// Zero means "nothing is due yet". A short burst of frames is normal after
    /// a stall; the count is capped at [`FrameClock::max_catch_up_frames`] and
    /// any backlog beyond it is discarded, so a host that cannot keep up falls
    /// behind in game time instead of accumulating latency it can never repay.
    pub fn frames_due(&mut self, now_ms: f64) -> u32 {
        let Some(last_ms) = self.last_ms else {
            // First update: there is no interval to measure yet.
            self.last_ms = Some(now_ms);
            self.window_start_ms = Some(now_ms);
            return 0;
        };
        self.last_ms = Some(now_ms);

        let mut delta_ms = now_ms - last_ms;
        if !delta_ms.is_finite() || delta_ms <= 0.0 {
            // A duplicated, rewound or nonsense timestamp contributes no time.
            return 0;
        }
        if delta_ms > self.max_delta_ms {
            delta_ms = self.max_delta_ms;
        }
        self.accumulator_ms += delta_ms;

        let mut frames = 0;
        while self.accumulator_ms >= self.frame_ms && frames < self.max_catch_up_frames {
            self.accumulator_ms -= self.frame_ms;
            frames += 1;
        }

        // Still behind after the whole catch-up budget: repay nothing, drop it.
        let budget_ms = self.frame_ms * self.max_catch_up_frames as f64;
        if self.accumulator_ms > budget_ms {
            self.accumulator_ms = 0.0;
        }

        self.frames_in_window += frames;
        self.sample_fps(now_ms);
        frames
    }

    /// Frame rate measured over the last sampling window, in Hz.
    ///
    /// Zero until the first window closes, so a host can tell "not measured
    /// yet" from a genuine zero.
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Average the frames emulated since the window opened, once it is full.
    fn sample_fps(&mut self, now_ms: f64) {
        let Some(window_start_ms) = self.window_start_ms else {
            self.window_start_ms = Some(now_ms);
            return;
        };

        let elapsed_ms = now_ms - window_start_ms;
        if elapsed_ms < self.fps_window_ms {
            return;
        }

        self.fps = self.frames_in_window as f64 * 1000.0 / elapsed_ms;
        self.frames_in_window = 0;
        self.window_start_ms = Some(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock whose frame is a round 10 ms, for readable arithmetic.
    fn clock_100hz() -> FrameClock {
        FrameClock::new(100.0)
    }

    #[test]
    fn first_update_only_seeds_the_clock() {
        let mut clock = clock_100hz();
        assert_eq!(clock.frames_due(0.0), 0);
        assert_eq!(clock.fps(), 0.0, "no window has closed yet");
    }

    #[test]
    fn a_full_frame_interval_releases_one_frame() {
        let mut clock = clock_100hz();
        clock.frames_due(1000.0);
        assert_eq!(clock.frames_due(1010.0), 1);
        // The leftover time is only three tenths of a frame.
        assert_eq!(clock.frames_due(1013.0), 0);
    }

    #[test]
    fn a_stall_releases_the_frames_needed_to_catch_up() {
        let mut clock = clock_100hz();
        clock.frames_due(0.0);
        assert_eq!(clock.frames_due(30.0), 3);
    }

    #[test]
    fn catch_up_is_capped_and_the_backlog_is_dropped() {
        let mut clock = clock_100hz();
        clock.frames_due(0.0);

        // 200 ms of backlog is capped at the budget of 4 frames.
        assert_eq!(clock.frames_due(200.0), 4);
        // The rest is discarded instead of being repaid frame by frame.
        assert_eq!(clock.frames_due(210.0), 1);
    }

    #[test]
    fn a_long_gap_is_clamped_before_it_is_charged() {
        let mut clock = clock_100hz();
        clock.frames_due(0.0);

        // Ten seconds of suspended time would be 1000 frames; the delta clamp
        // trims it to 250 ms first, and the budget then caps that at 4.
        assert_eq!(clock.frames_due(10_000.0), 4);
    }

    #[test]
    fn repeated_timestamps_contribute_nothing() {
        let mut clock = clock_100hz();
        clock.frames_due(500.0);
        assert_eq!(clock.frames_due(500.0), 0);
        // Even a timestamp that goes backwards is ignored rather than repaid.
        assert_eq!(clock.frames_due(400.0), 0);
    }

    #[test]
    fn a_healthy_pace_measures_the_target_rate() {
        let mut clock = clock_100hz();
        clock.frames_due(0.0);

        // One frame due every frame interval, for a full sample window.
        let mut now = 0.0;
        for _ in 0..60 {
            now += 10.0;
            clock.frames_due(now);
        }

        assert!(
            (clock.fps() - 100.0).abs() < 1.0,
            "measured {}",
            clock.fps()
        );
    }

    #[test]
    fn fps_below_the_target_signals_a_host_that_cannot_keep_up() {
        let mut clock = clock_100hz();
        clock.frames_due(0.0);

        // Ticks arrive every 100 ms, i.e. ten frames late, but the budget only
        // lets four through: 4 frames per tick is 40 Hz, not the targeted 100.
        let mut now = 0.0;
        for _ in 0..10 {
            now += 100.0;
            clock.frames_due(now);
        }

        assert!((clock.fps() - 40.0).abs() < 1.0, "measured {}", clock.fps());
    }

    #[test]
    fn reset_re_seeds_the_clock_and_clears_the_sample() {
        let mut clock = clock_100hz();
        clock.frames_due(0.0);
        clock.frames_due(1000.0);
        assert!(clock.fps() > 0.0);

        clock.reset();
        assert_eq!(clock.fps(), 0.0);
        // The next timestamp is a seed again, not a 1000 ms jump.
        assert_eq!(clock.frames_due(9_999_999.0), 0);
        assert_eq!(clock.frames_due(10_000_009.0), 1);
    }

    #[test]
    fn frame_rate_round_trips_and_rejects_nonsense() {
        let mut clock = clock_100hz();
        assert!((clock.frame_rate() - 100.0).abs() < 1e-9);

        clock.set_frame_rate(0.0);
        assert!((clock.frame_rate() - Region::Ntsc.fps()).abs() < 1e-9);

        clock.set_frame_rate(f64::NAN);
        assert!((clock.frame_rate() - Region::Ntsc.fps()).abs() < 1e-9);
    }

    #[test]
    fn pal_paces_slower_than_ntsc() {
        let ntsc = FrameClock::for_region(Region::Ntsc);
        let pal = FrameClock::for_region(Region::Pal);
        assert!(pal.frame_rate() < ntsc.frame_rate());
        assert!((ntsc.frame_rate() - Region::Ntsc.fps()).abs() < 1e-9);
    }
}
