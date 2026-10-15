package com.example.nesapp

import android.content.Context
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.Message
import android.util.Log
import android.view.Choreographer
import android.view.KeyEvent
import android.view.MotionEvent
import java.io.File
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
    private val handler = Handler(thread.looper) { message ->
        handleMessage(message)
        true
    }

    private var nes: Nes? = null
    private var clock: FrameClock? = null

    /** Whether the display link is currently driving the console. */
    private var pacing = false

    /** Set while the session is in the foreground; no frames are run otherwise. */
    private var foreground = false

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
        // A pause must not be emulated as elapsed time, and the next timestamps
        // have no relationship to the last ones seen.
        onEmulation { clock?.reset() }
        if (!pacing) {
            pacing = true
            Choreographer.getInstance().postFrameCallback(vsync)
        }
    }

    /** Stop advancing; the console keeps its state. */
    fun pause() {
        foreground = false
        pacing = false
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
     * The display link.
     *
     * Registered on the main thread, which is where a `Choreographer` belongs;
     * each timestamp is handed to the emulation thread rather than used here, so
     * the interface is never blocked by a frame of emulation.
     */
    private val vsync = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!pacing || !foreground) return
            val nowMs = frameTimeNanos / NANOS_PER_MILLI
            // One tick in flight at a time: when the emulation thread is busy the
            // timestamp it has not read yet is replaced rather than queued, and
            // `FrameClock` repays the elapsed time itself.
            handler.removeMessages(MSG_TICK)
            handler.obtainMessage(MSG_TICK, nowMs).sendToTarget()
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    private fun handleMessage(message: Message) {
        if (message.what == MSG_TICK) {
            runFrames(message.obj as Double)
        }
    }

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
        surface.show(console.frameBufferBytes())
        surface.requestRender()
        pumpAudio(console)
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

        const val MSG_TICK = 1
    }
}
