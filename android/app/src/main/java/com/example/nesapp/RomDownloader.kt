package com.example.nesapp

import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/**
 * Fetches one cartridge from the catalogue.
 *
 * The result goes to a single file in the cache directory that the session
 * deletes when it ends: downloaded games are played and dropped, never curated,
 * so the app's storage does not grow with the size of the list. The links point
 * at `raw.githubusercontent.com`, which serves the bytes directly and needs no
 * API token.
 */
class RomDownloader(private val target: File) {

    @Volatile
    private var cancelled = false

    /** Stop an in-flight download; the next read of the stream aborts it. */
    fun cancel() {
        cancelled = true
    }

    /**
     * Download [url] into [target].
     *
     * [onProgress] is called from the downloading thread with the bytes received
     * so far and the total when the server announced one (`-1` when it did not —
     * GitHub answers with chunked content, so that is the normal case for a
     * ROM link).
     */
    fun download(url: String, onProgress: (received: Long, total: Long) -> Unit): File {
        val connection = (URL(url).openConnection() as HttpURLConnection).apply {
            connectTimeout = 15_000
            readTimeout = 20_000
            // GitHub rejects requests without a user agent.
            setRequestProperty("User-Agent", "nes-emulator-android")
            instanceFollowRedirects = true
        }

        try {
            val code = connection.responseCode
            if (code != HttpURLConnection.HTTP_OK) {
                throw IOException("HTTP $code")
            }
            // Read the header rather than `contentLengthLong`: the latter is
            // API 24 and this app runs on 21.
            val total = connection.getHeaderField("Content-Length")?.toLongOrNull() ?: -1L
            target.parentFile?.mkdirs()

            connection.inputStream.use { input ->
                target.outputStream().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    var received = 0L
                    // Report on a time budget rather than per chunk: a fast link
                    // would otherwise post hundreds of updates a second, and the
                    // number on screen is unreadable anyway.
                    var reportedAt = 0L
                    while (true) {
                        if (cancelled) throw IOException("已取消")
                        val read = input.read(buffer)
                        if (read < 0) break
                        output.write(buffer, 0, read)
                        received += read
                        val now = System.currentTimeMillis()
                        if (now - reportedAt >= 100) {
                            reportedAt = now
                            onProgress(received, total)
                        }
                    }
                    onProgress(received, total)
                }
            }
            return target
        } finally {
            connection.disconnect()
        }
    }
}
