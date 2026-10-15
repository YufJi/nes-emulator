package com.example.nesapp

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.io.File
import kotlin.concurrent.thread
import uniffi.nes_uniffi.NesException

/** Which screen the app is showing. */
sealed interface Screen {
    /** The switch-on animation, before the catalogue has been read. */
    data object Boot : Screen

    /** The cartridge wall. */
    data object Home : Screen

    /** A game is being fetched; the numbers are what the progress bar shows. */
    data class Loading(val game: GameEntry, val received: Long, val total: Long) : Screen

    /** A game is running, with or without a panel over it. */
    data class Playing(val game: GameEntry, val overlay: Overlay) : Screen

    data object Settings : Screen

    data object About : Screen
}

/** The panel drawn over a running game. */
sealed interface Overlay {
    /** No panel: every key belongs to the console. */
    data object None : Overlay

    /** The in-game menu. */
    data object Menu : Overlay

    /** The four save-state slots; `saving` picks which way they are used. */
    data class Slots(val saving: Boolean) : Overlay
}

/**
 * Everything the screens read, and the few things they can do.
 *
 * One object owns the flow — which screen is up, what is downloading, what the
 * status bar says — so that the screens themselves stay functions of state. The
 * console lives in [NesSession]; this class is the part that knows what to do
 * when a game is picked, when a save is written, and when the session gives up.
 */
class AppState(private val context: Context) {

    val settings = Settings(context)
    val saves = SaveStore(context)
    val recents = Recents(context)

    var screen: Screen by mutableStateOf(Screen.Boot)
        private set

    /** The catalogue, or null while it is being read (or after a failure). */
    var games: List<GameEntry>? by mutableStateOf(null)
        private set

    /** Why the catalogue is missing, when it is. */
    var catalogError: String? by mutableStateOf(null)
        private set

    /** A one-line message for the footer: a failure, a hint, a confirmation. */
    var notice: String? by mutableStateOf(null)
        private set

    var framesPerSecond: Double by mutableStateOf(0.0)
        private set

    /** Whether the core reports a halted CPU, which is worth saying out loud. */
    var cpuHalted: Boolean by mutableStateOf(false)
        private set

    /** Whether the running game is paused. */
    var paused: Boolean by mutableStateOf(false)
        private set

    private val mainHandler = Handler(Looper.getMainLooper())

    /** The cartridge currently downloaded; deleted when its session ends. */
    private val romFile = File(context.cacheDir, ROM_FILE_NAME)

    private var downloader: RomDownloader? = null
    private var downloadCancelled = false
    private var bootDelayElapsed = false

    val session = NesSession(
        context = context,
        settings = settings,
        onStatus = { fps, halted ->
            onMain {
                framesPerSecond = fps
                cpuHalted = halted
            }
        },
        onFatalError = { message ->
            onMain {
                notice = message
                // The machine is mid-instruction and only another cartridge can
                // put it right; there is no state worth writing back.
                leaveGame(writeBattery = false)
            }
        },
    )

    // MARK: - Starting up

    /**
     * Play the switch-on click, read the catalogue and leave the boot screen.
     *
     * The animation is given a fixed moment of its own rather than waiting on
     * the catalogue: the file is an asset, but a first run on a slow box should
     * not turn a title screen into a spinner.
     */
    fun start() {
        // A cartridge left in the cache by a process that was killed is not
        // anyone's session any more.
        romFile.delete()
        session.playBootChime()
        readCatalog()
        mainHandler.postDelayed({
            bootDelayElapsed = true
            leaveBoot()
        }, BOOT_MILLIS)
    }

    /** Read the bundled catalogue again, e.g. after a failure. */
    fun readCatalog() {
        catalogError = null
        thread(name = "nes-catalog") {
            val result = runCatching { GameCatalog.load(context) }
            onMain {
                result
                    .onSuccess { games = it }
                    .onFailure {
                        catalogError = "游戏列表读不出来：${it.message ?: it::class.java.simpleName}"
                    }
                leaveBoot()
            }
        }
    }

    /** The boot screen gives way once both the animation and the catalogue are done. */
    private fun leaveBoot() {
        if (bootDelayElapsed && (games != null || catalogError != null) && screen is Screen.Boot) {
            screen = Screen.Home
        }
    }

    // MARK: - Games

    /**
     * Fetch and start a cartridge.
     *
     * The ROM goes to one file in the cache and is deleted when the session
     * ends: the catalogue is a list of links, not a library, and the app is not
     * a place to accumulate cartridges.
     */
    fun openGame(game: GameEntry) {
        if (downloader != null) return
        downloadCancelled = false
        screen = Screen.Loading(game, received = 0, total = -1)

        val fetch = RomDownloader(romFile)
        downloader = fetch
        thread(name = "nes-download") {
            try {
                val file = fetch.download(game.url) { received, total ->
                    onMain {
                        val loading = screen as? Screen.Loading
                        if (loading?.game == game) {
                            screen = Screen.Loading(game, received, total)
                        }
                    }
                }
                val rom = file.readBytes()
                onMain { start(game, rom) }
            } catch (cause: Exception) {
                // Logged as well as shown: the notice bar is the viewer's copy,
                // and this one is for whoever is looking at `adb logcat` when a
                // link goes bad.
                Log.w(TAG, "download failed for ${game.name}", cause)
                onMain {
                    downloader = null
                    if (!downloadCancelled) {
                        notice = "「${game.name}」下载失败：${cause.message ?: cause::class.java.simpleName}"
                    }
                    screen = Screen.Home
                }
            }
        }
    }

