package com.example.nesapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.foundation.focusable
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import com.example.nesapp.AppState
import com.example.nesapp.Overlay
import com.example.nesapp.SaveStore
import com.example.nesapp.Screen
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * The game, full screen, with whatever panel is over it.
 *
 * The picture is the surface the emulation thread uploads into. Nothing else is
 * composed while a game runs except the status strip and, when it is asked for,
 * one panel — a TV box has little enough to spare that the interface should cost
 * the frame it sits on top of nothing.
 */
@Composable
fun PlayingScreen(state: AppState, screen: Screen.Playing) {
    Box(Modifier.fillMaxSize().background(Retro.Tube)) {
        AndroidView(
            factory = { state.session.surface },
            modifier = Modifier.fillMaxSize(),
        )

        StatusBar(state, screen)

        when (val overlay = screen.overlay) {
            Overlay.None -> Unit
            Overlay.Menu -> MenuPanel(state, screen)
            is Overlay.Slots -> SlotPanel(state, screen, overlay.saving)
        }
    }
}

/**
 * The strip along the bottom: what is running, how fast, and whether it is still
 * running at all.
 *
 * Always there, at the owner's request: the frame rate is the one number worth
 * watching while a front end is being tuned, and a readout that has to be coaxed
 * out with a menu is a readout nobody looks at. It stays small and slightly
 * transparent so it does not compete with the picture.
 */
@Composable
private fun StatusBar(state: AppState, screen: Screen.Playing) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.BottomStart) {
        Box(
            Modifier
                .padding(12.dp)
                .background(Retro.Ink.copy(alpha = 0.82f), RoundedCornerShape(2.dp))
                .padding(horizontal = 12.dp, vertical = 6.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                RetroLabel(screen.game.name, color = Retro.Cream, size = 14, bold = true)
                Spacer(Modifier.width(12.dp))
                RetroLabel(
                    text = "${"%.1f".format(state.framesPerSecond)} FPS",
                    color = Retro.Gold,
                    size = 12,
                    monospace = true,
                )
                if (state.paused) {
                    Spacer(Modifier.width(12.dp))
                    RetroLabel("已暂停", color = Retro.Yellow, size = 12, bold = true)
                }
                if (state.cpuHalted) {
                    Spacer(Modifier.width(12.dp))
                    // A halted CPU still finishes frames, so the picture stops
                    // changing rather than disappearing; saying so beats silence.
                    RetroLabel("CPU 已挂起", color = Retro.Red, size = 12, bold = true)
                }
            }
        }
    }
}

/**
 * The in-game menu.
 *
 * Everything a session needs is here rather than a settings screen away: pause,
 * both save mechanisms, the two picture switches, reset, and the way out.
 */
@Composable
private fun MenuPanel(state: AppState, screen: Screen.Playing) {
    var index by remember { mutableIntStateOf(0) }

    val items = listOf(
        "继续游戏" to { app: AppState -> app.continueGame() },
        (if (state.paused) "恢复" else "暂停") to { app: AppState ->
            if (app.paused) app.resumeGame() else app.pauseGame()
        },
        "保存进度" to { app: AppState -> app.openOverlay(Overlay.Slots(saving = true)) },
        "读取进度" to { app: AppState -> app.openOverlay(Overlay.Slots(saving = false)) },
        (if (state.settings.muted) "声音：关" else "声音：开") to { app: AppState ->
            app.settings.muted = !app.settings.muted
        },
        (if (state.settings.crtFilter) "CRT 滤镜：开" else "CRT 滤镜：关") to { app: AppState ->
            app.settings.crtFilter = !app.settings.crtFilter
        },
        (if (state.settings.integerScale) "整数缩放：开" else "整数缩放：关") to { app: AppState ->
            app.settings.integerScale = !app.settings.integerScale
        },
        "重置卡带" to { app: AppState -> app.resetGame() },
        "返回游戏列表" to { app: AppState -> app.leaveGame() },
    )
    val requesters = remember(items.size) { List(items.size) { FocusRequester() } }

    LaunchedEffect(Unit) { runCatching { requesters[0].requestFocus() } }

    Box(
        Modifier.fillMaxSize().background(Retro.Tube.copy(alpha = 0.72f)),
        contentAlignment = Alignment.Center,
    ) {
        RetroPanel(Modifier.width(360.dp).height(430.dp)) {
            Column(Modifier.fillMaxSize().padding(18.dp)) {
                RetroLabel("卡带菜单", color = Retro.RedDeep, size = 18, bold = true)
                Spacer(Modifier.height(4.dp))
                RetroLabel(screen.game.name, color = Retro.Ash, size = 12)
                Spacer(Modifier.height(12.dp))

                items.forEachIndexed { itemIndex, (label, action) ->
                    val focused = index == itemIndex
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .height(34.dp)
                            .padding(vertical = 2.dp)
                            .focusRequester(requesters[itemIndex])
                            .focusable()
                            .onFocusChanged { if (it.isFocused) index = itemIndex }
                            .onKeyEvent { event ->
                                if (event.type != KeyEventType.KeyDown) return@onKeyEvent false
                                when (event.key) {
                                    Key.DirectionUp -> {
                                        focusMenuItem(requesters, index - 1, items.size) { index = it }
                                        true
                                    }

                                    Key.DirectionDown -> {
                                        focusMenuItem(requesters, index + 1, items.size) { index = it }
                                        true
                                    }

                                    Key.Enter, Key.DirectionCenter, Key.Spacebar, Key.ButtonA -> {
                                        action(state)
                                        true
                                    }

                                    else -> false
                                }
                            }
                            .background(
                                if (focused) Retro.Gold else Retro.Cream,
                                RoundedCornerShape(2.dp),
                            )
                            .padding(horizontal = 10.dp),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        RetroLabel(label, color = Retro.Ink, size = 15, bold = focused)
                    }
                }

                Spacer(Modifier.weight(1f))
                RetroLabel("BACK 关闭菜单", color = Retro.Ash, size = 11, monospace = true)
            }
        }
    }
}

