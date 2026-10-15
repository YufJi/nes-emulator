package com.example.nesapp

import android.content.Context
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.util.Log
import android.view.KeyEvent
import android.view.MotionEvent
import java.io.File
import com.sun.jna.Pointer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
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
 * into it happens on one dedicated thread; `Choreographer` on the main thread
 * only hands over timestamps, and the audio thread only ever touches
 * [AudioRing]. That is the same shape the iOS app has.
 *
 * Pacing comes from the core's own [FrameClock], fed the display's timestamps, so
 * a PAL cartridge runs at 50 Hz and an NTSC one at 60.0985 Hz without the host
 * knowing either number.
 */
class NesSession(
    context: Context,
    private val settings: Settings,
    private val onStatus: (framesPerSecond: Double, cpuHalted: Boolean) -> Unit,
    private val onFatalError: (String) -> Unit,
) {

    /** Where the pictures go. Compose attaches this to the playing screen. */
    val surface = VideoSurface(context, settings)

    private val ring = AudioRing(AUDIO_RING_SAMPLES)
    private val audio = AudioOutput(context, ring, settings)
    private val input = InputMapper()

    private val thread = HandlerThread("nes-emulation").apply { start() }
    private val handler = Handler(thread.looper)

    private var nes: Nes? = null
    private var clock: FrameClock? = null

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

    /** Whether the display link is currently driving the console. */
    @Volatile
    private var pacing = false

    /** Set while the session is in the foreground; no frames are run otherwise. */
    @Volatile
    private var foreground = false

    /**
     * The pacemaker: a timer on this thread, not the display link.
     *
     * `FrameClock` is a timestamp-driven pacer precisely so that a host without a
     * usable display link can still run a console at its own rate — the embedded
     * plan in `docs/embedded-handheld.md` drives it from a hardware timer, and
     * this is the same idea with `System.nanoTime()`.
     *
     * A `Choreographer` looks like the better answer and is not: measured on a
     * Sony BRAVIA, callbacks arrived every ~84 ms (one in five vsyncs — the
     * system throttles the link when the app is not the one being composited),
     * so `FrameClock` was handed a fifth of the timestamps, hit its four-frame
     * catch-up budget on each of them and settled at 50 frames a second. A timer
     * is not the display's business to throttle, and the picture is presented on
     * demand anyway, so pacing and presentation stay independent.
     */
    private val ticker = object : Runnable {
        override fun run() {
            if (!pacing || !foreground) return
            runFrames(System.nanoTime() / NANOS_PER_MILLI)
            if (pacing && foreground) {
                handler.postDelayed(this, TICK_INTERVAL_MS)
            }
        }
    }

    private var lastReportAt = 0L
    private var lastHalted = false

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
            clock = FrameClock(console.frameRate())
            input.forgetApplied()
            ring.flush()
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
            ring.flush()
            input.forgetApplied()
        }
    }

    /** Bring the session to the foreground: start sound and the display link. */
    fun resume() {
        foreground = true
        surface.onResume()
        audio.start()
        onEmulation {
            // A pause must not be emulated as elapsed time, and the next
            // timestamps have no relationship to the last ones seen.
            clock?.reset()
            if (!pacing) {
                pacing = true
                handler.post(ticker)
            }
        }
    }

    /** Stop advancing; the console keeps its state. */
    fun pause() {
        foreground = false
        surface.onPause()
        audio.stop()
        onEmulation {
            pacing = false
            handler.removeCallbacks(ticker)
        }
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

    private fun runFrames(nowMs: Double) {
        val console = nes ?: return
        val pacer = clock ?: return

        val due = pacer.framesDue(nowMs)
        if (due == 0u) return

        repeat(due.toInt()) {
            // Input first: the frame about to run is the frame that samples it.
            input.applyTo(console, console.frameNumber().toLong())
            try {
                console.frame()
            } catch (cause: NesException) {
                fail(cause)
                return
            }
        }
        publish(console)
        // One upload per wake-up, of the newest frame, however many were run.
        surface.show(readFrame(console))
        surface.requestRender()
        pumpAudio(console)
    }

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
     * Done here rather than on the audio thread because `fillAudio` needs the
     * machine, and the machine belongs to this thread.
     */
    private fun pumpAudio(console: Nes) {
        ring.write(console.fillAudio(AUDIO_PULL_SAMPLES.toUInt()))
    }

    /** Tell the interface what its status bar shows, at a readable rate. */
    private fun publish(console: Nes) {
        val now = System.currentTimeMillis()
        val halted = console.isCpuHalted()
        if (halted == lastHalted && now - lastReportAt < REPORT_INTERVAL_MS) return
        lastReportAt = now
        lastHalted = halted
        onStatus(clock?.fps() ?: 0.0, halted)
    }

    private fun fail(cause: NesException) {
        Log.e(TAG, "the console stopped", cause)
        pacing = false
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

        /** Nanoseconds in a millisecond: the display link hands over nanoseconds. */
        const val NANOS_PER_MILLI = 1_000_000.0

        /** One second of stereo samples — the most the ring will ever hold back. */
        const val AUDIO_RING_SAMPLES = 48_000 * 2

        /**
         * How many samples to ask the core for per wake-up: more than a frame
         * produces at either refresh rate, so a wake-up that ran several frames
         * drains them all.
         */
        const val AUDIO_PULL_SAMPLES = 4096

        /** Twice a second is enough for a frame-rate readout, and not a churn. */
        const val REPORT_INTERVAL_MS = 500L

        /** A load or a save is a few hundred kilobytes' worth of work at most. */
        const val EMULATION_CALL_TIMEOUT_MS = 2_000L

        /** One frame of `0xAARRGGBB` pixels: 256 x 240 x 4. */
        const val FRAME_BYTES = 256 * 240 * 4
        const val BYTES_PER_PIXEL = 4

        /**
         * How often the loop looks at the clock.
         *
         * A quarter of a frame at 60 Hz, so a frame is released within a few
         * milliseconds of the clock asking for it, and an idle wake-up is a
         * message dispatch rather than any work.
         */
        const val TICK_INTERVAL_MS = 4L
    }
}
