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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.example.nesapp.AppState
import com.example.nesapp.GameEntry
import com.example.nesapp.Recents
import com.example.nesapp.pageAfter
import com.example.nesapp.pageCountOf
import com.example.nesapp.pagerAvailable
import com.example.nesapp.preferredPagerIndex

/** Cartridges per page: three rows of four, which is what a 16:9 screen holds. */
private const val COLUMNS = 4
private const val ROWS = 3
private const val PER_PAGE = COLUMNS * ROWS

private const val PAGE_BUTTONS = 2

/** What holds focus on this screen. */
private enum class Zone { Settings, Recents, Cartridges, Pager }

/**
 * The cartridge wall.
 *
 * Everything is reachable with the D-pad and nothing needs a mouse: the focus
 * model is an explicit index per zone rather than Compose's two-dimensional
 * search, because on a TV the difference between "the obvious next cell" and
 * "whatever the search picked" is the difference between usable and not.
 */
@Composable
fun HomeScreen(state: AppState) {
    val games = state.games
    var page by remember { mutableStateOf(0) }
    var cell by remember { mutableStateOf(0) }
    var recentIndex by remember { mutableStateOf(0) }
    var zone by remember { mutableStateOf(Zone.Cartridges) }

    /** Which page button holds focus: 0 goes back, 1 goes forward. */
    var pagerIndex by remember { mutableStateOf(1) }

    val pageCount = pageCountOf(games?.size ?: 0, PER_PAGE)
    val pageGames = remember(games, page) {
        games?.drop(page * PER_PAGE)?.take(PER_PAGE).orEmpty()
    }
    val recentGames = remember(state.recents.names, games) {
        val byName = games?.associateBy { it.name }.orEmpty()
        state.recents.names.mapNotNull { byName[it] }
    }

    val settingsRequester = remember { FocusRequester() }
    val recentRequesters = remember { List(Recents.LIMIT) { FocusRequester() } }
    val cellRequesters = remember { List(PER_PAGE) { FocusRequester() } }
    val pageRequesters = remember { List(PAGE_BUTTONS) { FocusRequester() } }

    /**
     * Move focus to the page buttons.
     *
     * On a list this long the buttons are the point of the bottom row, so the
     * D-pad can reach them; a single-page result has nothing for them to do and
     * leaves focus where it is.
     */
    fun focusPager() {
        val target = when {
            pagerAvailable(pagerIndex, page, pageCount) -> pagerIndex
            pagerAvailable(1 - pagerIndex, page, pageCount) -> 1 - pagerIndex
            else -> return
        }
        zone = Zone.Pager
        pagerIndex = target
        runCatching { pageRequesters[target].requestFocus() }
    }

    Column(Modifier.fillMaxSize().background(Retro.Tube)) {
        Header(
            count = games?.size,
            requester = settingsRequester,
            focused = zone == Zone.Settings,
            onFocused = { zone = Zone.Settings },
            onDown = { zone = if (recentGames.isEmpty()) Zone.Cartridges else Zone.Recents },
            onMoveFocus = {
                if (recentGames.isEmpty()) {
                    runCatching { cellRequesters[cell.coerceIn(0, lastIndex(pageGames.size))].requestFocus() }
                } else {
                    runCatching { recentRequesters[recentIndex.coerceIn(0, recentGames.lastIndex)].requestFocus() }
                }
            },
            onSelect = state::openSettings,
        )

        Box(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 20.dp)) {
            when {
                state.catalogError != null -> CatalogError(state)
                games == null -> ReadingCatalog()
                else -> Column(Modifier.fillMaxSize()) {
                    if (recentGames.isNotEmpty()) {
                        SectionLabel("最近玩过")
                        Row(
                            Modifier.fillMaxWidth().padding(bottom = 10.dp),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            recentGames.forEachIndexed { index, game ->
                                RecentCard(
                                    game = game,
                                    focused = zone == Zone.Recents && recentIndex == index,
                                    requester = recentRequesters[index],
                                    onFocused = {
                                        zone = Zone.Recents
                                        recentIndex = index
                                    },
                                    onLeft = {
                                        if (index > 0) {
                                            recentIndex = index - 1
                                            runCatching { recentRequesters[index - 1].requestFocus() }
                                        }
                                    },
                                    onRight = {
                                        if (index < recentGames.lastIndex) {
                                            recentIndex = index + 1
                                            runCatching { recentRequesters[index + 1].requestFocus() }
                                        }
                                    },
                                    onDown = {
                                        zone = Zone.Cartridges
                                        runCatching { cellRequesters[cell.coerceIn(0, lastIndex(pageGames.size))].requestFocus() }
                                    },
                                    onUp = {
                                        zone = Zone.Settings
                                        runCatching { settingsRequester.requestFocus() }
                                    },
                                    onSelect = { state.openGame(game) },
                                )
                            }
                        }
                    }

                    SectionLabel("游戏列表")
                    for (row in 0 until ROWS) {
                        Row(
                            Modifier.fillMaxWidth().weight(1f),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            for (column in 0 until COLUMNS) {
                                val index = row * COLUMNS + column
                                val game = pageGames.getOrNull(index)
                                Box(Modifier.weight(1f)) {
                                    if (game != null) {
                                        GameCard(
                                            game = game,
                                            focused = zone == Zone.Cartridges && cell == index,
                                            requester = cellRequesters[index],
                                            onFocused = {
                                                zone = Zone.Cartridges
                                                cell = index
                                            },
                                            onMove = { target ->
                                                val next = move(index, target)
                                                when {
                                                    next != null && next in pageGames.indices -> {
                                                        cell = next
                                                        runCatching { cellRequesters[next].requestFocus() }
                                                    }

                                                    // Off the bottom of the wall: the
                                                    // page buttons are what is there.
                                                    target == Move.Down -> focusPager()

                                                    target == Move.Up -> {
                                                        if (recentGames.isEmpty()) {
                                                            zone = Zone.Settings
                                                            runCatching { settingsRequester.requestFocus() }
                                                        } else {
                                                            zone = Zone.Recents
                                                            runCatching {
                                                                recentRequesters[recentIndex.coerceIn(0, recentGames.lastIndex)]
                                                                    .requestFocus()
                                                            }
                                                        }
                                                    }
                                                }
                                            },
                                            onPage = { step ->
                                                if (step < 0 && page > 0) page--
                                                if (step > 0 && page < pageCount - 1) page++
                                                cell = index
                                                // The new page composes after this
                                                // frame; ask for focus again then.
                                            },
                                            onSelect = { state.openGame(game) },
                                        )
                                    }
                                }
                            }
                        }
                        Spacer(Modifier.height(12.dp))
                    }

                    PagerRow(
                        page = page,
                        pageCount = pageCount,
                        total = games.size,
                        focusedIndex = pagerIndex,
                        inPager = zone == Zone.Pager,
                        requesters = pageRequesters,
                        onFocused = { index ->
                            zone = Zone.Pager
                            pagerIndex = index
                        },
                        onMoveFocus = { index ->
                            pagerIndex = index
                            runCatching { pageRequesters[index].requestFocus() }
                        },
                        onStep = { step -> page = pageAfter(page, step, pageCount) },
                        onUp = {
                            zone = Zone.Cartridges
                            runCatching {
                                cellRequesters[cell.coerceIn(0, lastIndex(pageGames.size))].requestFocus()
                            }
                        },
                    )
                }
            }
        }

        HintBar(
            "方向键" to "选择",
            "A" to "开始",
            "PAGE" to "翻页",
            "MENU" to "设置",
            "BACK" to "退出",
        )
    }

    // Paging replaces every cell on screen, so whatever held focus has to be
    // resolved again once the new page exists — and the same effect covers the
    // first page, which is why the list arriving also lands focus on cell one.
    LaunchedEffect(page, pageGames.size) {
        when (zone) {
            Zone.Pager -> {
                // The button that was pressed may have run out of pages under it.
                val target = preferredPagerIndex(page, pageCount)
                    .takeIf { pagerAvailable(it, page, pageCount) }
                    ?: (1 - preferredPagerIndex(page, pageCount))
                pagerIndex = target
                runCatching { pageRequesters[target].requestFocus() }
            }

            else -> runCatching {
                cellRequesters[cell.coerceIn(0, lastIndex(pageGames.size))].requestFocus()
            }
        }
    }
}

