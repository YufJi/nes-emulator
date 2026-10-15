package com.example.nesapp

import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.Process
import android.util.Log
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.WindowManager
import java.io.File
import java.nio.FloatBuffer
import com.sun.jna.Memory
import com.sun.jna.Pointer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.min
import kotlin.math.roundToLong
import uniffi.nes_uniffi.Config
import uniffi.nes_uniffi.FrameClock
import uniffi.nes_uniffi.Nes
import uniffi.nes_uniffi.NesException
import uniffi.nes_uniffi.Region

/**
 * A running console: the machine, the thread it lives on, its sound and its
 * picture.
 *
 * The core is single-owner (`docs/architecture.md` §10.5) — `Nes` is `Send` but
 * not `Sync`, and the machine is not built for concurrent callers. So every call
 * into it happens on one dedicated thread, and the audio thread only ever touches
 * [AudioRing]. That is the same shape the iOS app has, except for what drives the
 * loop: iOS draws with a `CADisplayLink` and paces with [FrameClock], while here
 * the display itself is the pacemaker.
 *
 * The display is the pacemaker and the swap is the beat. Measured on a Sony
 * BRAVIA 4K VH22 (four Cortex-A55 at 1.8 GHz, 32-bit userspace): a
 * `Choreographer` callback arrived every ~84 ms — one vsync in five, the system
 * throttling a link whose app it is not compositing — so [FrameClock] was handed
 * a fifth of the timestamps and settled at a hard 50 fps. A `System.nanoTime()`
 * timer was no better in the end: a frame took ~15.5 ms to emulate, so the timer
 * ran a batch of four and handed over only the newest of them, and the surface
 * received 17 pictures a second while the readout said 60. Both failures come
 * from pacing against something other than the thing that shows the picture.
 *
 * So nothing here runs on a timer. [FrameGate] allows one frame in flight: the
 * loop produces a frame, hands it over, and waits to be told the swap happened
 * before producing another. `eglSwapBuffers` is what waits for vsync, so it
 * paces the whole chain and the emulation thread follows it — which is also why
 * a surface that stops presenting stops the console instead of letting it run
 * unseen. The price is that the console runs no faster than the display, so the
 * core's sample rate is retuned to the panel; see `AudioRate`.
 */
