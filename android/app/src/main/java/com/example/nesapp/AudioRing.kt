package com.example.nesapp

import java.nio.FloatBuffer

/**
 * The float buffer between the emulation thread and the audio thread.
 *
 * The core is deliberately single-owner (`docs/architecture.md` §10.5): the
 * emulation thread pulls samples with `fillAudioInto` and hands them over here,
 * and the audio thread never touches the console. The producer never blocks — if
 * the consumer falls behind, the oldest samples go, because stale audio is worse
 * than a gap.
 *
 * Both ends copy in bulk, at most two pieces each because the ring wraps. The
 * critical section is a `System.arraycopy` under a monitor: fine at this size,
 * where an atomic head/tail would give up the JVM's own bounds checks for the
 * sake of samples the platform is going to copy anyway.
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
    fun write(samples: FloatArray, count: Int = samples.size) {
        write(FloatBuffer.wrap(samples), count)
    }

    /**
     * Queue [count] samples (interleaved stereo) from [source], starting at its
     * current position — the shape the core's direct audio pull arrives in.
     *
     * The position advances; the caller rewinds to read a fresh batch.
     */
    fun write(source: FloatBuffer, count: Int = source.remaining()) {
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
            source.position(source.position() + (count - usable))
            val start = head
            val first = minOf(usable, capacity - start)
            source.get(storage, start, first)
            if (usable > first) {
                source.get(storage, 0, usable - first)
            }
            head = (start + usable) % capacity
        }
    }

    /**
     * Copy up to [count] samples into [out] at [offset]; returns how many were
     * copied.
     *
     * A short read is normal — the audio thread asks for a device period and
     * takes whatever has been produced so far — and the offset is what lets it
     * come back for the rest of that period and find the buffer where it left it.
     */
    fun read(out: FloatArray, count: Int, offset: Int = 0): Int {
        synchronized(lock) {
            val room = out.size - offset
            if (room <= 0) return 0
            val waiting = (head - tail + capacity) % capacity
            val taken = minOf(count, waiting, room)
            val start = tail
            val first = minOf(taken, capacity - start)
            System.arraycopy(storage, start, out, offset, first)
            if (taken > first) {
                System.arraycopy(storage, 0, out, offset + first, taken - first)
            }
            tail = (start + taken) % capacity
            return taken
        }
    }
}
