package com.example.nesapp

import android.content.Context
import com.google.gson.Gson
import com.google.gson.reflect.TypeToken

/**
 * One entry of the catalogue: a name and the direct link the ROM is fetched
 * from.
 *
 * The names in the list are unique — they come from the file names of a single
 * flat directory on GitHub — so the name doubles as the identity of a game, and
 * is what battery saves and save states are keyed by.
 */
data class GameEntry(val name: String, val url: String)

/**
 * The bundled game catalogue, `assets/games.json`.
 *
 * The file is the same one the web front end ships (Gradle's `copyGameList`
 * copies `web/public/games.json` into the assets), so the TV app and the web
 * page offer the same list and neither can drift from the other. It carries
 * names and links only: no ROM data is committed here, and the app downloads a
 * cartridge only after the viewer picks it.
 */
object GameCatalog {

    /** Parses the catalogue. Exposed separately so a JVM test can feed it a file. */
    fun parse(json: String): List<GameEntry> {
        val type = object : TypeToken<List<GameEntry>>() {}.type
        return Gson().fromJson<List<GameEntry>>(json, type) ?: emptyList()
    }

    /** Reads and parses the bundled catalogue. */
    fun load(context: Context): List<GameEntry> =
        context.assets.open("games.json").use { stream ->
            parse(stream.readBytes().decodeToString())
        }
}
