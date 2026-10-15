package com.example.nesapp

import java.nio.FloatBuffer
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
        ring.write(floatArrayOf(1f, 2f, 3f, 4f))

        assertEquals(4L, ring.available.toLong())
        val out = FloatArray(4)
        assertEquals(4L, ring.read(out, out.size).toLong())
        assertEquals(listOf(1f, 2f, 3f, 4f), out.toList())
    }

    @Test
    fun testTheConsumerTakesWhatIsThereAndNoMore() {
        val ring = AudioRing(8)
        ring.write(floatArrayOf(1f, 2f))
        val out = FloatArray(4)
        assertEquals(2L, ring.read(out, out.size).toLong())
        assertEquals("an empty ring reads empty, not garbage", 0L, ring.read(out, out.size).toLong())
    }

    @Test
    fun testTheRingWrapsAroundRatherThanFillingUp() {
        val ring = AudioRing(4)
        repeat(3) {
            ring.write(floatArrayOf(1f, 2f, 3f))
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
        ring.write(floatArrayOf(1f, 2f, 3f))
        ring.write(floatArrayOf(4f, 5f))

        val out = FloatArray(8)
        val read = ring.read(out, out.size)
        assertEquals(3L, read.toLong())
        assertEquals("the newest window survives", listOf(3f, 4f, 5f), out.take(read))
    }

    @Test
    fun testABatchLargerThanTheRingKeepsItsTail() {
        val ring = AudioRing(4)
        ring.write(floatArrayOf(1f, 2f, 3f, 4f, 5f, 6f))

        val out = FloatArray(8)
        val read = ring.read(out, out.size)
        assertEquals(listOf(4f, 5f, 6f), out.take(read))
    }

    @Test
    fun testFlushThrowsAwayTheBacklog() {
        val ring = AudioRing(8)
        ring.write(floatArrayOf(1f, 2f, 3f))
        ring.flush()

        assertEquals(0L, ring.available.toLong())
        assertEquals(0L, ring.read(FloatArray(4), 4).toLong())
    }

    @Test
    fun testADirectBatchIsTakenFromTheBuffersPosition() {
        // The shape the core's direct pull arrives in: a buffer this front end
        // keeps and rewinds, of which only the first `written` samples are real.
        val ring = AudioRing(8)
        val source = FloatBuffer.wrap(floatArrayOf(9f, 1f, 2f, 3f, 4f, 9f))
        source.position(1)
        ring.write(source, 4)

        val head = FloatArray(3)
        assertEquals(3L, ring.read(head, head.size).toLong())
        assertEquals(listOf(1f, 2f, 3f), head.toList())

        // And it wraps around the end of the ring like any other batch.
        ring.write(floatArrayOf(7f, 8f, 9f))
        val rest = FloatArray(8)
        val read = ring.read(rest, rest.size)
        assertEquals(listOf(4f, 7f, 8f, 9f), rest.take(read))
    }

    @Test
    fun testAShortReadCanBeToppedUpWhereItLeftOff() {
        // What the audio thread does when a device period arrives in two pieces:
        // the second read fills the buffer where the first one stopped.
        val ring = AudioRing(8)
        ring.write(floatArrayOf(1f, 2f))

        val period = FloatArray(4)
        assertEquals(2L, ring.read(period, period.size).toLong())

        ring.write(floatArrayOf(3f, 4f))
        assertEquals(2L, ring.read(period, period.size - 2, offset = 2).toLong())
        assertEquals(listOf(1f, 2f, 3f, 4f), period.toList())

        // An offset with no room behind it reads nothing rather than overflowing.
        assertEquals(0L, ring.read(period, 4, offset = 4).toLong())
    }

    @Test
    fun testADirectBatchBiggerThanTheRingKeepsItsTail() {
        val ring = AudioRing(4)
        val source = FloatBuffer.wrap(floatArrayOf(1f, 2f, 3f, 4f, 5f, 6f))
        ring.write(source, 6)

        // The whole batch is consumed, the part that did not fit included: the
        // dropped samples must not be handed back a second time.
        assertEquals("the batch is consumed, not replayed", 6, source.position())
        val out = FloatArray(8)
        val read = ring.read(out, out.size)
        assertEquals(listOf(4f, 5f, 6f), out.take(read))
    }
}
