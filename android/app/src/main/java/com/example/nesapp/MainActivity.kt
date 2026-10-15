package com.example.nesapp

import android.os.Bundle
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import com.example.nesapp.ui.NesApp

/**
 * The one activity: the interface, and the two key streams that feed it.
 *
 * While a game is running the console gets the key events first, which is the
 * whole reason this class exists rather than leaving it to the composables —
 * Compose's focus system must not see a D-pad press that belongs to Mario.
 */
class MainActivity : ComponentActivity() {

    private lateinit var state: AppState

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // A console that dims after a minute is a console nobody finishes a
        // level on. TV boxes count a game as idle where they would count video
        // as playing.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        state = AppState(this)
        setContent { NesApp(state) }

        state.start()
    }

    override fun onResume() {
        super.onResume()
        // Coming back from the launcher is like coming back from a menu: the
        // clock starts again from now rather than from where it stopped.
        if (state.screen is Screen.Playing) {
            state.session.resume()
            state.resumeGame()
        }
    }

    override fun onPause() {
        super.onPause()
        val playing = state.screen as? Screen.Playing ?: return
        // A TV box can take the app away without another callback, so the
        // battery save is written here rather than only on the way out.
        state.session.writeBattery(state.saves.batteryFile(playing.game.name))
        state.pauseGame()
    }

    override fun onDestroy() {
        state.session.close()
        super.onDestroy()
    }

    /**
     * Give the console every key that is not the interface's, and act on the one
     * key that is.
     *
     * MENU is handled here rather than in Compose because on the playing screen
     * nothing holds focus: the picture is a surface, not a widget, so a key event
     * has no path to travel. It is also the one key a remote is guaranteed to
     * have, which makes it the way out of a game and the way into the settings.
     */
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (event.keyCode == KeyEvent.KEYCODE_MENU) {
            if (event.action == KeyEvent.ACTION_UP) return true
            when (val screen = state.screen) {
                is Screen.Playing ->
                    if (screen.overlay is Overlay.None) state.openOverlay(Overlay.Menu) else state.closeOverlay()

                Screen.Home -> state.openSettings()
                Screen.Settings, Screen.About -> state.goHome()
                else -> Unit
            }
            return true
        }

        val playing = state.screen as? Screen.Playing
        if (playing != null &&
            playing.overlay is Overlay.None &&
            state.session.onKeyEvent(event.deviceId, event.isFromGamepad(), event)
        ) {
            return true
        }
        return super.dispatchKeyEvent(event)
    }

    /** The same rule for the analog sticks. */
    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        val playing = state.screen as? Screen.Playing
        if (playing != null && playing.overlay is Overlay.None) {
            if (state.session.onMotionEvent(event.deviceId, event)) return true
        }
        return super.onGenericMotionEvent(event)
    }
}

/**
 * Whether a key came from a gamepad rather than a remote or a keyboard.
 *
 * The distinction decides the player: a remote is always 1P, gamepads take 1P
 * and 2P in the order they are seen.
 */
private fun KeyEvent.isFromGamepad(): Boolean =
    device?.supportsSource(InputDevice.SOURCE_GAMEPAD) == true ||
        source and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
