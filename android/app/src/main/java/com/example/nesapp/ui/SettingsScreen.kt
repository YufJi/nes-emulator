package com.example.nesapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
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
import com.example.nesapp.AppState
import com.example.nesapp.RegionChoice

/** How many focusable rows the list has; the navigation wraps around it. */
private const val SETTINGS_COUNT = 6

/**
 * The settings that belong to the machine rather than to one game.
 *
 * Every one of them is read live — the picture switches are read by the renderer
 * on each frame, the TV system when a cartridge is loaded — so a change here
 * takes effect without a restart. The TV system is the one exception: a
 * cartridge already running keeps the system it started with.
 */
@Composable
fun SettingsScreen(state: AppState) {
    var index by remember { mutableIntStateOf(0) }
    val requesters = remember { List(SETTINGS_COUNT) { FocusRequester() } }

    fun focus(next: Int) {
        val target = (next + SETTINGS_COUNT) % SETTINGS_COUNT
        index = target
        runCatching { requesters[target].requestFocus() }
    }

    LaunchedEffect(Unit) { runCatching { requesters[0].requestFocus() } }

    Column(Modifier.fillMaxSize().background(Retro.Tube)) {
        TitleBar("设置")

        Row(Modifier.fillMaxSize()) {
            Column(Modifier.width(470.dp).padding(20.dp)) {
                SettingRow(
                    label = "整数缩放",
                    value = if (state.settings.integerScale) "开" else "关",
                    focused = index == 0,
                    requester = requesters[0],
                    onFocused = { index = 0 },
                    onUp = { focus(index - 1) },
                    onDown = { focus(index + 1) },
                    onSelect = { state.settings.integerScale = !state.settings.integerScale },
                )
                SettingRow(
                    label = "CRT 滤镜",
                    value = if (state.settings.crtFilter) "开" else "关",
                    focused = index == 1,
                    requester = requesters[1],
                    onFocused = { index = 1 },
                    onUp = { focus(index - 1) },
                    onDown = { focus(index + 1) },
                    onSelect = { state.settings.crtFilter = !state.settings.crtFilter },
                )
                SettingRow(
                    label = "声音",
                    value = if (state.settings.muted) "关" else "开",
                    focused = index == 2,
                    requester = requesters[2],
                    onFocused = { index = 2 },
                    onUp = { focus(index - 1) },
                    onDown = { focus(index + 1) },
                    onSelect = { state.settings.muted = !state.settings.muted },
                )
                SettingRow(
                    label = "制式",
                    value = when (state.settings.region) {
                        RegionChoice.Auto -> "自动 · 跟随卡带"
                        RegionChoice.Ntsc -> "NTSC 60Hz"
                        RegionChoice.Pal -> "PAL 50Hz"
                    },
                    focused = index == 3,
                    requester = requesters[3],
                    onFocused = { index = 3 },
                    onUp = { focus(index - 1) },
                    onDown = { focus(index + 1) },
                    // Stepped rather than toggled: three values read better on a
                    // line than a switch that has to be pressed twice.
                    onLeft = { state.settings.region = state.settings.region.previous() },
                    onRight = { state.settings.region = state.settings.region.next() },
                )
                SettingRow(
                    label = "清空全部存档",
                    value = "›",
                    focused = index == 4,
                    requester = requesters[4],
                    onFocused = { index = 4 },
                    onUp = { focus(index - 1) },
                    onDown = { focus(index + 1) },
                    onSelect = { state.clearAllSaves() },
                )
                SettingRow(
                    label = "关于与免责声明",
                    value = "›",
                    focused = index == 5,
                    requester = requesters[5],
                    onFocused = { index = 5 },
                    onUp = { focus(index - 1) },
                    onDown = { focus(index + 1) },
                    onSelect = { state.openAbout() },
                )
            }

            KeyGuide()
        }

        HintBar("上下" to "选择", "左右" to "切换", "A" to "确定", "BACK" to "返回")
    }
}

