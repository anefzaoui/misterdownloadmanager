package com.misterdownloadmanager.android.core

import android.content.Context
import android.os.Build
import android.os.Environment
import android.util.Log
import android.webkit.WebSettings
import com.misterdownloadmanager.android.service.Alarms
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import java.io.File

/**
 * The app's one way to the core: starts it, sends requests, and holds the latest snapshot the
 * core sent ([snapshot]) for the interface and the service to show.
 */
object Mdm {
    private const val TAG = "mdm"

    private val _snapshot = MutableStateFlow<Snapshot?>(null)
    val snapshot: StateFlow<Snapshot?> = _snapshot.asStateFlow()

    /** Why the core couldn't start, if it couldn't. */
    private val _startError = MutableStateFlow<String?>(null)
    val startError: StateFlow<String?> = _startError.asStateFlow()

    @Volatile
    private var started = false
    private lateinit var app: Context

    /** The phone's own: Download/MisterDownloadManager (each category's folder inside). */
    fun phoneDir(): File =
        File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "MisterDownloadManager")

    /** The app's folder on each SD card or USB drive (Android lets the app write there without asking). */
    fun cardDirs(context: Context): List<File> =
        context.getExternalFilesDirs(Environment.DIRECTORY_DOWNLOADS).drop(1).filterNotNull()
            .filter { Environment.getExternalStorageState(it) == Environment.MEDIA_MOUNTED }

    /** Where downloads go: the phone's Download folder, or the card chosen in Settings. */
    fun downloadsDir(): File {
        val chosen = if (::app.isInitialized) {
            app.getSharedPreferences("mdm-android", Context.MODE_PRIVATE).getString("downloads_dir", null)
        } else {
            null
        }
        return chosen?.let(::File)?.takeIf { it.parentFile?.exists() == true } ?: phoneDir()
    }

    /** Send new downloads to `dir` (null: the phone's Download folder). */
    suspend fun setDownloadsDir(dir: File?) {
        app.getSharedPreferences("mdm-android", Context.MODE_PRIVATE).edit()
            .putString("downloads_dir", dir?.path).apply()
        request("downloads_dir") { put("dir", (dir ?: phoneDir()).path) }
    }

    /** Start the core once per process (any thread; returns at once when it runs already). */
    @Synchronized
    fun start(context: Context): Boolean {
        if (started) return true
        app = context.applicationContext
        val data = File(app.filesDir, "mdm").apply { mkdirs() }
        val downloads = downloadsDir().apply { mkdirs() }
        val userAgent = runCatching { WebSettings.getDefaultUserAgent(app) }.getOrNull()
        val config = buildJsonObject {
            put("data_dir", data.path)
            put("downloads_dir", downloads.path)
            put("cache_dir", app.cacheDir.path)
            put("native_lib_dir", app.applicationInfo.nativeLibraryDir)
            put("language", app.resources.configuration.locales[0].toLanguageTag())
            if (userAgent != null) put("user_agent", userAgent)
        }
        val error = NativeCore.start(app, config.toString(), Host)
        _startError.value = error
        if (error != null) {
            Log.e(TAG, "core didn't start: $error")
            return false
        }
        started = true
        // The first snapshot at once, rather than at the core's next change.
        Host.changed(NativeCore.request("""{"op":"snapshot"}"""))
        return true
    }

    val isStarted get() = started

    /** A request, off the main thread. */
    suspend fun request(json: JsonObject): JsonObject = withContext(Dispatchers.IO) {
        val answer = NativeCore.request(json.toString())
        runCatching { CoreJson.parseToJsonElement(answer).jsonObject }
            .getOrElse { buildJsonObject { put("error", answer) } }
    }

    suspend fun request(op: String, build: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit = {}): JsonObject =
        request(buildJsonObject { put("op", op); build() })

    suspend fun add(
        urls: List<String>,
        later: Boolean = false,
        category: String? = null,
        referer: String? = null,
    ): AddAnswer {
        val answer = request("add") {
            put("urls", kotlinx.serialization.json.JsonArray(urls.map(::JsonPrimitive)))
            put("later", later)
            if (category != null) put("category", category)
            if (referer != null) put("referer", referer)
        }
        return CoreJson.decodeFromJsonElement(AddAnswer.serializer(), answer)
    }

    suspend fun addTorrentFile(bytes: ByteArray, later: Boolean = false): JsonObject =
        request("add_torrent_file") {
            put("data", android.util.Base64.encodeToString(bytes, android.util.Base64.NO_WRAP))
            put("later", later)
        }

    /**
     * What a link is: `file`, `video`, `torrent`, `gallery`, `none`, or `protected` (a DRM
     * streaming service, with the core's [Kind.message] saying so).
     */
    suspend fun classify(url: String): Kind {
        val r = request("classify") { put("url", url) }
        return Kind(r["kind"]?.jsonPrimitive?.contentOrNull ?: "none", r["message"]?.jsonPrimitive?.contentOrNull)
    }

    data class Kind(val kind: String, val message: String? = null)

    suspend fun act(action: String, ids: Collection<Long>): JsonObject =
        request("act") {
            put("action", action)
            put("ids", kotlinx.serialization.json.JsonArray(ids.map(::JsonPrimitive)))
        }

    suspend fun runQueue(name: String?, run: Boolean): JsonObject =
        request("queue") {
            if (name != null) put("queue", name)
            put("run", run)
        }

    suspend fun details(id: Long): Details? {
        val answer = request("details") { put("id", id) }
        if (answer.containsKey("error")) return null
        return runCatching { CoreJson.decodeFromJsonElement(Details.serializer(), answer) }.getOrNull()
    }

    suspend fun settings(): JsonObject = request("settings")

    suspend fun setSettings(patch: JsonObject): JsonObject = request("set_settings") { put("patch", patch) }

    suspend fun speedLimit(on: Boolean?, kb: Long?): JsonObject = request("speed_limit") {
        if (on != null) put("on", on)
        if (kb != null) put("kb", kb)
    }

    suspend fun videoLookup(address: String, title: String? = null): VideoLookup =
        video(request("video_lookup") {
            put("address", address)
            if (title != null) put("title", title)
        })

    suspend fun videoPoll(token: String): VideoLookup = video(request("video_poll") { put("token", token) })

    suspend fun videoPick(token: String, choice: Int): VideoLookup =
        video(request("video_pick") {
            put("token", token)
            put("choice", choice)
        })

    private fun video(o: JsonObject): VideoLookup =
        runCatching { CoreJson.decodeFromJsonElement(VideoLookup.serializer(), o) }
            .getOrElse { VideoLookup(state = "failed", error = o.toString()) }

    /** Look a torrent's files up (a magnet link asks the swarm): its token. */
    suspend fun torrentLookup(magnet: String?, data: ByteArray?): String? = request("torrent_lookup") {
        if (magnet != null) put("url", magnet)
        if (data != null) put("data", android.util.Base64.encodeToString(data, android.util.Base64.NO_WRAP))
    }["token"]?.jsonPrimitive?.contentOrNull

    suspend fun torrentPoll(token: String): TorrentLookup =
        CoreJson.decodeFromJsonElement(TorrentLookup.serializer(), request("torrent_poll") { put("token", token) })

    /** Add a looked-up torrent with the files chosen (all when empty). */
    suspend fun torrentAdd(token: String, files: List<Long>, later: Boolean, category: String?, inOrder: Boolean): JsonObject =
        request("torrent_add") {
            put("token", token)
            put("ids", kotlinx.serialization.json.JsonArray(files.map(::JsonPrimitive)))
            put("later", later)
            put("on", inOrder)
            if (category != null) put("category", category)
        }

    suspend fun playlistStart(url: String): String? =
        request("playlist_start") { put("url", url) }["token"]?.jsonPrimitive?.contentOrNull

    suspend fun playlistPoll(token: String): PlaylistListing =
        CoreJson.decodeFromJsonElement(PlaylistListing.serializer(), request("playlist_poll") { put("token", token) })

    suspend fun playlistAdd(token: String, entries: List<Long>, choice: Int, later: Boolean): JsonObject =
        request("playlist_add") {
            put("token", token)
            put("ids", kotlinx.serialization.json.JsonArray(entries.map(::JsonPrimitive)))
            put("choice", choice)
            put("later", later)
        }

    suspend fun helpers(): Helpers =
        CoreJson.decodeFromJsonElement(Helpers.serializer(), request("helpers"))

    /** Install (or update) yt-dlp from its GitHub releases, checked against its signature. */
    suspend fun installYtdlp(): Helpers {
        request("install_ytdlp")
        var h = helpers()
        while (h.installing != null) {
            kotlinx.coroutines.delay(500)
            h = helpers()
        }
        return h
    }

    /** Install (or update) gallery-dl, checked against its signature. */
    suspend fun installGallery(): Helpers {
        request("install_gallery")
        var h = helpers()
        while (h.installing != null) {
            kotlinx.coroutines.delay(500)
            h = helpers()
        }
        return h
    }

    suspend fun addGallery(url: String, later: Boolean): JsonObject = request("gallery_add") {
        put("url", url)
        put("later", later)
    }

    suspend fun languages(): Languages =
        CoreJson.decodeFromJsonElement(Languages.serializer(), request("languages"))

    suspend fun setLanguage(code: String?): JsonObject = request("set_language") {
        put("code", code?.let(::JsonPrimitive) ?: JsonNull)
    }

    suspend fun dismissNotice() = request("dismiss_notice")

    /** Texts in the interface language (the core's catalogs in i18n/). */
    suspend fun translate(texts: List<String>): List<String> {
        val answer = request("tr") { put("texts", kotlinx.serialization.json.JsonArray(texts.map(::JsonPrimitive))) }
        return (answer["texts"] as? kotlinx.serialization.json.JsonArray)?.map { it.jsonPrimitive.content } ?: texts
    }

    /** What the core calls back. */
    private object Host : NativeHost {
        override fun changed(snapshot: String) {
            runCatching { CoreJson.decodeFromString(Snapshot.serializer(), snapshot) }
                .onSuccess { _snapshot.value = it }
                .onFailure { Log.w(TAG, "snapshot unreadable: $it") }
        }

        override fun call(json: String): String {
            val req = runCatching { Json.parseToJsonElement(json).jsonObject }.getOrNull()
                ?: return error("bad call")
            return try {
                when (req.str("fn")) {
                    "keychain" -> keychain(req)
                    "wake_at" -> {
                        Alarms.wakeAt(app, req["at"]?.jsonPrimitive?.longOrNull)
                        ok(JsonNull)
                    }
                    // The download service holds the locks while anything runs (DownloadService).
                    "keep_awake" -> ok(JsonPrimitive(true))
                    else -> error("unknown call")
                }
            } catch (e: Exception) {
                Log.w(TAG, "host call failed", e)
                error(e.message ?: e.toString())
            }
        }

        private fun keychain(req: JsonObject): String {
            val service = req.str("service") ?: ""
            val account = req.str("account") ?: ""
            val store = Secrets(app)
            return when (req.str("op")) {
                "probe" -> ok(JsonNull)
                "read" -> ok(store.read(service, account)?.let(::JsonPrimitive) ?: JsonNull)
                "write" -> {
                    store.write(service, account, req.str("secret") ?: "")
                    ok(JsonNull)
                }
                "delete" -> {
                    store.delete(service, account)
                    ok(JsonNull)
                }
                "list" -> ok(JsonPrimitive(store.list(service).joinToString("\n")))
                else -> error("unknown keychain op")
            }
        }

        private fun JsonObject.str(key: String): String? =
            (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.contentOrNull

        private fun ok(value: JsonElement) = buildJsonObject { put("value", value) }.toString()
        private fun error(why: String) = buildJsonObject { put("error", why) }.toString()
    }

    @Suppress("unused")
    private fun JsonObject.bool(key: String) = this[key]?.jsonPrimitive?.booleanOrNull

    @Suppress("unused")
    private val sdk = Build.VERSION.SDK_INT
}
