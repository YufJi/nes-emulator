package com.example.nesapp

import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import kotlin.math.abs
import uniffi.nes_uniffi.Nes

/**
 * The pads, in the console's own button order.
 *
 * Values match the core's `BUTTON_*` constants (`buttonA()` … `buttonRight()`
 * on the binding), which is the one place the two sides could drift apart.
 */
enum class PadButton(val index: Int) {
    A(0),
    B(1),
    SELECT(2),
    START(3),
    UP(4),
    DOWN(5),
    LEFT(6),
    RIGHT(7),
}

/** Which console each connected pad drives. */
private const val PLAYER_1 = 0
private const val PLAYER_2 = 1

/**
 * Turns key and joystick events into the console's buttons.
 *
 * Two kinds of device arrive here and they behave differently:
 *
 * * a **TV remote** (or a keyboard, or the emulator's host keyboard) drives
 *   player 1, because it is the only pad there is;
 * * **gamepads** take player 1 and player 2 in the order they are seen, so an
 *   ordinary setup needs no configuration to play a two-player game.
 *
 * Turbo is not a console button: it is a switch on the pad that fires its button
 * again and again while held. It is applied per emulated frame in [snapshot], so
 * an ordinary press stays an ordinary press — a game that wants A held down
 * still gets it.
 *
 * Key events arrive on the main thread and the frame path runs on the emulation
 * thread, so every entry point is synchronized: the state here is small and the
 * calls are few, and a lock is the honest way to keep two threads off it rather
 * than a queue that would put a frame of latency in front of every press.
 */
class InputMapper {

    /** What the pads are holding, per player. */
    private val held = Array(2) { BooleanArray(PadButton.entries.size) }

    /** Turbo switches, per player: index 0 fires A, index 1 fires B. */
    private val turbo = Array(2) { BooleanArray(2) }

    /** Which player each gamepad drives; they claim players in the order seen. */
    private val padPlayers = HashMap<Int, Int>()

    /** The last state handed to the console, so only changes cross the FFI boundary. */
    private val applied = Array(2) { BooleanArray(PadButton.entries.size) }

    /**
     * Apply a key event.
     *
     * `gamepad` says whether it came from a pad rather than a remote or a
     * keyboard, which is what decides the player. Returns true when the key was
     * one of the console's, so the caller knows whether it was consumed.
     */
    @Synchronized
    fun onKey(deviceId: Int, gamepad: Boolean, keyCode: Int, pressed: Boolean): Boolean {
        val player = playerFor(deviceId, gamepad)
        when (keyCode) {
            KeyEvent.KEYCODE_DPAD_UP -> set(player, PadButton.UP, pressed)
            KeyEvent.KEYCODE_DPAD_DOWN -> set(player, PadButton.DOWN, pressed)
            KeyEvent.KEYCODE_DPAD_LEFT -> set(player, PadButton.LEFT, pressed)

            KeyEvent.KEYCODE_DPAD_RIGHT -> set(player, PadButton.RIGHT, pressed)

            // The remote's centre key is A: it is the only button a bare
            // remote has that a game would expect to be the action button.
            KeyEvent.KEYCODE_DPAD_CENTER, KeyEvent.KEYCODE_ENTER,
            KeyEvent.KEYCODE_NUMPAD_ENTER,
            -> set(player, PadButton.A, pressed)

            KeyEvent.KEYCODE_BUTTON_A -> set(player, PadButton.A, pressed)
            KeyEvent.KEYCODE_BUTTON_B -> set(player, PadButton.B, pressed)
            // X is the face button beside B on most pads, so it is B as well —
            // the same choice the web front end makes.
            KeyEvent.KEYCODE_BUTTON_X -> set(player, PadButton.B, pressed)
            KeyEvent.KEYCODE_BUTTON_Y -> set(player, PadButton.A, pressed)

            KeyEvent.KEYCODE_BUTTON_START, KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE -> set(player, PadButton.START, pressed)

            // Not the remote's MENU: while a game is running that key belongs
            // to the interface (it opens the in-game menu), so a bare remote can
            // always get back out of a game.
            KeyEvent.KEYCODE_BUTTON_SELECT -> set(player, PadButton.SELECT, pressed)

            // Shoulders are not console buttons: they are the turbo switches, the
            // right one firing A and the left one B.
            KeyEvent.KEYCODE_BUTTON_R1, KeyEvent.KEYCODE_BUTTON_R2 -> turbo(player, a = true, pressed)
            KeyEvent.KEYCODE_BUTTON_L1, KeyEvent.KEYCODE_BUTTON_L2 -> turbo(player, a = false, pressed)

            else -> return false
        }
        return true
    }