class NesSession(
    private val context: Context,
    private val settings: Settings,
    private val onStatus: (framesPerSecond: Double, cpuHalted: Boolean) -> Unit,
    private val onFatalError: (String) -> Unit,
) {

    /**
     * The one-frame handshake with the GL thread.
     *
     * Taken by the loop before it emulates, given back by the GL thread when the
     * picture reaches the screen. This is the whole pacemaker: a display that
     * presents nothing gets no more frames, and a display that presents an extra
     * picture cannot make the console run one.
     */
    private val gate = FrameGate()

    /** Where the pictures go. Compose attaches this to the playing screen. */
    val surface = VideoSurface(context, settings, gate) { postTick() }

    private val ring = AudioRing(AUDIO_RING_SAMPLES)
    private val audio = AudioOutput(context, ring, settings)
    private val input = InputMapper()

    /**
     * The thread the machine lives on, and the one that must not be starved: it
     * has 16.6 ms to emulate a frame, and the box this was written for has four
     * small cores and nothing to spare. `THREAD_PRIORITY_URGENT_DISPLAY` is nice
     * -8, which android grants an app that asks; RetroArch runs its own emulation
     * thread at -10 on this hardware.
     */
    private val thread = HandlerThread("nes-emulation", Process.THREAD_PRIORITY_URGENT_DISPLAY).apply { start() }
    private val handler = Handler(thread.looper)

    private var nes: Nes? = null
    private var clock: FrameClock? = null

    /** The cartridge's own rate, for retuning the pacer and the resampler. */
    private var cartridgeFps = 0.0

    /**
     * The panel's refresh rate in Hz, re-read whenever the session comes forward.
     *
     * A television can change mode while the app is away — a 24 Hz film, a 50 Hz
     * channel — and both the beat and the core's sample rate are derived from it.
     * One read at resume is enough: this is not a tracking loop, and a mode change
     * mid-game is a pause and a resume as far as the app is concerned.
     */
    @Volatile
    private var displayFps = DEFAULT_DISPLAY_FPS

    /** The sample rate the core is currently resampling to, to avoid a needless retune. */
    private var appliedSampleRate = 0

    /**
     * The buffer the core's audio is pulled into, allocated once.
     *
     * `fill_audio_into` writes where it is told instead of handing back a
     * `List<Float>`: the copying shape boxes 4096 numbers per frame, 60 times a
     * second, and this box cannot afford the garbage. One address down, one bulk
     * copy back into the ring.
     */
    private val audioBuffer = Memory(AUDIO_PULL_SAMPLES.toLong() * FLOAT_BYTES)

    /**
     * The same memory seen as floats, in native order, for reading it back.
     *
     * A view, not a copy, and made once: the samples go straight from the core's
     * buffer into the ring with nothing in between. Its position is the ring's
     * business (`AudioRing.write` advances it), so it is rewound before each fill.
     */
    private val audioSamples: FloatBuffer =
        audioBuffer.getByteBuffer(0L, AUDIO_PULL_SAMPLES.toLong() * FLOAT_BYTES).asFloatBuffer()

    /**
     * The two buffers the picture is copied into, alternating.
     *
     * The core's frame buffer is read straight into one of these — one `memcpy`
     * from the machine's own memory — rather than asking the binding for a fresh
     * `ByteArray` per frame: at 60 frames a second that was 245 KB allocated and
     * thrown away 60 times over, which on a TV is 14 MB/s of large-object churn
     * for the garbage collector to sweep. Two buffers rather than one because
     * the GL thread may still be uploading the buffer the emulation thread
     * filled last time; the one it is not holding is the one to fill.
     */
    private val frameBuffers = Array(2) { ByteArray(FRAME_BYTES) }
    private var frameBufferIndex = 0

    /** Set while the session is in the foreground; no frames are run otherwise. */
    @Volatile
    private var foreground = false

    /**
     * The composed timeline handed to [FrameClock], in milliseconds.
     *
     * The pacer is fed a synthetic clock rather than `System.nanoTime()` because
     * the unit of this loop is the frame the display took, not the millisecond
     * that happened to pass: one turn of the loop is one display period, whether
     * it was a swap the display acknowledged or a vsync the loop decided to skip.
     * That is what keeps `framesDue` at 0 or 1 — handed raw time, a late present
     * would build up a two-frame debt and hand back a batch to emulate and throw
     * away, which is exactly the failure this pacing exists to remove.
     */
    private var clockMs = 0.0

    /** The turn of the loop. Posted by a presentation or by the loop itself, never by a timer. */
    private val tick = Runnable { runTick() }

    private var lastReportAt = 0L
    private var lastPresented = 0L

    /**
     * Load a cartridge and run its first frame.
     *
     * [saveRam] is written back before that frame: boot code can read its save
     * that early, and a frame emulated against blank SRAM would look like "no
     * save" to the game.
     *
     * Throws [NesException] when the image is not a cartridge this core can run;
     * the caller decides what to tell the viewer.
     */
    fun load(rom: ByteArray, saveRam: ByteArray?) {
        onEmulation {
            val console = Nes.withConfig(
                Config(
                    regionOverride = when (settings.region) {
                        RegionChoice.Auto -> null
                        RegionChoice.Ntsc -> Region.NTSC
                        RegionChoice.Pal -> Region.PAL
                    },
                    // The core buffers audio for the host to pull, and muting
                    // happens on this side of the boundary, so unmuting never
                    // needs a reload.
                    enableAudio = true,
                    sampleRate = audio.sampleRate.toUInt(),
                ),
            )
            console.loadRom(rom)
            if (saveRam != null) console.loadSaveRam(saveRam)

            // The core keeps a dot-resolution record of the PPU bus for hosts
            // that ask for one — the mapper tests are the ones that do — and
            // writes to it on every one of the 89,342 dots in a frame. Measured
            // on a Sony BRAVIA, not keeping it is worth 5–8% of the frame budget:
            // small next to the emulation itself, and free, because this front
            // end never reads the trace. Zero means "do not keep one".
            console.setPpuBusTraceCapacity(0u)

            // One frame up front: the first picture is not blank, and a
            // cartridge that traps fails here rather than mid-session.
            console.frame()

            nes?.close()
            nes = console
            cartridgeFps = console.frameRate()
            // The pacer runs at whichever of the cartridge and the panel is
            // slower, so a PAL cartridge keeps its own 50 Hz on a 60 Hz screen.
            clock = FrameClock(min(cartridgeFps, displayFps))
            retune(console)
            anchorClock()
            input.forgetApplied()
            ring.flush()
            lastPresented = surface.presentedCount
            lastReportAt = 0L
            publish(console)
        }
    }

    /** Whether the loaded cartridge keeps a battery save. */
    fun hasBattery(): Boolean = onEmulation { nes?.hasBattery() } ?: false

    /** The console's battery RAM, or null when there is nothing to write. */
    fun readBattery(): ByteArray? = onEmulation {
        val console = nes ?: return@onEmulation null
        if (console.hasBattery()) console.saveRam() else null
    }

    /** Write the battery save to [target], when the cartridge has one. */
    fun writeBattery(target: File) {
        val data = readBattery() ?: return
        target.parentFile?.mkdirs()
        target.writeBytes(data)
    }

    /** A save state of the whole machine. */
    fun saveState(): ByteArray? = onEmulation { nes?.saveState() }

    /** Restore a state produced by [saveState]. */
    fun loadState(state: ByteArray) {
        onEmulation {
            nes?.loadState(state)
            // The buttons the old machine had held are not the ones the new one
            // has; send the current set again rather than leaving one down.
            input.forgetApplied()
        }
    }

    /** Press the reset button. */
    fun reset() {
        onEmulation {
            nes?.reset()
            clock?.reset()
            anchorClock()
            ring.flush()
            input.forgetApplied()
        }
    }

    /**
     * Bring the session to the foreground: start sound and let the display drive.
     *
     * The panel may have changed mode while the app was away, and the beat and
     * the core's sample rate are both derived from it, so it is re-read first.
     * The timer-free loop has nothing to start — only a first turn to post.
     */
    fun resume() {
        foreground = true
        val panelMoved = readDisplayRate()
        surface.onResume()
        audio.start()
        gate.reset()
        onEmulation {
            if (panelMoved) nes?.let { retune(it) }
            // A pause must not be emulated as elapsed time, and the next
            // timestamps have no relationship to the last ones seen.
            clock?.reset()
            anchorClock()
        }
        postTick()
    }

    /**
     * Stop advancing; the console keeps its state.
     *
     * A turn of the loop already in flight is waited out before the GL thread
     * goes away: that turn posts its next one on this thread's looper, so a no-op
     * queued behind it is a barrier, and the loop is idle afterwards because
     * every turn checks [foreground] before it takes the gate.
     */
    fun pause() {
        foreground = false
        onEmulation { }
        surface.onPause()
        audio.stop()
    }

    /**
     * Release everything.
     *
     * The caller writes the battery save first ([writeBattery]), because the
     * console is gone after this.
     */
    fun close() {
        pause()
        onEmulation {
            nes?.close()
            nes = null
            clock = null
            ring.flush()
        }
        input.releaseAll()
        // Before the looper goes: a draw already in the GL thread's queue can
        // still report itself, and a session that is closing must not answer with
        // one more turn of the loop.
        gate.reset()
        thread.quitSafely()
    }

    /** Ask for the boot screen's click, synthesized by the audio thread. */
    fun playBootChime() {
        audio.start()
        audio.playChime()
    }

    /**
     * Route a key event to the console.
     *
     * Returns true when the key is one of the console's, so the caller can keep
     * it from also moving the interface's focus. Repeats are re-asserted rather
     * than ignored: holding a key is a pad holding a button, and the console
     * wants the state, not the event.
     */
    fun onKeyEvent(deviceId: Int, gamepad: Boolean, event: KeyEvent): Boolean {
        val pressed = event.action == KeyEvent.ACTION_DOWN
        return input.onKey(deviceId, gamepad, event.keyCode, pressed)
    }

    /** Route a joystick event to the console; true when it moved something. */
    fun onMotionEvent(deviceId: Int, event: MotionEvent): Boolean =
        input.onMotion(deviceId, event)

    // MARK: - The emulation thread

    /**
     * One vsync: emulate a frame if the cartridge has one due, and hand it over.
     *
     * A turn arrives from one of two places — the GL thread reporting a swap, or
     * this loop coming back after it skipped a vsync — and it is worth exactly one
     * vsync of emulated time either way. At most one frame is emulated per turn,
     * so a batch is not possible by construction: with the display as the pacer
     * there is nowhere for a batch to hide.
     */
    private fun runTick() {
        if (!foreground) return
        val console = nes ?: return
        val pacer = clock ?: return

        // One frame in flight. A turn that cannot take the gate is a turn that
        // arrived before the display caught up, and it has nothing to do.
        if (!gate.tryBeginProduce()) return

        val due = pacer.framesDue(advanceClock())
        if (due == 0u) {
            // Nothing is due yet. A 50 Hz cartridge on a 60 Hz panel skips one
            // vsync in six, and skipping is exactly what this turn did: the
            // permit goes back, and the loop comes round again one vsync later.
            gate.onPresented()
            postTick(displayPeriodMs())
            return
        }

        // Input first: the frame about to run is the frame that samples it.
        input.applyTo(console, console.frameNumber().toLong())
        try {
            console.frame()
        } catch (cause: NesException) {
            // The gate stays taken: a machine in a bad state is not given
            // another frame, and the failure is reported to the viewer.
            fail(cause)
            return
        }
        publish(console)
        surface.show(readFrame(console))
        surface.requestRender()
        pumpAudio(console)
    }

    /**
     * Hand the loop its next turn, replacing whatever turn was already waiting.
     *
     * A turn can be asked for from two threads at once — the GL thread after a
     * swap, this one before a skipped vsync — and the loop wants exactly one of
     * them: two turns would take two goes at the single frame the display
     * released. So the pending one is replaced rather than joined.
     */
    private fun postTick(delayMs: Long = 0L) {
        handler.removeCallbacks(tick)
        handler.postDelayed(tick, delayMs)
    }

    /** One display period in milliseconds, never zero. */
    private fun displayPeriodMs(): Long = (1000.0 / displayFps).roundToLong().coerceAtLeast(1L)

    /**
     * The composed clock: one display period per turn of the loop.
     *
     * The pacer is fed a synthetic timeline rather than `System.nanoTime()`
     * because a turn stands for the frame the display took, not the milliseconds
     * that happened to pass. The re-anchor is a guard, not a correction: if turns
     * were lost for a whole frame or more — a stall, a mode switch, a thread that
     * was not scheduled — the composed clock is pulled forward to the real one
     * rather than left to replay the gap frame by frame. It is only ever pulled
     * forward: a turn that produced nothing has to accumulate towards the next
     * frame, or a 50 Hz cartridge would never reach one.
     */
    private fun advanceClock(): Double {
        clockMs += 1000.0 / displayFps
        val now = System.nanoTime() / NANOS_PER_MILLI
        if (clockMs < now - CLOCK_DRIFT_MS) clockMs = now
        return clockMs
    }

    /** Start the composed clock from the real one, e.g. on a load or a resume. */
    private fun anchorClock() {
        clockMs = System.nanoTime() / NANOS_PER_MILLI
    }

    /**
     * Point the pacer and the core's resampler at the panel.
     *
     * Both follow the display: the pacer runs at whichever of the cartridge and
     * the panel is slower, and the core is asked for however many samples make a
     * second of audio the same length as a second of emulation — 48079 where the
     * device plays 48000, because pacing an NTSC cartridge to a 60.000004 Hz panel
     * runs it 0.16% slow. See `AudioRate`. Called only when the panel moved, since
     * the core drops whatever it has queued when its rate changes.
     */
    private fun retune(console: Nes) {
        clock?.setFrameRate(min(cartridgeFps, displayFps))
        val rate = coreSampleRate(audio.sampleRate, cartridgeFps, displayFps)
        if (rate != appliedSampleRate) {
            console.setSampleRate(rate.toUInt())
            appliedSampleRate = rate
        }
    }

    /**
     * Re-read the panel's refresh rate; true when it moved.
     *
     * `Display.refreshRate` is the only honest answer here — `supportedModes`
     * on the television this was written for holds a single 60.000004 Hz entry —
     * and it is read on the main thread, where the context is a display one.
     */
    private fun readDisplayRate(): Boolean {
        val rate = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            runCatching { context.display?.refreshRate }.getOrNull()
        } else {
            runCatching { legacyRefreshRate() }.getOrNull()
        }
        val resolved = if (rate != null && rate > 1f) rate.toDouble() else DEFAULT_DISPLAY_FPS
        if (resolved == displayFps) return false
        displayFps = resolved
        return true
    }

    /** `defaultDisplay`, for the android versions that predate `Context.display`. */
    @Suppress("DEPRECATION")
    private fun legacyRefreshRate(): Float =
        (context.getSystemService(Context.WINDOW_SERVICE) as WindowManager).defaultDisplay.refreshRate

    /**
     * Copy the console's picture into a buffer of this front end's own.
     *
     * The address is the core's `frame_buffer_ptr`, re-read every frame because
     * `load_state` may move the buffer, and its length is checked against what
     * the core says it holds before anything is read: this is the one place the
     * Android host touches the machine's memory directly, and a wrong length
     * would be a crash rather than a wrong picture. (The iOS app does the same
     * thing with `UnsafeRawBufferPointer`; the binding's `frame_buffer_bytes` is
     * the copying alternative that Kotlin hosts without JNA would use.)
     */
    private fun readFrame(console: Nes): ByteArray {
        val pending = surface.pendingBuffer()
        val index = if (pending === frameBuffers[frameBufferIndex]) 1 - frameBufferIndex else frameBufferIndex
        frameBufferIndex = index
        val target = frameBuffers[index]

        val pointer = console.frameBufferPtr().toLong()
        val bytes = console.frameBufferLen().toLong() * BYTES_PER_PIXEL.toLong()
        if (pointer != 0L && bytes == target.size.toLong()) {
            Pointer(pointer).read(0L, target, 0, target.size)
        }
        return target
    }

    /**
     * Move whatever the core produced into the ring.
     *
     * Done here rather than on the audio thread because the core needs the
     * machine, and the machine belongs to this thread. The samples land in
     * [audioBuffer] because of what the copying shape costs the Kotlin side: a
     * `List<Float>` is 4096 boxed numbers per frame, 60 times a second, and this
     * box is being asked to hold 60 frames a second with four small cores.
     */
    private fun pumpAudio(console: Nes) {
        val address = Pointer.nativeValue(audioBuffer).toULong()
        val count = console.fillAudioInto(address, AUDIO_PULL_SAMPLES.toUInt()).toInt()
        if (count > 0) {
            audioSamples.position(0)
            ring.write(audioSamples, count)
        }
    }

    /**
     * Tell the interface what its status bar shows, at a readable rate.
     *
     * The number is the rate the display took pictures, not the rate the console
     * ran: those are the same number when the loop is healthy, and when it is not
     * — which is the whole reason this front end was rewritten — the presented
     * count is the one that tells the truth about what the viewer is seeing.
     *
     * The elapsed time is checked before anything is asked of the core. Asking it
     * whether the CPU is halted is a call across the binding, which costs about
     * 0.6 ms of a 16.7 ms frame on this hardware, and a label that is redrawn
     * twice a second has no business being fed sixty times. A halted machine is
     * reported on the next report rather than on the next frame, which for a
     * readout is the same instant.
     */
    private fun publish(console: Nes) {
        val now = System.currentTimeMillis()
        if (now - lastReportAt < REPORT_INTERVAL_MS) return

        val halted = console.isCpuHalted()
        val presented = surface.presentedCount
        val elapsed = now - lastReportAt
        if (lastReportAt != 0L && elapsed > 0) {
            onStatus((presented - lastPresented) * 1000.0 / elapsed, halted)
        }
        lastPresented = presented
        lastReportAt = now
    }

    private fun fail(cause: NesException) {
        Log.e(TAG, "the console stopped", cause)
        onFatalError(
            "模拟器遇到问题，已停止这一局：${cause.message ?: cause::class.java.simpleName}",
        )
    }

    /**
     * Run [block] on the emulation thread and wait for it.
     *
     * The machine is single-owner, so anything that touches it has to hop
     * threads. These are small, infrequent calls — a load, a save, a reset — which
     * is why waiting for one is acceptable here where it never would be on the
     * frame path.
     *
     * Anything [block] throws is rethrown on the caller's thread; without the
     * catch it would reach the thread's `Looper` and take the process down.
     */
    private fun <T> onEmulation(block: () -> T): T? {
        if (Looper.myLooper() == thread.looper) return block()
        if (!thread.isAlive) return null

        var result: T? = null
        var failure: Throwable? = null
        val done = CountDownLatch(1)
        handler.post {
            try {
                result = block()
            } catch (cause: Throwable) {
                failure = cause
            } finally {
                done.countDown()
            }
        }

        if (!done.await(EMULATION_CALL_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
            Log.w(TAG, "the console did not answer in time")
            return null
        }
        failure?.let {
            Log.e(TAG, "console call failed", it)
            throw it
        }
        return result
    }

    private companion object {
        const val TAG = "NesSession"

        /** Nanoseconds in a millisecond: the composed clock is in milliseconds. */
        const val NANOS_PER_MILLI = 1_000_000.0

        /**
         * A panel nobody can read is treated as the 60 Hz one nearly every
         * television is: the beat has to come from somewhere.
         */
        const val DEFAULT_DISPLAY_FPS = 60.0

        /**
         * How far the composed clock may fall behind the real one before it is
         * pulled forward, in milliseconds. A display period is 16.7 of them, so
         * anything smaller than this is a hitch and anything larger is lost time.
         */
        const val CLOCK_DRIFT_MS = 10.0

        /** One second of stereo samples — the most the ring will ever hold back. */
        const val AUDIO_RING_SAMPLES = 48_000 * 2

        /**
         * How many samples to ask the core for per turn of the loop, and how many
         * the direct-pull buffer holds.
         *
         * One frame is at most 1602 of them (801 stereo pairs at the core's
         * 48079 Hz) and a turn emulates exactly one frame, so a turn never leaves
         * anything behind in the core's queue.
         */
        const val AUDIO_PULL_SAMPLES = 4096

        /** Twice a second is enough for a frame-rate readout, and not a churn. */
        const val REPORT_INTERVAL_MS = 500L

        /** A load or a save is a few hundred kilobytes' worth of work at most. */
        const val EMULATION_CALL_TIMEOUT_MS = 2_000L

        /** One frame of `0xAARRGGBB` pixels: 256 x 240 x 4. */
        const val FRAME_BYTES = 256 * 240 * 4
        const val BYTES_PER_PIXEL = 4

        const val FLOAT_BYTES = 4L
    }
}
