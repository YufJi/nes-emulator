package com.example.nesapp

import java.util.concurrent.atomic.AtomicBoolean

/**
 * The one-frame-deep handshake between the emulation thread and the GL thread.
 *
 * The display is the clock (`NesSession`): the GL thread presents a picture, and
 * only then may the emulation thread produce the next one. One flag is the whole
 * protocol — [tryBeginProduce] takes it, [onPresented] gives it back — and
 * because the exchange is exactly one frame deep, a frame that cannot be finished
 * before the next vsync is shown one refresh late instead of being thrown away,
 * and a display that stops presenting stops the console rather than letting it
 * run on unseen.
 *
 * That last part is the point. Measured on a Sony BRAVIA 4K VH22 (Cortex-A55 at
 * 1.8 GHz, 32-bit userspace), a free-running timer let the console emulate 60
 * frames a second while the surface only received a new picture every three or
 * four vsyncs: every wake-up ran a batch of frames and handed over only the last
 * one, so 17 of those 60 frames reached the screen and the rest were emulated
 * and discarded.
 *
 * Both calls are compare-and-set, so a duplicated or late call from either thread
 * does nothing rather than counting twice — the GL thread presents once per
 * requested render, but a surface that is recreated draws once on its own, and a
 * stray call must not hand the console an extra frame.
 *
 * No Android types here: it is a plain object and its rules are unit tested on
 * the JVM.
 */
class FrameGate {

    private val outstanding = AtomicBoolean(false)

    /** Forget any frame in flight, e.g. when a session starts or resumes. */
    fun reset() {
        outstanding.set(false)
    }

    /**
     * The emulation thread asking whether it may run one frame.
     *
     * True means it may: nothing it produced is still on its way to the screen.
     * False means the display has not caught up, and a frame run now would be
     * presented, if at all, behind the one it should have replaced.
     */
    fun tryBeginProduce(): Boolean = outstanding.compareAndSet(false, true)

    /**
     * The GL thread reporting that a picture is on its way to the screen.
     *
     * True when it was the frame the emulation thread was waiting on, false when
     * the same picture was drawn twice (or no picture was drawn at all).
     */
    fun onPresented(): Boolean = outstanding.compareAndSet(true, false)
}
