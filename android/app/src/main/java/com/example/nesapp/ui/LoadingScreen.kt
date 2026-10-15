package com.example.nesapp.ui

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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.example.nesapp.Screen

/**
 * The cartridge going in: a title and a bar, over the black of a tube that has
 * not been fed yet.
 *
 * Progress is shown in bytes rather than a percentage when the server does not
 * announce a length, which is the normal case for a GitHub raw link — a bar that
 * sits at 0% for ten seconds is worse than a counter that moves.
 */
@Composable
fun LoadingScreen(screen: Screen.Loading) {
    Column(
        Modifier.fillMaxSize().background(Retro.Tube),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            Modifier
                .background(Retro.Red, RoundedCornerShape(2.dp))
                .padding(horizontal = 20.dp, vertical = 10.dp),
        ) {
            RetroLabel("插卡中", color = Retro.Cream, size = 22, bold = true, letterSpacing = 4f)
        }
        Spacer(Modifier.height(18.dp))
        RetroLabel(screen.game.name, color = Retro.Gold, size = 18, bold = true)
        Spacer(Modifier.height(20.dp))

        val fraction = if (screen.total > 0) {
            (screen.received.toFloat() / screen.total).coerceIn(0f, 1f)
        } else {
            0f
        }
        Box(
            Modifier
                .width(420.dp)
                .height(14.dp)
                .background(Retro.Ink, RoundedCornerShape(2.dp))
                .padding(2.dp),
        ) {
            Box(
                Modifier
                    .fillMaxWidth(fraction)
                    .fillMaxHeight()
                    .background(Retro.Gold, RoundedCornerShape(1.dp)),
            )
        }
        Spacer(Modifier.height(10.dp))
        RetroLabel(
            text = if (screen.total > 0) {
                "${kilobytes(screen.received)} / ${kilobytes(screen.total)} KB"
            } else {
                "已收到 ${kilobytes(screen.received)} KB"
            },
            color = Retro.Ash,
            size = 13,
            monospace = true,
        )
        Spacer(Modifier.height(28.dp))
        RetroLabel("BACK 取消", color = Retro.Ash, size = 12, monospace = true)
    }
}

/** Bytes as a whole number of kilobytes; a ROM is never small enough to matter. */
private fun kilobytes(bytes: Long): Long = bytes / 1024
