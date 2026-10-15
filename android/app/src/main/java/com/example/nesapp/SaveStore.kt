package com.example.nesapp

import android.content.Context
import java.io.File

/**
 * What the app keeps on disk for a cartridge: its battery save and four
 * save-state slots.
 *
 * Files are keyed by the catalogue's game name, which is what lets a save
 * outlive the ROM. The cartridge itself is deleted the moment a session ends,
 * but the save stays behind, waiting for the next download of the same game.
 */
class SaveStore(context: Context) {

    /** Cartridge save RAM, one file per game. */
    private val battery = File(context.filesDir, "saves")

    /** Save states, one file per game and slot. */
    private val states = File(context.filesDir, "states")

    /** The one file a game's battery save lives in. */
    fun batteryFile(name: String): File = File(battery, "${fileStem(name)}.sav")

    /** The file a game's save-state slot lives in. */
    fun stateFile(name: String, slot: Int): File = File(states, "${fileStem(name)}.$slot.ss")

    fun readBattery(name: String): ByteArray? = batteryFile(name).takeIf { it.isFile }?.readBytes()

    fun writeBattery(name: String, data: ByteArray) {
        battery.mkdirs()
        batteryFile(name).writeBytes(data)
    }

    fun readState(name: String, slot: Int): ByteArray? =
        stateFile(name, slot).takeIf { it.isFile }?.readBytes()

    fun writeState(name: String, slot: Int, data: ByteArray) {
        states.mkdirs()
        stateFile(name, slot).writeBytes(data)
    }

    /** When a slot was written, or null when it is empty; the slot list shows it. */
    fun stateTime(name: String, slot: Int): Long? =
        stateFile(name, slot).takeIf { it.isFile }?.lastModified()

    /** Delete all battery saves in the store. */
    fun clearBatterySaves() {
        battery.listFiles()?.forEach { it.delete() }
    }

    /** Delete all save states in the store. */
    fun clearStates() {
        states.listFiles()?.forEach { it.delete() }
    }

    companion object {
        /** How many save-state slots there are; the same count the web front end offers. */
        const val SLOTS = 4

        /**
         * A file name for a catalogue name.
         *
         * The names contain spaces, `#!`, parentheses and Chinese; the length
         * cap keeps the UTF-8 encoding of a long Chinese name inside the
         * filesystem's 255-byte limit. Characters Android forbids are replaced
         * rather than rejected — a save must never fail because of a title.
         *
         * Internal rather than private so the unit tests can hold it to those
         * two promises.
         */
        internal fun fileStem(name: String): String =
            name.replace(Regex("[\\\\/:*?\"<>|\\x00-\\x1f]"), "_").take(60).ifEmpty { "game" }
    }
}