/** One line of the list: a label on the left, its value on the right. */
@Composable
private fun SettingRow(
    label: String,
    value: String,
    focused: Boolean,
    requester: FocusRequester,
    onFocused: () -> Unit,
    onUp: () -> Unit,
    onDown: () -> Unit,
    onLeft: (() -> Unit)? = null,
    onRight: (() -> Unit)? = null,
    onSelect: (() -> Unit)? = null,
) {
    Box(
        Modifier
            .fillMaxWidth()
            .height(48.dp)
            .padding(vertical = 3.dp)
            .focusRequester(requester)
            .focusable()
            .onFocusChanged { if (it.isFocused) onFocused() }
            .onKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onKeyEvent false
                when (event.key) {
                    Key.DirectionUp -> {
                        onUp()
                        true
                    }

                    Key.DirectionDown -> {
                        onDown()
                        true
                    }

                    Key.DirectionLeft -> onLeft?.let {
                        it()
                        true
                    } ?: false

                    Key.DirectionRight -> onRight?.let {
                        it()
                        true
                    } ?: false

                    Key.Enter, Key.DirectionCenter, Key.Spacebar, Key.ButtonA -> onSelect?.let {
                        it()
                        true
                    } ?: false

                    else -> false
                }
            }
            .focusRing(focused)
            .background(if (focused) Retro.Gold else Retro.Cream, RoundedCornerShape(2.dp))
            .padding(horizontal = 14.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            RetroLabel(label, color = Retro.Ink, size = 16, bold = focused)
            RetroLabel(value, color = Retro.Ink, size = 14)
        }
    }
}

/**
 * The key guide: which pad button is which console button.
 *
 * Read-only, and deliberately next to the settings rather than inside a game:
 * "which button is SELECT" is a question you have while holding the pad, not one
 * worth opening a menu to answer.
 */
@Composable
private fun KeyGuide() {
    val rows = listOf(
        "方向键 / 摇杆" to "十字键",
        "A / 中键 / Y" to "A 键",
        "B / X" to "B 键",
        "START" to "START（暂停）",
        "SELECT" to "SELECT（选择）",
        "RB / R1 / R2" to "连发 A",
        "LB / L1 / L2" to "连发 B",
        "MENU" to "打开菜单",
        "BACK" to "菜单 / 返回",
    )

    Column(Modifier.padding(vertical = 26.dp, horizontal = 8.dp)) {
        RetroLabel("按键对照", color = Retro.Gold, size = 13, bold = true, letterSpacing = 1f)
        Spacer(Modifier.height(10.dp))
        rows.forEach { (pad, console) ->
            Row(Modifier.padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
                RetroLabel(
                    text = pad,
                    color = Retro.Cream,
                    size = 13,
                    monospace = true,
                    modifier = Modifier.width(160.dp),
                )
                RetroLabel(console, color = Retro.Cream, size = 13)
            }
        }
        Spacer(Modifier.height(12.dp))
        RetroLabel(
            text = "第二只手柄自动接到 2P，遥控器固定在 1P。",
            color = Retro.Ash,
            size = 12,
            maxLines = 2,
            modifier = Modifier.width(340.dp),
        )
    }
}

/** The red header bar every full-screen page carries. */
@Composable
fun TitleBar(title: String) {
    Box(
        Modifier
            .fillMaxWidth()
            .background(Retro.Red)
            .padding(horizontal = 20.dp, vertical = 14.dp),
    ) {
        RetroLabel(title, color = Retro.Cream, size = 20, bold = true)
    }
}

/** The next TV system in the list, wrapping around. */
private fun RegionChoice.next(): RegionChoice =
    RegionChoice.entries[(ordinal + 1) % RegionChoice.entries.size]

/** The previous TV system in the list, wrapping around. */
private fun RegionChoice.previous(): RegionChoice =
    RegionChoice.entries[(ordinal - 1 + RegionChoice.entries.size) % RegionChoice.entries.size]
