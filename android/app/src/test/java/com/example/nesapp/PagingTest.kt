package com.example.nesapp

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The page buttons: how many pages there are, and what each one does at the two
 * ends of the list.
 */
class PagingTest {

    @Test
    fun testTheCatalogueSplitsIntoPagesWithARemainder() {
        assertEquals(78, pageCountOf(932, 12))
        assertEquals(1, pageCountOf(12, 12))
        assertEquals(2, pageCountOf(13, 12))
        assertEquals(0, pageCountOf(0, 12))
    }

    @Test
    fun testThePreviousButtonIsDeadOnTheFirstPageAndTheNextOnTheLast() {
        val pages = 78
        assertFalse("nothing before the first page", pagerAvailable(0, page = 0, pageCount = pages))
        assertTrue(pagerAvailable(1, page = 0, pageCount = pages))
        assertTrue(pagerAvailable(0, page = 77, pageCount = pages))
        assertFalse("nothing after the last page", pagerAvailable(1, page = 77, pageCount = pages))
        assertTrue("and both work in between", pagerAvailable(0, page = 5, pageCount = pages))
    }

    @Test
    fun testASinglePageHasNoUsableButtons() {
        assertFalse(pagerAvailable(0, page = 0, pageCount = 1))
        assertFalse(pagerAvailable(1, page = 0, pageCount = 1))
    }

    @Test
    fun testSteppingStopsAtTheEndsInsteadOfWrappingAround() {
        // Wrapping is the wrong answer here: 932 cartridges sorted by name mean a
        // viewer paging past the end wants the end, not the beginning.
        assertEquals(0, pageAfter(0, -1, 78))
        assertEquals(77, pageAfter(77, 1, 78))
        assertEquals(2, pageAfter(1, 1, 78))
        assertEquals(0, pageAfter(0, 1, 1))
    }

    @Test
    fun testFocusStartsOnTheForwardButtonExceptOnTheLastPage() {
        assertEquals(1, preferredPagerIndex(page = 0, pageCount = 78))
        assertEquals(1, preferredPagerIndex(page = 76, pageCount = 78))
        assertEquals(0, preferredPagerIndex(page = 77, pageCount = 78))
        assertEquals(0, preferredPagerIndex(page = 0, pageCount = 1))
    }
}