/** The row above the wall: the mark, the count, and the way into settings. */
@Composable
private fun Header(
    count: Int?,
    requester: FocusRequester,
    focused: Boolean,
    onFocused: () -> Unit,
    onDown: () -> Unit,
    onMoveFocus: () -> Unit,
    onSelect: () -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .background(Retro.Red)
            .padding(horizontal = 20.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .background(Retro.Cream, RoundedCornerShape(2.dp))
                .padding(horizontal = 10.dp, vertical = 4.dp),
        ) {
            RetroLabel("8-BIT", color = Retro.RedDeep, size = 20, bold = true, letterSpacing = 2f)
        }
        Spacer(Modifier.width(14.dp))
        Column {
            RetroLabel("红白机 电视版", color = Retro.Cream, size = 20, bold = true)
            RetroLabel(
                if (count == null) "游戏列表读取中…" else "卡带 ${count} 款 · 全部来自网络，本机不存 ROM",
                color = Retro.Cream.copy(alpha = 0.75f),
                size = 12,
            )
        }
        Spacer(Modifier.weight(1f))
        Box(
            Modifier
                .focusRequester(requester)
                .focusable()
                .onFocusChanged { if (it.isFocused) onFocused() }
                .onKeyEvent { event ->
                    if (event.type != KeyEventType.KeyDown) return@onKeyEvent false
                    when (event.key) {
                        Key.DirectionDown -> {
                            onDown()
                            onMoveFocus()
                            true
                        }

                        Key.Enter, Key.DirectionCenter, Key.Spacebar -> {
                            onSelect()
                            true
                        }

                        else -> false
                    }
                }
                .focusRing(focused)
                .background(Retro.Cream, RoundedCornerShape(2.dp))
                .padding(horizontal = 14.dp, vertical = 6.dp),
        ) {
            RetroLabel("设置", color = Retro.Ink, size = 14, bold = true)
        }
    }
}

