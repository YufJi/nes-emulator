package com.example.nesapp

import android.content.Context
import android.content.SharedPreferences
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlin.properties.ReadWriteProperty
import kotlin.reflect.KProperty

/** Which TV system a cartridge is run with. */
enum class RegionChoice(val stored: String) {
    /** Trust the cartridge header, which is what the core does by default. */
    Auto("auto"),
    /** Force NTSC (60.0985 Hz, 262 scanlines). */
    Ntsc("ntsc"),
    /** Force PAL (50.0067 Hz, 312 scanlines). */
    Pal("pal"),
}

/**
 * Host-side preferences.
 *
 * Each one is Compose state *and* a stored value, because three readers have to
 * agree at the same moment: the settings screen that changes it, the renderer on
 * the GL thread and the emulation thread that reads it when a cartridge is
 * loaded. A plain `SharedPreferences` getter would only be noticed on the next
 * read, and a plain Compose value would be forgotten on exit.
 */
class Settings(context: Context) {
    private val prefs: SharedPreferences =
        context.getSharedPreferences("nes-tv-settings", Context.MODE_PRIVATE)

    /** Whole-pixel scaling (the default) rather than stretching to fill the screen. */
    var integerScale: Boolean by BooleanPreference(prefs, "integer-scale", true)

    /** Scanlines and a soft corner falloff over the picture. */
    var crtFilter: Boolean by BooleanPreference(prefs, "crt-filter", false)

    /** Silence the console without disturbing its state. */
    var muted: Boolean by BooleanPreference(prefs, "muted", false)

    /** Cartridge TV system; the header decides while this is `Auto`. */
    var region: RegionChoice by EnumPreference(prefs, "region", RegionChoice.Auto, RegionChoice.entries)
}

/** A stored boolean that is also Compose state. */
private class BooleanPreference(
    private val prefs: SharedPreferences,
    private val key: String,
    default: Boolean,
) : ReadWriteProperty<Any?, Boolean> {
    private var value by mutableStateOf(prefs.getBoolean(key, default))

    override fun getValue(thisRef: Any?, property: KProperty<*>): Boolean = value

    override fun setValue(thisRef: Any?, property: KProperty<*>, value: Boolean) {
        this.value = value
        prefs.edit().putBoolean(key, value).apply()
    }
}

/** A stored enum that is also Compose state; stored by name, so reordering is safe. */
private class EnumPreference<T : Enum<T>>(
    private val prefs: SharedPreferences,
    private val key: String,
    private val default: T,
    private val values: List<T>,
) : ReadWriteProperty<Any?, T> {
    private var value by mutableStateOf(read())

    override fun getValue(thisRef: Any?, property: KProperty<*>): T = value

    override fun setValue(thisRef: Any?, property: KProperty<*>, value: T) {
        this.value = value
        prefs.edit().putString(key, value.name).apply()
    }

    /** An unknown or missing name falls back to the default instead of throwing. */
    private fun read(): T {
        val stored = prefs.getString(key, null) ?: return default
        return values.firstOrNull { it.name == stored } ?: default
    }
}
