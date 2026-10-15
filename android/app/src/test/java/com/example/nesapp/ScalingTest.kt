package com.example.nesapp

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The placement of the picture: whole multiples, centred by the caller, and a
 * scanline pitch that matches the pixels rather than the screen.
 */
class ScalingTest {

    @Test
    fun testA1080pSurfaceTakesAWholeFourTimesScale() {
        // 4x240 = 960 of 1080: the largest whole multiple that fits.
        assertEquals(1024 to 960, pictureViewport(1920, 1080, 256, 240, stretch = false))
    }

    @Test
    fun testA720pSurfaceTakesThreeTimes() {
        assertEquals(768 to 720, pictureViewport(1280, 720, 256, 240, stretch = false))
    }

    @Test
    fun testStretchingFillsTheSurface() {
        assertEquals(1280 to 720, pictureViewport(1280, 720, 256, 240, stretch = true))
    }

    @Test
    fun testASurfaceSmallerThanThePictureStillDrawsOnePixelPerPixel() {
        // Clipping is worse than blurring here, but a scale of zero would show
        // nothing at all.
        assertEquals(256 to 240, pictureViewport(200, 200, 256, 240, stretch = false))
    }

    @Test
    fun testTheScanlinePitchFollowsTheScale() {
        assertEquals(4f, scanlineRows(960, 240, enabled = true), 0f)
        assertEquals(3f, scanlineRows(720, 240, enabled = true), 0f)
        assertEquals(0f, scanlineRows(960, 240, enabled = false), 0f)
    }

    @Test
    fun testASurfaceBeforeItsFirstLayoutDoesNotDivideByZero() {
        assertEquals(256 to 240, pictureViewport(0, 0, 256, 240, stretch = false))
        assertEquals(0f, scanlineRows(0, 240, enabled = true), 0f)
    }
}