@Composable
private fun SectionLabel(text: String) {
    RetroLabel(text, color = Retro.Gold, size = 13, bold = true, letterSpacing = 1f)
    Spacer(Modifier.height(6.dp))
}

/** One cell of the wall: a cartridge with a label. */
@Composable
private fun GameCard(
    game: GameEntry,
    focused: Boolean,
    requester: FocusRequester,
    onFocused: () -> Unit,
    onMove: (Move) -> Unit,
    onPage: (Int) -> Unit,
    onSelect: () -> Unit,
) {
    Box(
        Modifier
            .fillMaxSize()
            .focusRequester(requester)
            .focusable()
            .onFocusChanged { if (it.isFocused) onFocused() }
            .onKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onKeyEvent false
                // Holding a direction pages through the list: 932 cartridges are
                // too many to walk four at a time.
                val held = event.nativeKeyEvent.repeatCount
                when (event.key) {
                    Key.DirectionLeft -> {
                        if (held > 0 && held % FastPage.REPEATS == 0) onPage(-1) else onMove(Move.Left)
                        true
                    }

                    Key.DirectionRight -> {
                        if (held > 0 && held % FastPage.REPEATS == 0) onPage(1) else onMove(Move.Right)
                        true
                    }

                    Key.DirectionUp -> {
                        if (held > 0 && held % FastPage.REPEATS == 0) onPage(-1) else onMove(Move.Up)
                        true
                    }

                    Key.DirectionDown -> {
                        if (held > 0 && held % FastPage.REPEATS == 0) onPage(1) else onMove(Move.Down)
                        true
                    }

                    Key.PageUp, Key.ChannelUp -> {
                        onPage(-1)
                        true
                    }

                    Key.PageDown, Key.ChannelDown -> {
                        onPage(1)
                        true
                    }

                    Key.Enter, Key.DirectionCenter, Key.Spacebar, Key.ButtonA -> {
                        onSelect()
                        true
                    }

                    else -> false
                }
            }
            .focusRing(focused)
            .background(if (focused) Retro.Gold else Retro.RedDeep, RoundedCornerShape(2.dp))
            .padding(4.dp),
    ) {
        Column(Modifier.fillMaxSize()) {
            // The ridge across the top of a cartridge shell.
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(10.dp)
                    .background(Retro.Ink.copy(alpha = 0.55f), RoundedCornerShape(1.dp)),
            )
            Spacer(Modifier.height(4.dp))
            Box(
                Modifier
                    .fillMaxWidth()
                    .weight(1f)
                    .background(Retro.Cream, RoundedCornerShape(1.dp))
                    .padding(6.dp),
                contentAlignment = Alignment.Center,
            ) {
                RetroLabel(
                    text = game.name,
                    color = Retro.Ink,
                    size = 13,
                    bold = focused,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}

/**
 * The bottom row: back a page, the counter, forward a page.
 *
 * Buttons rather than a hint, because paging 78 pages by holding a direction is
 * a guess at how long to hold: the two ends of the list are where this screen is
 * easiest to get lost in.
 */
@Composable
private fun PagerRow(
    page: Int,
    pageCount: Int,
    total: Int,
    focusedIndex: Int,
    inPager: Boolean,
    requesters: List<FocusRequester>,
    onFocused: (Int) -> Unit,
    onMoveFocus: (Int) -> Unit,
    onStep: (Int) -> Unit,
    onUp: () -> Unit,
) {
    val back = pagerAvailable(0, page, pageCount)
    val forward = pagerAvailable(1, page, pageCount)

    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (pageCount > 1) {
            PageButton(
                label = "‹ 上一页",
                enabled = back,
                focused = inPager && focusedIndex == 0,
                requester = requesters[0],
                onFocused = { onFocused(0) },
                onStep = { onStep(-1) },
                onUp = onUp,
                onRight = if (forward) ({ onMoveFocus(1) }) else null,
            )
            RetroLabel(
                text = "第 ${page + 1} / $pageCount 页",
                color = Retro.Cream,
                size = 13,
                monospace = true,
            )
            PageButton(
                label = "下一页 ›",
                enabled = forward,
                focused = inPager && focusedIndex == 1,
                requester = requesters[1],
                onFocused = { onFocused(1) },
                onStep = { onStep(1) },
                onUp = onUp,
                onLeft = if (back) ({ onMoveFocus(0) }) else null,
            )
        }
        Spacer(Modifier.weight(1f))
        RetroLabel("共 $total 款", color = Retro.Ash, size = 13, monospace = true)
    }
}

/** One page button: moulded, dimmed when there is no page that way. */
@Composable
private fun PageButton(
    label: String,
    enabled: Boolean,
    focused: Boolean,
    requester: FocusRequester,
    onFocused: () -> Unit,
    onStep: () -> Unit,
    onUp: () -> Unit,
    onLeft: (() -> Unit)? = null,
    onRight: (() -> Unit)? = null,
) {
    Box(
        Modifier
            .height(30.dp)
            .focusRequester(requester)
            .focusable(enabled = enabled)
            .onFocusChanged { if (it.isFocused) onFocused() }
            .onKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onKeyEvent false
                when (event.key) {
                    Key.DirectionLeft -> onLeft?.let {
                        it()
                        true
                    } ?: false

                    Key.DirectionRight -> onRight?.let {
                        it()
                        true
                    } ?: false

                    Key.DirectionUp -> {
                        onUp()
                        true
                    }

                    Key.Enter, Key.DirectionCenter, Key.Spacebar, Key.ButtonA -> {
                        onStep()
                        true
                    }

                    else -> false
                }
            }
            .focusRing(focused)
            .background(
                if (!enabled) Retro.Ink else if (focused) Retro.Gold else Retro.Cream,
                RoundedCornerShape(2.dp),
            )
            .padding(horizontal = 14.dp),
        contentAlignment = Alignment.Center,
    ) {
        RetroLabel(
            text = label,
            color = if (enabled) Retro.Ink else Retro.Ash,
            size = 13,
            bold = focused,
        )
    }
}

