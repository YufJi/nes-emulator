package com.example.nesapp

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The handshake that keeps the console one frame behind the display: one frame in
 * flight at a time, and a presentation — not a timer — is what releases the next.
 */
class FrameGateTest {

    @Test
    fun testAFrameNeedsAPresentationBeforeTheNextOne() {
        val gate = FrameGate()

        assertTrue("the first frame may run", gate.tryBeginProduce())
        assertFalse("a second one may not", gate.tryBeginProduce())
        assertTrue("the presentation releases it", gate.onPresented())
        assertTrue("and the next frame may run", gate.tryBeginProduce())
    }

    @Test
    fun testAPresentationWithoutAFrameInFlightGrantsNothing() {
        val gate = FrameGate()

        // A surface that is recreated draws once on its own, with no frame behind
        // it: that must not hand the console an extra frame. The same rule makes a
        // picture drawn twice release its frame only once.
        assertFalse("nothing was in flight to release", gate.onPresented())
        assertTrue(gate.tryBeginProduce())
        assertTrue("the one frame in flight is released", gate.onPresented())
        assertFalse("not twice", gate.onPresented())
    }

    @Test
    fun testAResumedSessionStartsWithItsPermitBack() {
        val gate = FrameGate()

        assertTrue(gate.tryBeginProduce())
        gate.reset()
        assertTrue("a paused session has nothing in flight", gate.tryBeginProduce())
    }
}
