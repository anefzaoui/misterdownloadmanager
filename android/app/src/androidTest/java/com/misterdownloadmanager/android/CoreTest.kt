package com.misterdownloadmanager.android

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.misterdownloadmanager.android.core.Download
import com.misterdownloadmanager.android.core.Mdm
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.BeforeClass
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.net.ServerSocket
import kotlin.concurrent.thread

/**
 * The core on a real Android system: started through JNI, it downloads a file from a server in
 * this process into Download/MisterDownloadManager, and a file over HTTPS (Android's certificate
 * checks, through rustls-platform-verifier).
 */
@RunWith(AndroidJUnit4::class)
class CoreTest {
    companion object {
        private val body = ByteArray(3 shl 20) { (it * 31 + 7).toByte() }
        private lateinit var server: ServerSocket

        /** A minimal HTTP/1.1 server with ranges: enough for the engine's parts. */
        @BeforeClass
        @JvmStatic
        fun serve() {
            server = ServerSocket(0)
            thread(isDaemon = true) {
                while (true) {
                    val s = runCatching { server.accept() }.getOrNull() ?: break
                    thread(isDaemon = true) {
                        // The engine closes a connection as soon as its part is done or re-split:
                        // a write after that fails, which is the client's choice, not an error.
                        try { s.use { sock ->
                            val input = sock.getInputStream().bufferedReader(Charsets.ISO_8859_1)
                            val out = sock.getOutputStream()
                            while (true) {
                                val request = input.readLine() ?: break
                                if (request.isEmpty()) continue
                                var range: LongRange? = null
                                while (true) {
                                    val h = input.readLine() ?: break
                                    if (h.isEmpty()) break
                                    if (h.lowercase().startsWith("range: bytes=")) {
                                        val (a, b) = h.substringAfter('=').split('-')
                                        val start = a.toLong()
                                        val end = if (b.isEmpty()) body.size - 1L else minOf(b.toLong(), body.size - 1L)
                                        range = start..end
                                    }
                                }
                                val head = request.startsWith("HEAD")
                                val r = range ?: 0L..(body.size - 1L)
                                val status = if (range != null) "206 Partial Content" else "200 OK"
                                val headers = buildString {
                                    append("HTTP/1.1 $status\r\n")
                                    append("Content-Type: application/octet-stream\r\n")
                                    append("Accept-Ranges: bytes\r\n")
                                    append("Content-Length: ${r.last - r.first + 1}\r\n")
                                    if (range != null) append("Content-Range: bytes ${r.first}-${r.last}/${body.size}\r\n")
                                    append("\r\n")
                                }
                                out.write(headers.toByteArray(Charsets.ISO_8859_1))
                                if (!head) out.write(body, r.first.toInt(), (r.last - r.first + 1).toInt())
                                out.flush()
                            }
                        } } catch (_: java.io.IOException) {
                        }
                    }
                }
            }
            val context = InstrumentationRegistry.getInstrumentation().targetContext
            assertTrue("core started: ${Mdm.startError.value}", Mdm.start(context))
        }
    }

    private fun waitFor(id: Long, what: String, ok: (Download) -> Boolean): Download = runBlocking {
        withTimeout(60_000) {
            Mdm.snapshot.first { s -> s?.downloads?.firstOrNull { it.id == id }?.let(ok) == true }!!
                .downloads.first { it.id == id }
        }.also { assertNotNull(what, it) }
    }

    @Test
    fun downloadsAFileIntoDownloads() = runBlocking {
        val url = "http://127.0.0.1:${server.localPort}/test-${System.nanoTime()}.bin"
        val added = Mdm.add(listOf(url))
        assertTrue("added: $added", added.ids.isNotEmpty())
        val done = waitFor(added.ids[0], "complete") { it.complete || it.failed }
        assertEquals("status (${done.error})", "Complete", done.status)
        val file = File(done.path)
        assertTrue("under Download/: ${file.path}", file.path.startsWith(Mdm.downloadsDir().path))
        assertTrue(file.readBytes().contentEquals(body))
        Mdm.act("delete", listOf(done.id))
        Unit
    }

    @Test
    fun downloadsOverHttps() = runBlocking {
        val added = Mdm.add(listOf("https://www.google.com/robots.txt?mdm=${System.nanoTime()}"))
        assertTrue("added: $added", added.ids.isNotEmpty())
        val done = waitFor(added.ids[0], "finished") { it.complete || it.failed }
        assertEquals("status (${done.error})", "Complete", done.status)
        Mdm.act("delete", listOf(done.id))
        Unit
    }

    @Test
    fun answersTheInterface() = runBlocking {
        assertEquals("video", Mdm.classify("https://www.youtube.com/watch?v=dQw4w9WgXcQ").kind)
        assertEquals("torrent", Mdm.classify("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567").kind)
        val spotify = Mdm.classify("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC")
        assertEquals("protected", spotify.kind)
        assertTrue(spotify.message.orEmpty().contains("Spotify"))
        assertTrue(Mdm.languages().languages.size > 50)
        val h = Mdm.helpers()
        assertNotNull("ffmpeg bundled", h.ffmpeg)
    }

    /**
     * 7-Zip, which the APK carries for 7z and RAR (built with the NDK against Android's libc),
     * runs in the app's sandbox: it makes a 7z archive and extracts it again, unchanged.
     */
    @Test
    fun sevenZipRoundTrips() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val sevenZip = File(context.applicationInfo.nativeLibraryDir, "lib7zz.so").path
        val dir = File(context.cacheDir, "7z-test").apply { deleteRecursively(); mkdirs() }
        val text = "Mister Download Manager ".repeat(4000)
        File(dir, "a.txt").writeText(text)
        fun run(vararg args: String) {
            val p = ProcessBuilder(sevenZip, *args).directory(dir).redirectErrorStream(true).start()
            val out = p.inputStream.bufferedReader().readText()
            assertEquals("7zz ${args.joinToString(" ")}: $out", 0, p.waitFor())
        }
        run("a", "-bd", "a.7z", "a.txt")
        run("x", "-bd", "-y", "-oout", "a.7z")
        assertEquals(text, File(dir, "out/a.txt").readText())
        dir.deleteRecursively()
    }
}