/** The four save-state slots, used either to save into or to load from. */
@Composable
private fun SlotPanel(state: AppState, screen: Screen.Playing, saving: Boolean) {
    var index by remember { mutableIntStateOf(0) }
    val requesters = remember { List(SaveStore.SLOTS) { FocusRequester() } }
    val stamp = remember { SimpleDateFormat("MM-dd HH:mm", Locale.CHINA) }

    LaunchedEffect(Unit) { runCatching { requesters[0].requestFocus() } }

    Box(
        Modifier.fillMaxSize().background(Retro.Tube.copy(alpha = 0.72f)),
        contentAlignment = Alignment.Center,
    ) {
        RetroPanel(Modifier.width(520.dp).height(230.dp)) {
            Column(Modifier.fillMaxSize().padding(18.dp)) {
                RetroLabel(
                    if (saving) "保存到哪一档？" else "读取哪一档？",
                    color = Retro.RedDeep,
                    size = 18,
                    bold = true,
                )
                Spacer(Modifier.height(14.dp))

                Row(
                    Modifier.fillMaxWidth().weight(1f),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    repeat(SaveStore.SLOTS) { slot ->
                        val focused = index == slot
                        val time = state.slotTime(screen.game, slot)
                        Box(
                            Modifier
                                .weight(1f)
                                .fillMaxHeight()
                                .focusRequester(requesters[slot])
                                .focusable()
                                .onFocusChanged { if (it.isFocused) index = slot }
                                .onKeyEvent { event ->
                                    if (event.type != KeyEventType.KeyDown) return@onKeyEvent false
                                    when (event.key) {
                                        Key.DirectionLeft -> {
                                            val next = (slot - 1 + SaveStore.SLOTS) % SaveStore.SLOTS
                                            index = next
                                            runCatching { requesters[next].requestFocus() }
                                            true
                                        }

                                        Key.DirectionRight -> {
                                            val next = (slot + 1) % SaveStore.SLOTS
                                            index = next
                                            runCatching { requesters[next].requestFocus() }
                                            true
                                        }

                                        Key.Enter, Key.DirectionCenter, Key.Spacebar, Key.ButtonA -> {
                                            if (saving) state.saveTo(slot) else state.loadFrom(slot)
                                            true
                                        }

                                        else -> false
                                    }
                                }
                                .background(
                                    if (focused) Retro.Gold else Retro.Cream,
                                    RoundedCornerShape(2.dp),
                                )
                                .padding(8.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                Box(
                                    Modifier
                                        .fillMaxWidth()
                                        .height(50.dp)
                                        .background(Retro.RedDeep, RoundedCornerShape(1.dp)),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    RetroLabel("第 ${slot + 1} 档", color = Retro.Cream, size = 14, bold = true)
                                }
                                Spacer(Modifier.height(8.dp))
                                RetroLabel(
                                    text = time?.let { stamp.format(Date(it)) } ?: "空",
                                    color = Retro.Ink,
                                    size = 11,
                                    monospace = true,
                                )
                            }
                        }
                    }
                }

                Spacer(Modifier.height(12.dp))
                RetroLabel("左右 选档 · A 确定 · BACK 取消", color = Retro.Ash, size = 11, monospace = true)
            }
        }
    }
}

/** Move the menu's focus by [delta], wrapping around, and remember where it went. */
private fun focusMenuItem(
    requesters: List<FocusRequester>,
    target: Int,
    size: Int,
    remember: (Int) -> Unit,
) {
    val index = (target + size) % size
    remember(index)
    runCatching { requesters[index].requestFocus() }
}
