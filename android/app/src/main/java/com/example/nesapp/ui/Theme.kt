package com.example.nesapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The palette of a 1990s console: moulded red plastic, cream labels, gold
 * cartridge shells and the black of a switched-off tube.
 *
 * Deliberately not Material: this interface is a piece of hardware seen from the
 * sofa, so it is built from thick outlines and hard shadows rather than
 * elevation, and corners are nearly square. Nothing here imitates a particular
 * brand — it is the look of the era, which is what the front end is going for.
 */
object Retro {
    val Red = Color(0xFFB3232B)
    val RedDeep = Color(0xFF8A2B22)
    val Cream = Color(0xFFF2E4C8)
    val Ink = Color(0xFF1A1614)
    val Gold = Color(0xFFE0B23C)
    val Yellow = Color(0xFFFFD24A)
    val Green = Color(0xFF2C6E3C)
    val Tube = Color(0xFF14100E)
    val Ash = Color(0xFF6B6259)

    /** How thick the outlines are, everywhere; one number keeps it consistent. */
    val Edge: Dp = 3.dp

    /** How far the hard shadow sits below a raised panel. */
    val Shadow: Dp = 4.dp
}

/** Text in the interface's own voice, with the size and weight the caller wants. */
@Composable
fun RetroLabel(
    text: String,
    color: Color = Retro.Ink,
    size: Int = 16,
    bold: Boolean = false,
    monospace: Boolean = false,
    letterSpacing: Float = 0f,
    maxLines: Int = 1,
    align: TextAlign? = null,
    modifier: Modifier = Modifier,
) {
    Text(
        text = text,
        modifier = modifier,
        maxLines = maxLines,
        overflow = TextOverflow.Ellipsis,
        textAlign = align,
        style = TextStyle(
            color = color,
            fontSize = size.sp,
            fontWeight = if (bold) FontWeight.Bold else FontWeight.Medium,
            // Monospace for numbers and short readouts: the fixed advance reads
            // as machine output, which is the closest thing to a dot-matrix look
            // that ships without a font file.
            fontFamily = if (monospace) FontFamily.Monospace else FontFamily.Default,
            letterSpacing = letterSpacing.sp,
        ),
    )
}

/**
 * A moulded panel: cream face, thick ink outline, hard shadow under it.
 *
 * The shadow is a second box behind the panel rather than a blur, because the
 * hardware this imitates had no blur anywhere.
 */
@Composable
fun RetroPanel(
    modifier: Modifier = Modifier,
    face: Color = Retro.Cream,
    edge: Color = Retro.Ink,
    raised: Boolean = true,
    content: @Composable () -> Unit,
) {
    Box(modifier) {
        if (raised) {
            Box(
                Modifier
                    .matchParentSize()
                    .offset(y = Retro.Shadow)
                    .background(edge, RoundedCornerShape(2.dp)),
            )
        }
        Box(
            Modifier
                .matchParentSize()
                .background(face, RoundedCornerShape(2.dp))
                .border(Retro.Edge, edge, RoundedCornerShape(2.dp)),
        )
        content()
    }
}

/**
 * The focus treatment: a gold outline and a small nudge.
 *
 * Focus has to be unmistakable from three metres away, and on a TV it is the
 * only cursor there is — so it changes both colour and position, not just
 * colour.
 */
fun Modifier.focusRing(focused: Boolean, inset: Dp = Retro.Edge): Modifier =
    if (focused) {
        this
            .border(inset, Retro.Yellow, RoundedCornerShape(2.dp))
            .offset(y = (-2).dp)
    } else {
        this
    }

/**
 * Horizontal lines over everything, the way a shadow mask shows through.
 *
 * A cheap single-pass trick, and enough to sell the illusion at TV distance.
 */
@Composable
fun ScanlineOverlay(
    modifier: Modifier = Modifier,
    color: Color = Color(0x14000000),
    spacing: Dp = 3.dp,
) {
    Box(
        modifier
            .drawBehind {
                var y = 0f
                val step = spacing.toPx()
                while (y < size.height) {
                    drawLine(
                        color = color,
                        start = Offset(0f, y),
                        end = Offset(size.width, y),
                        strokeWidth = 1f,
                    )
                    y += step
                }
            },
    )
}

/** A row of "键 说明" hints, the footer every TV interface needs. */
@Composable
fun HintBar(vararg hints: Pair<String, String>) {
    Row(
        Modifier
            .fillMaxWidth()
            .background(Retro.Ink)
            .padding(horizontal = 16.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        hints.forEach { (key, what) ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier
                        .background(Retro.Gold, RoundedCornerShape(2.dp))
                        .padding(horizontal = 6.dp, vertical = 1.dp),
                ) {
                    RetroLabel(key, color = Retro.Ink, size = 11, bold = true, monospace = true)
                }
                Spacer(Modifier.width(6.dp))
                RetroLabel(what, color = Retro.Cream, size = 12)
            }
        }
    }
}

/** A thin frame that separates sections without a Material divider. */
@Composable
fun InkRule(modifier: Modifier = Modifier, color: Color = Retro.Ink) {
    Box(modifier.fillMaxWidth().height(2.dp).background(color, RoundedCornerShape(1.dp)))
}
