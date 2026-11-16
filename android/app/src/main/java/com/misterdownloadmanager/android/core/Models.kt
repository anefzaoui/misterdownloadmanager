package com.misterdownloadmanager.android.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement

/** The JSON the core speaks (crates/app/src/android_ui.rs). */
val CoreJson = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = false
}

@Serializable
data class Download(
    val id: Long,
    val file: String,
    val url: String,
    val path: String,
    /** Queued, Downloading, Stopped, Complete, Error. */
    val status: String,
    /** Probing, Downloading, Verifying, Merging, Completing: what the engine is doing while it runs. */
    val stage: String? = null,
    val size: Long? = null,
    val downloaded: Long = 0,
    val fraction: Double? = null,
    val speed: Long = 0,
    val eta: Long? = null,
    val connections: Int = 0,
    val category: String = "General",
    /** file, video, torrent, gallery. */
    val kind: String = "file",
    val error: String? = null,
    val added: String? = null,
    val resumable: Boolean? = null,
    @kotlinx.serialization.SerialName("can_start") val canStart: Boolean = false,
    val running: Boolean = false,
) {
    val complete get() = status == "Complete"
    val failed get() = status == "Error"
}

@Serializable
data class QueueInfo(val name: String, val running: Boolean, val ids: List<Long> = emptyList())

@Serializable
data class CategoryInfo(val key: String, val name: String)

@Serializable
data class Snapshot(
    val downloads: List<Download> = emptyList(),
    val speed: Long = 0,
    val queues: List<QueueInfo> = emptyList(),
    val categories: List<CategoryInfo> = emptyList(),
    val free: Long? = null,
    val language: String = "en",
    val rtl: Boolean = false,
    @kotlinx.serialization.SerialName("speed_limit_on") val speedLimitOn: Boolean = false,
    val notice: List<String>? = null,
) {
    val running get() = downloads.count { it.running }
}

@Serializable
data class AddAnswer(
    val ids: List<Long> = emptyList(),
    val pending: Int = 0,
    val errors: List<JsonElement> = emptyList(),
)

/** The video look-up (mdm_ipc::api::VideoLookupAnswer): `pending`, `ready`, `failed`, `list`, `busy`, `gone`, `use-window`, `started`, `queued`, `sent`. */
@Serializable
data class VideoLookup(
    val state: String = "",
    val token: String? = null,
    val title: String? = null,
    val choices: List<VideoChoice> = emptyList(),
    val reason: String? = null,
    val message: String? = null,
    val notice: VideoNotice? = null,
    /** The core's own error answer (`{"error": …}`). */
    val error: String? = null,
)

@Serializable
data class VideoNotice(val text: String = "", val link: String = "", val url: String = "")

@Serializable
data class VideoChoice(
    val label: String = "",
    val ext: String = "",
    val size: Long? = null,
    val audio: Boolean = false,
    val height: Int? = null,
    val fps: Int? = null,
)

@Serializable
data class Details(
    val download: Download,
    val description: String = "",
    val referer: String? = null,
    val page: String? = null,
    /** [start, cursor, end] of each part. */
    val segments: List<List<Long>>? = null,
    val connections: List<Connection>? = null,
    val server: Server? = null,
    val log: List<LogLine> = emptyList(),
)

/**
 * Where a download's server is, in the desktop's words (crates/app/src/whereis.rs): the place,
 * its country's code (its flag), the round trip, and what the desktop's tooltip says.
 */
@Serializable
data class Server(
    val place: String,
    val country: String? = null,
    val time: String? = null,
    val reach: String? = null,
    val about: String = "",
)

@Serializable
data class Connection(val downloaded: Long = 0, val speed: Long = 0, val status: String = "")

@Serializable
data class LogLine(val at: Long = 0, val text: String = "")

/**
 * The video helpers: yt-dlp's version (none: not installed), ffmpeg's, gallery-dl's, which one is
 * being installed (`ytdlp` or `gallery`), and each one's last install's failure.
 */
@Serializable
data class Helpers(
    val ytdlp: String? = null,
    val ffmpeg: String? = null,
    val gallery: String? = null,
    val installing: String? = null,
    @SerialName("ytdlp_error") val ytdlpError: String? = null,
    @SerialName("gallery_error") val galleryError: String? = null,
)

@Serializable
data class TorrentFile(val index: Long, val path: String, val size: Long = 0)

/** A torrent being looked up: `pending`, `ready` (name, size, files), `failed`, `gone`. */
@Serializable
data class TorrentLookup(
    val state: String = "",
    val name: String = "",
    val size: Long = 0,
    val files: List<TorrentFile> = emptyList(),
    val known: Boolean = false,
    val message: String? = null,
)

@Serializable
data class PlaylistEntry(val index: Long, val title: String = "", val duration: Double? = null, val unavailable: Boolean = false)

/** A playlist or channel being listed: `listing`, `done`, `gone`. */
@Serializable
data class PlaylistListing(
    val state: String = "",
    val title: String? = null,
    val total: Long? = null,
    val error: String? = null,
    val entries: List<PlaylistEntry> = emptyList(),
    val picks: List<String> = emptyList(),
)

@Serializable
data class Language(val code: String, val name: String)

@Serializable
data class Languages(
    val languages: List<Language> = emptyList(),
    val chosen: String? = null,
    val current: String = "en",
)
