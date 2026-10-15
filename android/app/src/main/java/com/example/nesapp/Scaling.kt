package com.example.nesapp

/**
 * How much of the surface the picture is drawn into, in pixels.
 *
 * Whole multiples by default: 256x240 scaled by anything other than an integer
 * gives pixels of uneven size, and an uneven grid of pixels shimmers as a game
 * scrolls — the one artifact a viewer notices immediately and cannot unsee. The
 * remainder becomes a black border, which is what a console on a 16:9 screen
 * looked like anyway.
 *
 * `stretch` is for a screen whose shape the viewer would rather fill, at the
 * cost of that shimmer.
 */
fun pictureViewport(
    surfaceWidth: Int,
    surfaceHeight: Int,
    pictureWidth: Int,
    pictureHeight: Int,
    stretch: Boolean,
): Pair<Int, Int> {
    if (surfaceWidth <= 0 || surfaceHeight <= 0) return pictureWidth to pictureHeight
    if (stretch) return surfaceWidth to surfaceHeight

    val scale = minOf(surfaceWidth / pictureWidth, surfaceHeight / pictureHeight).coerceAtLeast(1)
    return pictureWidth * scale to pictureHeight * scale
}

/**
 * How many output rows one source pixel row occupies.
 *
 * The scanline filter needs it to put a line on every pixel row rather than
 * every N screen rows: a filter at the wrong pitch reads as stripes, not as a
 * tube. Zero means the filter is off.
 */
fun scanlineRows(viewportHeight: Int, pictureHeight: Int, enabled: Boolean): Float =
    if (enabled && viewportHeight > 0 && pictureHeight > 0) {
        viewportHeight.toFloat() / pictureHeight
    } else {
        0f
    }
