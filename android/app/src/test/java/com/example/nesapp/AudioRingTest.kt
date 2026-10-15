package com.example.nesapp

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The ring between the emulation thread and the audio thread: it must never
 * block the producer, and what it drops when it overflows must be the oldest
 * samples rather than the newest.
 */
class AudioRingTest {

    @Test
    fun testWhatGoesInComesOutInOrder() {
        val ring = AudioRing(8)
        ring.write(listOf(1f, 2f, 3f, 4f))

        assertEquals(4L, ring.available.toLong())
        val out = FloatArray(4)
        assertEquals(4L, ring.read(out, out.size).toLong())
        assertEquals(listOf(1f, 2f, 3f, 4f), out.toList())
    }

    @Test
    fun testTheConsumerTakesWhatIsThereAndNoMore() {
        val ring = AudioRing(8)
        ring.write(listOf(1f, 2f))
        val out = FloatArray(4)
        assertEquals(2L, ring.read(out, out.size).toLong())
        assertEquals("an empty ring reads empty, not garbage", 0L, ring.read(out, out.size).toLong())
    }

    @Test
    fun testTheRingWrapsAroundRatherThanFillingUp() {
        val ring = AudioRing(4)
        repeat(3) {
            ring.write(listOf(1f, 2f, 3f))
            val out = FloatArray(3)
            assertEquals(3L, ring.read(out, out.size).toLong())
            assertEquals(listOf(1f, 2f, 3f), out.toList())
        }
    }

    @Test
    fun testOverflowDropsTheOldestSamples() {
        // Four slots hold three samples: the fourth write would be the one that
        // pretends the ring is a queue, and it is not.
        val ring = AudioRing(4)
        ring.write(listOf(1f, 2f, 3f))
        ring.write(listOf(4f, 5f))

        val out = FloatArray(8)
        val read = ring.read(out, out.size)
        assertEquals(3L, read.toLong())
        assertEquals("the newest window survives", listOf(3f, 4f, 5f), out.take(read))
    }

    @Test
    fun testABatchLargerThanTheRingKeepsItsTail() {
        val ring = AudioRing(4)
        ring.write(listOf(1f, 2f, 3f, 4f, 5f, 6f))

        val out = FloatArray(8)
        val read = ring.read(out, out.size)
        assertEquals(listOf(4f, 5f, 6f), out.take(read))
    }

    @Test
    fun testFlushThrowsAwayTheBacklog() {
        val ring = AudioRing(8)
        ring.write(listOf(1f, 2f, 3f))
        ring.flush()

        assertEquals(0L, ring.available.toLong())
        assertEquals(0L, ring.read(FloatArray(4), 4).toLong())
    }
}
