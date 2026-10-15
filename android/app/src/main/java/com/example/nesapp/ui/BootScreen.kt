package com.example.nesapp.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlin.random.Random

/**
 * The switch-on screen: static on the tube, then the mark.
 *
 * Short by design — a title screen that outstays its welcome is the first thing
 * a viewer learns to skip. The static is drawn, not filmed, so nothing here is a
 * media asset.
 */
@Composable
fun BootScreen() {
    var tick by remember { mutableIntStateOf(0) }

    LaunchedEffect(Unit) {
        while (true) {
            delay(STATIC_STEP_MILLIS)
            tick++
        }
    }

    Box(Modifier.fillMaxSize().background(Retro.Tube), contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            // A fresh seed per step: the noise has to change, or it reads as a
            // pattern rather than as a signal that is not there.
            val random = Random(tick)
            repeat(STATIC_SPECKLES) {
                val x = random.nextFloat() * size.width
                val y = random.nextFloat() * size.height
                val side = if (random.nextFloat() < 0.15f) 4f else 2f
                drawRect(
                    color = Color.White.copy(alpha = 0.10f + random.nextFloat() * 0.25f),
                    topLeft = Offset(x, y),
                    size = Size(side, side),
                )
            }
        }

        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Box(
                Modifier
                    .background(Retro.Red, RoundedCornerShape(2.dp))
                    .padding(horizontal = 24.dp, vertical = 12.dp),
            ) {
                RetroLabel("8-BIT", color = Retro.Cream, size = 44, bold = true, letterSpacing = 6f)
            }
            Spacer(Modifier.height(10.dp))
            RetroLabel("红白机 电视版", color = Retro.Gold, size = 20, bold = true, letterSpacing = 2f)
            Spacer(Modifier.height(26.dp))
            Box(
                Modifier
                    .fillMaxWidth(0.25f)
                    .height(10.dp)
                    .background(Retro.Ink, RoundedCornerShape(2.dp))
                    .padding(2.dp),
            ) {
                // A bar that fills as the static keeps ticking: the only thing
                // moving on screen while the catalogue is read.
                Box(
                    Modifier
                        .fillMaxWidth((tick % BAR_STEPS + 1).toFloat() / BAR_STEPS)
                        .fillMaxHeight()
                        .background(Retro.Gold, RoundedCornerShape(1.dp)),
                )
            }
            Spacer(Modifier.height(10.dp))
            RetroLabel("正在启动…", color = Retro.Ash, size = 13)
        }

        ScanlineOverlay(Modifier.fillMaxSize())
    }
}

/** How often the static is redrawn. */
private const val STATIC_STEP_MILLIS = 70L

/** How many specks of noise one redraw carries. */
private const val STATIC_SPECKLES = 900

/** How many ticks the boot bar takes to fill. */
private const val BAR_STEPS = 18