    /**
     * Apply a joystick event: the left stick and the hat both move the D-pad.
     *
     * Returns true when the event moved something.
     */
    @Synchronized
    fun onMotion(deviceId: Int, event: MotionEvent): Boolean =
        onAxes(
            deviceId = deviceId,
            // A remote with a touchpad sends motion events too, and those must
            // not steer the D-pad; only a pad's own axes do.
            joystick = event.source and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK,
            hatX = event.getAxisValue(MotionEvent.AXIS_HAT_X),
            hatY = event.getAxisValue(MotionEvent.AXIS_HAT_Y),
            stickX = event.getAxisValue(MotionEvent.AXIS_X),
            stickY = event.getAxisValue(MotionEvent.AXIS_Y),
        )

    /**
     * The axis half of [onMotion], taking the values rather than the event.
     *
     * Split out because a `MotionEvent` cannot be built off a device, and this is
     * the part with the dead zone in it: the part worth testing.
     */
    @Synchronized
    fun onAxes(
        deviceId: Int,
        joystick: Boolean,
        hatX: Float,
        hatY: Float,
        stickX: Float,
        stickY: Float,
    ): Boolean {
        if (!joystick) return false
        val player = playerFor(deviceId, gamepad = true)
        // The hat and the stick drive the same four directions, so each axis
        // takes whichever of the two readings is further from centre. Applying
        // one and then the other would let a resting stick release whatever the
        // hat had just pressed.
        axis(player, dominant(hatX, stickX), PadButton.LEFT, PadButton.RIGHT)
        axis(player, dominant(hatY, stickY), PadButton.UP, PadButton.DOWN)
        return true
    }

    /** Whichever of a hat's reading and a stick's is further from centre. */
    private fun dominant(hat: Float, stick: Float): Float =
        if (abs(hat) >= abs(stick)) hat else stick

    /** Let go of everything, e.g. when a session ends. */
    @Synchronized
    fun releaseAll() {
        held.forEach { it.fill(false) }
        turbo.forEach { it.fill(false) }
        padPlayers.clear()
        applied.forEach { it.fill(false) }
    }

    /**
     * The buttons to hand the console for the next frame.
     *
     * Turbo alternates two frames on and two frames off — about 15 pulses a
     * second, which is fast enough that a game sampling input once a frame sees
     * separate presses and slow enough that the release is visible at all.
     */
    @Synchronized
    fun snapshot(emulatedFrame: Long): Array<BooleanArray> {
        val firing = (emulatedFrame / TURBO_HALF_PERIOD) % 2 == 0L
        return Array(2) { player ->
            BooleanArray(PadButton.entries.size) { button ->
                val plain = held[player][button]
                val pulsed = when {
                    !firing -> false
                    button == PadButton.A.index -> turbo[player][0]
                    button == PadButton.B.index -> turbo[player][1]
                    else -> false
                }
                plain || pulsed
            }
        }
    }

    /**
     * Push the current frame's buttons to the console, sending only changes.
     *
     * Called on the emulation thread, immediately before the frame is run.
     */
    @Synchronized
    fun applyTo(nes: Nes, emulatedFrame: Long) {
        val state = snapshot(emulatedFrame)
        for (player in 0 until 2) {
            for (button in PadButton.entries) {
                val pressed = state[player][button.index]
                if (pressed != applied[player][button.index]) {
                    applied[player][button.index] = pressed
                    nes.setButton((player + 1).toUByte(), button.index.toUByte(), pressed)
                }
            }
        }
    }

    /** Drop the "last applied" record, e.g. after the console is replaced. */
    @Synchronized
    fun forgetApplied() {
        applied.forEach { it.fill(false) }
    }

    private fun set(player: Int, button: PadButton, pressed: Boolean) {
        held[player][button.index] = pressed
    }

    private fun turbo(player: Int, a: Boolean, pressed: Boolean) {
        turbo[player][if (a) 0 else 1] = pressed
    }

    private fun axis(player: Int, value: Float, negative: PadButton, positive: PadButton) {
        when {
            value <= -DEAD_ZONE -> {
                set(player, negative, true)
                set(player, positive, false)
            }

            value >= DEAD_ZONE -> {
                set(player, positive, true)
                set(player, negative, false)
            }

            else -> {
                set(player, negative, false)
                set(player, positive, false)
            }
        }
    }

    /**
     * Which console a device drives.
     *
     * The remote (or a keyboard) is always player 1 — it is the only pad such a
     * setup has. Gamepads take player 1 and then player 2 in the order they are
     * first seen, so a two-player game needs nothing but a second pad.
     */
    private fun playerFor(deviceId: Int, gamepad: Boolean): Int {
        if (!gamepad) return PLAYER_1
        padPlayers[deviceId]?.let { return it }
        val player = if (PLAYER_1 in padPlayers.values) PLAYER_2 else PLAYER_1
        padPlayers[deviceId] = player
        return player
    }

    private companion object {
        /** Sticks rest near zero; half travel is well clear of the noise. */
        const val DEAD_ZONE = 0.5f

        /** Frames per turbo half-cycle; four frames is the NES Advantage's rate. */
        const val TURBO_HALF_PERIOD = 2L
    }
}