    /** Abandon a download and go back to the list. */
    fun cancelDownload() {
        downloadCancelled = true
        downloader?.cancel()
        downloader = null
        screen = Screen.Home
    }

    /** Load the cartridge into the console and put the picture up. */
    private fun start(game: GameEntry, rom: ByteArray) {
        downloader = null
        // The battery save is written back before the first frame, and the ROM
        // is deleted at the end of the session, so the save is the only thing
        // this game leaves behind.
        val battery = saves.readBattery(game.name)
        try {
            session.load(rom, battery)
        } catch (cause: NesException) {
            notice = "「${game.name}」打不开：${cause.message ?: cause::class.java.simpleName}"
            screen = Screen.Home
            return
        }
        recents.remember(game.name)
        paused = false
        cpuHalted = false
        screen = Screen.Playing(game, Overlay.None)
        session.resume()
    }

    /**
     * End the session and return to the list.
     *
     * The battery save is written first — it is the only thing that outlives the
     * cartridge file, which is deleted here.
     */
    fun leaveGame(writeBattery: Boolean = true) {
        val playing = screen as? Screen.Playing ?: return
        if (writeBattery) {
            session.writeBattery(saves.batteryFile(playing.game.name))
        }
        session.pause()
        session.surface.clearPicture()
        romFile.delete()
        paused = false
        cpuHalted = false
        framesPerSecond = 0.0
        screen = Screen.Home
    }

    /**
     * Hold the running game.
     *
     * Two functions rather than one taking a flag: `setPaused` would collide with
     * the setter Kotlin generates for [paused], and a call site that reads
     * `pauseGame()` says what it does without a comment.
     */
    fun pauseGame() {
        if (screen !is Screen.Playing || paused) return
        paused = true
        session.pause()
    }

    /** Let the running game run again. */
    fun resumeGame() {
        if (screen !is Screen.Playing || !paused) return
        paused = false
        session.resume()
    }

    /** Close the menu and let the game run again, resuming it if it was held. */
    fun continueGame() {
        resumeGame()
        closeOverlay()
    }

    /**
     * Put a panel over the game, and hold the game while it is there.
     *
     * A menu that leaves the game running behind it is a menu you cannot read:
     * the picture keeps moving, the sound keeps playing, and a press meant for
     * the panel can end up in the game.
     */
    fun openOverlay(overlay: Overlay) {
        val playing = screen as? Screen.Playing ?: return
        if (overlay !is Overlay.None) pauseGame()
        screen = playing.copy(overlay = overlay)
    }

    /** Take the panel away and let the game run again. */
    fun closeOverlay() {
        val playing = screen as? Screen.Playing ?: return
        resumeGame()
        screen = playing.copy(overlay = Overlay.None)
    }

    /** Whether [slot] holds a state for the running game. */
    fun slotTime(game: GameEntry, slot: Int): Long? = saves.stateTime(game.name, slot)

    /** Write the running console into a slot. */
    fun saveTo(slot: Int) {
        val playing = screen as? Screen.Playing ?: return
        val state = session.saveState()
        if (state == null) {
            notice = "这一局还没法保存"
            return
        }
        saves.writeState(playing.game.name, slot, state)
        notice = "已存到第 ${slot + 1} 档"
        closeOverlay()
    }

    /** Restore a slot into the running console. */
    fun loadFrom(slot: Int) {
        val playing = screen as? Screen.Playing ?: return
        val state = saves.readState(playing.game.name, slot)
        if (state == null) {
            notice = "第 ${slot + 1} 档还是空的"
            return
        }
        session.loadState(state)
        notice = "已读取第 ${slot + 1} 档"
        paused = false
        closeOverlay()
    }

    /** Press the console's reset button. */
    fun resetGame() {
        session.reset()
        notice = "已重置卡带"
        closeOverlay()
    }

    // MARK: - Screens

    fun openSettings() {
        screen = Screen.Settings
    }

    fun openAbout() {
        screen = Screen.About
    }

    fun goHome() {
        screen = Screen.Home
    }

    /** Drop every save the app holds: battery, states and the play history. */
    fun clearAllSaves() {
        saves.clearBatterySaves()
        saves.clearStates()
        recents.clear()
        notice = "已清空全部存档"
    }

    fun dismissNotice() {
        notice = null
    }

    /** Run [block] on the main thread, wherever this is called from. */
    private fun onMain(block: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) block() else mainHandler.post(block)
    }

    private companion object {
        const val TAG = "AppState"

        /** Where the one downloaded cartridge lives until its session ends. */
        const val ROM_FILE_NAME = "cartridge.nes"

        /** How long the switch-on animation runs before the list appears. */
        const val BOOT_MILLIS = 1200L
    }
}
