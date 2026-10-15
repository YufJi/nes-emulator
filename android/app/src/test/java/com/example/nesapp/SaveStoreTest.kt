package com.example.nesapp

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The file names a save is stored under.
 *
 * A save is keyed by the catalogue name, and those names contain spaces, `#!`,
 * parentheses and Chinese; a name that cannot be written to disk would lose the
 * only thing that outlives a downloaded cartridge.
 */
class SaveStoreTest {

    @Test
    fun testNamesBecomeFilesystemSafeWithoutLosingTheirIdentity() {
        // A slash would turn one file into a directory path, and a colon or a
        // quote is not allowed on the filesystem at all.
        val stem = SaveStore.fileStem("a/b\\c:d\"e")
        assertEquals("a_b_c_d_e", stem)
    }

    @Test
    fun testAnOrdinaryNameIsLeftAlone() {
        // Spaces, `#!`, parentheses and Chinese are all legal here, and keeping
        // them keeps a save file recognisable next to its game.
        assertEquals("A B #!(J).nes", SaveStore.fileStem("A B #!(J).nes"))
    }

    @Test
    fun testALongChineseNameStaysInsideTheFilesystemsByteLimit() {
        val name = "超级".repeat(80)
        val stem = SaveStore.fileStem(name)

        // 255 bytes is the limit on Android's filesystems, and a Chinese
        // character is three of them in UTF-8.
        assertTrue(
            "a name like this would fail to write",
            stem.toByteArray(Charsets.UTF_8).size < 255,
        )
    }

    @Test
    fun testDifferentGamesGetDifferentFiles() {
        assertNotEquals(
            SaveStore.fileStem("90坦克"),
            SaveStore.fileStem("1943"),
        )
    }

    @Test
    fun testAnEmptyNameStillProducesAFile() {
        assertEquals("game", SaveStore.fileStem(""))
    }
}
