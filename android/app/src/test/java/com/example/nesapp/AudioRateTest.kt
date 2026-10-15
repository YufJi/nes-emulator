package com.example.nesapp

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The audio rate that keeps a display-paced console fed: the core is asked for
 * more samples per emulated second when the screen runs the cartridge slow, and
 * for the device's own rate when it does not.
 */
class AudioRateTest {

    /** 60.000004 Hz: the single mode the reference television offers. */
    private val braviaHz = 60.000004

    /** 60.0985 Hz: NTSC. 50.0067 Hz: PAL. */
    private val ntscHz = 60.0985
    private val palHz = 50.0067

    @Test
    fun testAnNtscCartridgeOnASixtyHertzPanelAsksForMoreSamples() {
        // 48000 × 60.0985 / 60.000004 = 48078.8: the game runs 0.16% slow, so the
        // core owes the device 0.16% more samples per emulated second.
        assertEquals(48079L, coreSampleRate(48_000, ntscHz, braviaHz).toLong())
    }

    @Test
    fun testAPalCartridgeOnASixtyHertzPanelKeepsTheDeviceRate() {
        // 50 frames a second is slower than the screen, so emulated seconds are
        // real seconds and nothing needs rescaling.
        assertEquals(48_000L, coreSampleRate(48_000, palHz, braviaHz).toLong())
    }

    @Test
    fun testAPanelFasterThanTheCartridgeKeepsTheDeviceRate() {
        // A 120 Hz screen still shows an NTSC cartridge 60.0985 times a second.
        assertEquals(48_000L, coreSampleRate(48_000, ntscHz, 120.0).toLong())
    }

    @Test
    fun testSomethingTheHostCannotReadFallsBackToTheDeviceRate() {
        assertEquals(48_000L, coreSampleRate(48_000, ntscHz, 0.0).toLong())
        assertEquals(48_000L, coreSampleRate(48_000, ntscHz, Double.NaN).toLong())
        assertEquals(48_000L, coreSampleRate(48_000, 0.0, braviaHz).toLong())
        assertEquals(48_000L, coreSampleRate(48_000, Double.NaN, braviaHz).toLong())
    }
}
