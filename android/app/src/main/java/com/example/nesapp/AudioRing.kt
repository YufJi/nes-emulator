package com.example.nesapp

/**
 * The float buffer between the emulation thread and the audio thread.
 *
 * The core is deliberately single-owner (`docs/architecture.md` §10.5): the
 * emulation thread pulls samples with `fillAudio` and hands them over here, and
 * the audio thread never touches the console. The producer never blocks — if the
 * consumer falls behind, the oldest samples go, because stale audio is worse
 * than a gap.
 *
 * The critical section is a `System.arraycopy` under a monitor. That is fine at
 * this size; a realtime audio callback would prefer an atomic head/tail, which
 * would mean giving up the JVM's own bounds checks for nothing measurable here.
 */
class AudioRing(capacitySamples: Int) {

    private val storage = FloatArray(capacitySamples.coerceAtLeast(2))
    private val capacity = storage.size
    private val lock = Any()

    /** Write position, one past the newest sample. */
    private var head = 0

    /** Read position: the oldest sample still waiting. */
    private var tail = 0

    /** Samples waiting to be played. */
    val available: Int
        get() = synchronized(lock) { (head - tail + capacity) % capacity }

    /** Throw away everything queued, e.g. when a cartridge is loaded. */
    fun flush() {
        synchronized(lock) {
            head = 0
            tail = 0
        }
    }

    /**
     * Queue [samples] (interleaved stereo).
     *
     * When they do not fit, the oldest samples already in the ring are dropped
     * to make room: the newest audio is always the audio that matters.
     */
    fun write(samples: List<Float>, count: Int = samples.size) {
        if (count <= 0) return
        synchronized(lock) {
            // One slot is kept free so `head == tail` still means "empty".
            val room = capacity - 1
            val usable = minOf(count, room)
            // What does not fit is dropped from the *old* end: the newest
            // samples are the ones worth hearing.
            val waiting = (head - tail + capacity) % capacity
            val overflow = waiting + usable - room
            if (overflow > 0) {
                tail = (tail + overflow) % capacity
            }
            // When even an empty ring could not hold the batch, keep its tail —
            // again, the newest part.
            val first = count - usable
            for (index in first until count) {
                storage[head] = samples[index]
                head = (head + 1) % capacity
            }
        }
    }

    /**
     * Copy up to [count] samples into [out]; returns how many were copied.
     *
     * A short read is normal — the audio thread asks for a device period and
     * takes whatever has been produced so far.
     */
    fun read(out: FloatArray, count: Int): Int {
        synchronized(lock) {
            val waiting = (head - tail + capacity) % capacity
            val taken = minOf(count, waiting, out.size)
            for (index in 0 until taken) {
                out[index] = storage[tail]
                tail = (tail + 1) % capacity
            }
            return taken
        }
    }
}
