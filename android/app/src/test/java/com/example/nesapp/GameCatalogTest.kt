package com.example.nesapp

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The catalogue parse.
 *
 * The file is the web front end's own `games.json`, so the only things this can
 * get wrong are the things that would break a game: a name that is not the one
 * the list sorts by, or a link that has been decoded on the way in.
 */
class GameCatalogTest {

    @Test
    fun testNamesAndLinksSurviveTheParseUnchanged() {
        // The real file contains Chinese names, spaces and `#!`, and links whose
        // path segments are percent-encoded; decoding either would break the
        // download.
        val json = """
            [
              {"url":"https://raw.githubusercontent.com/x/y/refs/heads/master/100%E5%90%881.NES","name":"100合1"},
              {"url":"https://raw.githubusercontent.com/x/y/refs/heads/master/A%20B%20%23%21.nes","name":"A B #!"}
            ]
        """.trimIndent()

        val games = GameCatalog.parse(json)

        assertEquals(2L, games.size.toLong())
        assertEquals("100合1", games[0].name)
        assertTrue(games[0].url.endsWith("/100%E5%90%881.NES"))
        assertEquals("A B #!", games[1].name)
        assertTrue(games[1].url.endsWith("/A%20B%20%23%21.nes"))
    }

    @Test
    fun testTheRealCatalogueParsesIntoUniqueNames() {
        // The name doubles as the key for saves, so a duplicate would mean two
        // games sharing one save file.
        val games = GameCatalog.parse(CATALOGUE_SAMPLE)
        assertEquals(games.size, games.map { it.name }.toSet().size)
        assertTrue("names must not be blank", games.all { it.name.isNotBlank() })
        assertTrue("links must be absolute", games.all { it.url.startsWith("https://") })
    }

    @Test
    fun testAnEmptyListIsNotAnError() {
        assertEquals(emptyList<GameEntry>(), GameCatalog.parse("[]"))
    }

    private companion object {
        val CATALOGUE_SAMPLE = """
            [
              {"url":"https://example.invalid/a.nes","name":"阿拉丁"},
              {"url":"https://example.invalid/b.nes","name":"90坦克"},
              {"url":"https://example.invalid/c.nes","name":"1943"}
            ]
        """.trimIndent()
    }
}