/** One of the recently played games, as a smaller label. */
@Composable
private fun RecentCard(
    game: GameEntry,
    focused: Boolean,
    requester: FocusRequester,
    onFocused: () -> Unit,
    onLeft: () -> Unit,
    onRight: () -> Unit,
    onDown: () -> Unit,
    onUp: () -> Unit,
    onSelect: () -> Unit,
) {
    Box(
        Modifier
            .width(150.dp)
            .height(44.dp)
            .focusRequester(requester)
            .focusable()
            .onFocusChanged { if (it.isFocused) onFocused() }
            .onKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onKeyEvent false
                when (event.key) {
                    Key.DirectionLeft -> {
                        onLeft()
                        true
                    }

                    Key.DirectionRight -> {
                        onRight()
                        true
                    }

                    Key.DirectionDown -> {
                        onDown()
                        true
                    }

                    Key.DirectionUp -> {
                        onUp()
                        true
                    }

                    Key.Enter, Key.DirectionCenter, Key.Spacebar, Key.ButtonA -> {
                        onSelect()
                        true
                    }

                    else -> false
                }
            }
            .focusRing(focused)
            .background(if (focused) Retro.Gold else Retro.Ink, RoundedCornerShape(2.dp))
            .padding(horizontal = 8.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        RetroLabel(
            text = game.name,
            color = if (focused) Retro.Ink else Retro.Cream,
            size = 12,
            maxLines = 2,
        )
    }
}

