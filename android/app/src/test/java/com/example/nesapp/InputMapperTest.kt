package com.example.nesapp

import android.view.KeyEvent
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The input path: which key is which console button, who owns it, and how turbo
 * fires.
 *
 * `MotionEvent` cannot be constructed off a device, so the stick half is tested
 * through `onAxes`, which takes the values the event would have carried.
 */
class InputMapperTest {

    private val mapper = InputMapper()

    @Test
    fun testTheDpadAndTheFaceButtonsMapToTheConsole() {
        assertTrue(mapper.onKey(REMOTE, gamepad = false, KeyEvent.KEYCODE_DPAD_UP, pressed = true))
        assertTrue(mapper.onKey(REMOTE, gamepad = false, KeyEvent.KEYCODE_BUTTON_A, pressed = true))

        val held = mapper.snapshot(emulatedFrame = 0)[0]
        assertTrue("up", held[PadButton.UP.index])
        assertTrue("A", held[PadButton.A.index])
        assertFalse("down stays up", held[PadButton.DOWN.index])
    }

    @Test
    fun testAKeyThatIsNotAConsoleButtonIsNotConsumed() {
        // Volume and the like have to keep travelling to the system.
        assertFalse(mapper.onKey(REMOTE, gamepad = false, KeyEvent.KEYCODE_VOLUME_UP, pressed = true))
    }

    @Test
    fun testTheRemoteIsAlwaysTheFirstPlayerAndPadsTakePlayersInOrder() {
        mapper.onKey(REMOTE, gamepad = false, KeyEvent.KEYCODE_DPAD_UP, pressed = true)
        mapper.onKey(PAD_ONE, gamepad = true, KeyEvent.KEYCODE_BUTTON_A, pressed = true)
        mapper.onKey(PAD_TWO, gamepad = true, KeyEvent.KEYCODE_BUTTON_A, pressed = true)

        val state = mapper.snapshot(emulatedFrame = 0)
        assertTrue("the pad and the remote both drive 1P", state[0][PadButton.UP.index])
        assertTrue("the first pad is 1P", state[0][PadButton.A.index])
        assertTrue("the second pad is 2P", state[1][PadButton.A.index])
    }

    @Test
    fun testTurboFiresEveryOtherHalfCycleAndLeavesAPlainPressAlone() {
        mapper.onKey(PAD_ONE, gamepad = true, KeyEvent.KEYCODE_BUTTON_R1, pressed = true)
        mapper.onKey(PAD_ONE, gamepad = true, KeyEvent.KEYCODE_BUTTON_B, pressed = true)

        // Two frames on, two frames off: the rate a NES Advantage fired at.
        assertTrue("turbo A fires on the first frame", mapper.snapshot(0)[0][PadButton.A.index])
        assertTrue(mapper.snapshot(1)[0][PadButton.A.index])
        assertFalse("and rests on the third", mapper.snapshot(2)[0][PadButton.A.index])
        assertFalse(mapper.snapshot(3)[0][PadButton.A.index])
        assertTrue("before firing again", mapper.snapshot(4)[0][PadButton.A.index])

        // A plain hold is a hold, not a pulse: a game that wants B down gets it.
        assertTrue(mapper.snapshot(2)[0][PadButton.B.index])
    }

    @Test
    fun testTheStickNeedsHalfTravelBeforeItSteers() {
        mapper.onAxes(PAD_ONE, joystick = true, hatX = 0f, hatY = 0f, stickX = 0.2f, stickY = 0f)
        assertFalse("noise near centre must not steer", mapper.snapshot(0)[0][PadButton.RIGHT.index])

        mapper.onAxes(PAD_ONE, joystick = true, hatX = 0f, hatY = 0f, stickX = -0.9f, stickY = 0f)
        assertTrue(mapper.snapshot(0)[0][PadButton.LEFT.index])
        assertFalse("and the opposite direction lets go", mapper.snapshot(0)[0][PadButton.RIGHT.index])
    }

    @Test
    fun testATouchpadIsNotASteeringWheel() {
        // A remote with a touchpad sends motion events as well; they are not a
        // pad's axes and must not move the D-pad.
        assertFalse(
            mapper.onAxes(PAD_ONE, joystick = false, hatX = 1f, hatY = 0f, stickX = 1f, stickY = 1f),
        )
        assertFalse(mapper.snapshot(0)[0][PadButton.RIGHT.index])
    }

    @Test
    fun testTheHatAndTheStickBothWork() {
        mapper.onAxes(PAD_ONE, joystick = true, hatX = 0f, hatY = 1f, stickX = 0f, stickY = 0f)
        assertTrue("the hat is a D-pad", mapper.snapshot(0)[0][PadButton.DOWN.index])

        mapper.onAxes(PAD_ONE, joystick = true, hatX = 0f, hatY = 0f, stickX = 0f, stickY = -0.8f)
        assertTrue(mapper.snapshot(0)[0][PadButton.UP.index])
        assertFalse("and it releases the hat's direction", mapper.snapshot(0)[0][PadButton.DOWN.index])
    }

    private companion object {
        const val REMOTE = 1
        const val PAD_ONE = 2
        const val PAD_TWO = 3
    }
}
