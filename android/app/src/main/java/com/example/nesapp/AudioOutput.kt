package com.example.nesapp

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.os.Build

/**
 * The device end of the audio path: an `AudioTrack` that drains [AudioRing].
 *
 * Nothing here touches the core. The emulation thread produces samples, this
 * thread consumes them, and the only shared object is the ring — which is what
 * keeps a blocking device write away from the emulation loop.
 *
 * The sample rate is the device's own, read from `AudioManager` before any
 * cartridge is loaded and handed to the core through `Config.sampleRate`, so the
 * core resamples once, into the rate the speaker actually runs at.
 */
class AudioOutput(
    context: Context,
    private val ring: AudioRing,
    private val settings: Settings,
) {

    /** The rate the device wants; also what the core is asked to produce. */
    val sampleRate: Int = run {
        val manager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        manager.getProperty(AudioManager.PROPERTY_OUTPUT_SAMPLE_RATE)?.toIntOrNull() ?: 48_000
    }

    @Volatile
    private var running = false

    /** Set when the boot screen asks for its click before the loop starts. */
    @Volatile
    private var chimePending = false

    private var track: AudioTrack? = null
    private var thread: Thread? = null

    /** Scratch buffer, sized to one device period; reused so the loop allocates nothing. */
    private val period: Int = run {
        val min = AudioTrack.getMinBufferSize(
            sampleRate,
            AudioFormat.CHANNEL_OUT_STEREO,
            AudioFormat.ENCODING_PCM_FLOAT,
        )
        // `getMinBufferSize` returns a negative error code on failure; fall back
        // to a sane period rather than handing a negative size to the track.
        if (min > 0) min / (2 * FLOAT_BYTES) else sampleRate / 50
    }
    private val scratch = FloatArray(period.coerceAtLeast(256))

    /**
     * Start the device thread, if it is not already running.
     *
     * Called as soon as the app starts rather than when a game does: the boot
     * screen's click is the first thing the speaker is used for.
     */
    fun start() {
        if (running) return
        val device = try {
            openTrack()
        } catch (cause: Exception) {
            // A TV box without a usable output should play silently rather than
            // refuse to run; the picture and the game are unaffected.
            return
        }
        track = device
        running = true
        device.play()
        thread = Thread({ loop(device) }, "nes-audio").apply {
            priority = Thread.MAX_PRIORITY
            start()
        }
    }

    fun stop() {
        running = false
        thread?.join(500)
        thread = null
        track?.let { device ->
            runCatching { device.pause() }
            runCatching { device.flush() }
            runCatching { device.release() }
        }
        track = null
    }

    /**
     * Queue the synthesized boot click.
     *
     * Synthesized on purpose: the app ships no audio files, so there is no media
     * asset to license or explain.
     */
    fun playChime() {
        chimePending = true
    }

    private fun loop(device: AudioTrack) {
        if (chimePending) {
            chimePending = false
            writeBlocking(device, chime())
        }
        while (running) {
            // Read live rather than synced from a setter: the setting can change
            // from the in-game menu, and one snapshot read per period is cheaper
            // than keeping a copy honest.
            if (settings.muted) {
                // Drain and discard: the core keeps filling the ring, and a
                // backlog would otherwise play as a burst on the next unmute.
                ring.read(scratch, scratch.size)
                sleep(SILENT_SLEEP_MS)
                continue
            }
            val count = ring.read(scratch, scratch.size)
            if (count <= 0) {
                sleep(SILENT_SLEEP_MS)
                continue
            }
            writeBlocking(device, scratch, count)
        }
    }

    /** A blocking write: the device paces the thread, which is what keeps latency low. */
    private fun writeBlocking(device: AudioTrack, samples: FloatArray, count: Int = samples.size) {
        var written = 0
        while (written < count && running) {
            val result = runCatching {
                device.write(samples, written, count - written, AudioTrack.WRITE_BLOCKING)
            }.getOrElse { return }
            if (result <= 0) return
            written += result
        }
    }

    /**
     * A short two-tone click, the sound of a console being switched on.
     *
     * A square wave at a couple of hundred hertz for a few tens of
     * milliseconds, with a fast decay so it reads as a "tick" rather than a
     * note.
     */
    private fun chime(): FloatArray {
        val frames = sampleRate * CHIME_MS / 1000
        val samples = FloatArray(frames * 2)
        for (frame in 0 until frames) {
            val seconds = frame.toDouble() / sampleRate
            val firstHalf = frame < frames / 2
            val frequency = if (firstHalf) 660.0 else 880.0
            val phase = (seconds * frequency) % 1.0
            val square = if (phase < 0.5) 1.0 else -1.0
            val decay = 1.0 - frame.toDouble() / frames
            val value = (square * decay * decay * CHIME_LEVEL).toFloat()
            samples[frame * 2] = value
            samples[frame * 2 + 1] = value
        }
        return samples
    }

    private fun sleep(milliseconds: Long) {
        try {
            Thread.sleep(milliseconds)
        } catch (interrupted: InterruptedException) {
            Thread.currentThread().interrupt()
        }
    }

    private fun openTrack(): AudioTrack {
        val format = AudioFormat.Builder()
            .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
            .setSampleRate(sampleRate)
            .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
            .build()

        val bytes = period * 2 * FLOAT_BYTES

        val device = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            val builder = AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_GAME)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build(),
                )
                .setAudioFormat(format)
                .setBufferSizeInBytes(bytes)
                .setTransferMode(AudioTrack.MODE_STREAM)

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                // Ask for the low-latency path where the device has one; a box
                // that cannot honour it plays through the ordinary mixer.
                builder.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
            }
            builder.build()
        } else {
            // `AudioTrack.Builder` is API 23; on 21 and 22 the constructor is
            // the only way to ask for float samples.
            @Suppress("DEPRECATION")
            AudioTrack(
                AudioManager.STREAM_MUSIC,
                sampleRate,
                AudioFormat.CHANNEL_OUT_STEREO,
                AudioFormat.ENCODING_PCM_FLOAT,
                bytes,
                AudioTrack.MODE_STREAM,
            )
        }

        return device
    }

    private companion object {
        const val FLOAT_BYTES = 4

        /** How long the boot click lasts. */
        const val CHIME_MS = 140
        const val CHIME_LEVEL = 0.35

        /**
         * How long the audio thread sleeps when there is nothing to play.
         *
         * Short enough that a game's first samples are not delayed noticeably,
         * long enough not to spin a core while idle.
         */
        const val SILENT_SLEEP_MS = 4L
    }
}