@Composable
private fun ReadingCatalog() {
    Column(
        Modifier.fillMaxSize().padding(20.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        RetroLabel("正在读取游戏列表…", color = Retro.Cream, size = 18)
        Spacer(Modifier.height(10.dp))
        RetroLabel("列表来自 GitHub，本机不存 ROM", color = Retro.Ash, size = 13)
    }
}

@Composable
private fun CatalogError(state: AppState) {
    Column(
        Modifier.fillMaxSize().padding(20.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        RetroLabel(state.catalogError ?: "", color = Retro.Yellow, size = 16, maxLines = 3, align = TextAlign.Center)
        Spacer(Modifier.height(16.dp))
        Box(
            Modifier
                .focusable()
                .onKeyEvent { event ->
                    if (event.type == KeyEventType.KeyDown &&
                        (event.key == Key.Enter || event.key == Key.DirectionCenter)
                    ) {
                        state.readCatalog()
                        true
                    } else {
                        false
                    }
                }
                .background(Retro.Gold, RoundedCornerShape(2.dp))
                .padding(horizontal = 18.dp, vertical = 8.dp),
        ) {
            RetroLabel("重新读取", color = Retro.Ink, size = 15, bold = true)
        }
    }
}

/** A direction on the wall. */
private enum class Move { Left, Right, Up, Down }

/**
 * Where a direction lands inside the current page.
 *
 * Movement stays inside the page and the page turns instead: a grid that scrolls
 * under a sticky cursor hides most of the list, and on a TV the list is what you
 * are looking at.
 */
private fun move(index: Int, direction: Move): Int? {
    val row = index / COLUMNS
    val column = index % COLUMNS
    return when (direction) {
        Move.Left -> if (column == 0) null else index - 1
        Move.Right -> if (column == COLUMNS - 1) null else index + 1
        Move.Up -> if (row == 0) null else index - COLUMNS
        Move.Down -> if (row == ROWS - 1) null else index + COLUMNS
    }
}

/** How often a held direction has to repeat before it turns the page. */
private object FastPage {
    const val REPEATS = 4
}

/** The last index of a possibly-empty page. */
private fun lastIndex(size: Int): Int = (size - 1).coerceAtLeast(0)
