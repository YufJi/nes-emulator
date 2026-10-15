package com.example.nesapp

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import android.content.Context
import android.content.SharedPreferences
import kotlin.properties.ReadWriteProperty
import kotlin.reflect.KProperty

/**
 * The games played most recently, newest first.
 *
 * Kept as a list of catalogue names in `SharedPreferences` rather than derived
 * from the save files: a game can be played without ever writing a save, and
 * the home screen should still be able to offer it back.
 */
class Recents(context: Context) {

    private val prefs: SharedPreferences =
        context.getSharedPreferences("nes-tv-recents", Context.MODE_PRIVATE)

    /** The names on the home screen's "最近玩过" row, newest first. */
    var names: List<String> by NamesPreference(prefs)

    /** Record a play, moving an already-known game back to the front. */
    fun remember(name: String) {
        names = (listOf(name) + names.filterNot { it == name }).take(LIMIT)
    }

    /** Forget every game, e.g. when all of the app's saves are cleared. */
    fun clear() {
        names = emptyList()
    }

    companion object {
        private const val KEY = "names"
        private const val SEPARATOR = "\n"

        /** How many games the home screen's row shows: a row, not a list. */
        const val LIMIT = 6
    }

    /** The list as Compose state, stored one name per line. */
    private class NamesPreference(private val prefs: SharedPreferences) :
        ReadWriteProperty<Any?, List<String>> {

        private var value by mutableStateOf(read())

        override fun getValue(thisRef: Any?, property: KProperty<*>): List<String> = value

        override fun setValue(thisRef: Any?, property: KProperty<*>, value: List<String>) {
            this.value = value
            prefs.edit().putString(KEY, value.joinToString(SEPARATOR)).apply()
        }

        private fun read(): List<String> =
            prefs.getString(KEY, null)?.split(SEPARATOR)?.filter { it.isNotEmpty() } ?: emptyList()
    }
}
