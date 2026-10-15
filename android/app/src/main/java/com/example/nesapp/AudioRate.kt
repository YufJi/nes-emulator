package com.example.nesapp

import kotlin.math.min
import kotlin.math.roundToInt

/**
 * The sample rate to ask the core for, so its audio lasts exactly as long as the
 * frames it is asked to emulate.
 *
 * The display is the one clock this front end cannot argue with: the television
 * this was written for offers a single 60.000004 Hz mode, while an NTSC cartridge
 * emulates 60.0985 frames a second. Pacing at the panel runs the game 0.16% slow
 * — inaudible, and the alternative is a repeated frame every ten seconds — but it
 * also means the core produces 0.16% fewer samples per second than the sound
 * device consumes, so a ring fed straight from the core would run dry after
 * about twenty minutes.
 *
 * The core resamples whatever it makes to a rate the host hands it, and that rate
 * does not have to be the device's. Emulated time here runs at
 * `pacedFps / cartridgeFps` real seconds per second, so asking for
 * `deviceRate × cartridgeFps / pacedFps` samples per *emulated* second — 48079
 * where the device plays 48000 — makes the two ends agree, at the price of
 * playing 0.16% flat. That is the same trade RetroArch makes on this box, where
 * `audio_rate_control` absorbs a display the cartridge does not divide evenly.
 *
 * The rate is rounded to whole samples, which is what the core takes; the residue
 * is well under a sample a second and disappears into the ring's own overflow.
 */
fun coreSampleRate(deviceRate: Int, cartridgeFps: Double, displayFps: Double): Int {
    if (deviceRate <= 0) return deviceRate
    if (!cartridgeFps.isFinite() || cartridgeFps <= 0.0) return deviceRate
    if (!displayFps.isFinite() || displayFps <= 0.0) return deviceRate

    // Whichever is slower is what the console actually runs at: a PAL cartridge
    // on a 60 Hz screen still runs its own 50.0067 frames a second, so one of its
    // seconds is one of ours and the device rate is already the right answer.
    val pacedFps = min(cartridgeFps, displayFps)
    val rate = (deviceRate * cartridgeFps / pacedFps).roundToInt()
    return if (rate > 0) rate else deviceRate
}
