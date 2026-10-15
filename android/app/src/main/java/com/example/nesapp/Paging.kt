package com.example.nesapp

/**
 * The arithmetic behind paging through the catalogue.
 *
 * Kept out of the screen because a screen cannot be tested off a device, and
 * because the ends of the list are where a pager usually goes wrong: the first
 * page has nothing before it, the last page has nothing after it, and a button
 * that does nothing is worse than one that is visibly unavailable.
 */

/** How many pages [count] entries take, [perPage] to a page. */
fun pageCountOf(count: Int, perPage: Int): Int =
    if (perPage <= 0 || count <= 0) 0 else (count + perPage - 1) / perPage

/** Where [step] pages from [page] lands, without leaving the list. */
fun pageAfter(page: Int, step: Int, pageCount: Int): Int =
    (page + step).coerceIn(0, (pageCount - 1).coerceAtLeast(0))

/**
 * Whether the pager button [index] means anything on [page].
 *
 * `0` is the one that goes back and `1` the one that goes forward, in the order
 * they are drawn.
 */
fun pagerAvailable(index: Int, page: Int, pageCount: Int): Boolean =
    if (index == 0) page > 0 else page < pageCount - 1

/**
 * Which button to focus when the pager is first reached.
 *
 * Forward, because that is what a pager is for — unless the list has run out, in
 * which case the only button left to press is the one that goes back.
 */
fun preferredPagerIndex(page: Int, pageCount: Int): Int =
    if (pagerAvailable(1, page, pageCount)) 1 else 0
