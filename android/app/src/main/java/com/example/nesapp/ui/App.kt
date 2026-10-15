package com.example.nesapp.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.example.nesapp.AppState
import com.example.nesapp.Overlay
import com.example.nesapp.Screen
import kotlinx.coroutines.delay

/**
 * The app: one screen at a time, and the two keys that mean "up a level".
 *
 * There is no navigation library here on purpose. The whole app is five screens
 * with no arguments and no deep links, and a TV viewer's mental model of them is
 * a stack of one — so the state is a single value and BACK walks it.
 */
@Composable
fun NesApp(state: AppState) {
    val screen = state.screen

    BackHandler(enabled = screen !is Screen.Boot && screen !is Screen.Home) {
        when (screen) {
            is Screen.Playing ->
                if (screen.overlay is Overlay.None) state.openOverlay(Overlay.Menu) else state.closeOverlay()

            is Screen.Loading -> state.cancelDownload()
            Screen.Settings, Screen.About -> state.goHome()
            else -> Unit
        }
    }

    Box(Modifier.fillMaxSize().background(Retro.Tube)) {
        when (screen) {
            Screen.Boot -> BootScreen()
            Screen.Home -> HomeScreen(state)
            is Screen.Loading -> LoadingScreen(screen)
            is Screen.Playing -> PlayingScreen(state, screen)
            Screen.Settings -> SettingsScreen(state)
            Screen.About -> AboutScreen()
        }

        Notice(state)
    }
}

/** The one-line message at the bottom: a failure, a confirmation, a hint. */
@Composable
private fun Notice(state: AppState) {
    val notice = state.notice ?: return

    LaunchedEffect(notice) {
        delay(NOTICE_MILLIS)
        state.dismissNotice()
    }

    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.BottomCenter) {
        Box(
            Modifier
                .padding(bottom = 54.dp)
                .background(Retro.Ink.copy(alpha = 0.9f), RoundedCornerShape(2.dp))
                .padding(horizontal = 16.dp, vertical = 8.dp),
        ) {
            RetroLabel(notice, color = Retro.Cream, size = 14, maxLines = 2)
        }
    }
}

/** How long a notice stays up before it takes itself away. */
private const val NOTICE_MILLIS = 3_000L
