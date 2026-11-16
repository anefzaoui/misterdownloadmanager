package com.misterdownloadmanager.android

import com.misterdownloadmanager.android.core.CategoryInfo
import com.misterdownloadmanager.android.core.Download
import com.misterdownloadmanager.android.core.QueueInfo
import com.misterdownloadmanager.android.core.Snapshot

/** Realistic data for the screenshots. */
object Samples {
    private const val MB = 1L shl 20
    val downloads = listOf(
        Download(1, "ubuntu-24.04.1-desktop-amd64.iso", "https://releases.ubuntu.com/24.04/ubuntu-24.04.1-desktop-amd64.iso",
            "/storage/emulated/0/Download/MisterDownloadManager/Programs/ubuntu-24.04.1-desktop-amd64.iso",
            "Downloading", "Downloading", 5900 * MB, 2310 * MB, 0.39, 14_600_000, 245, 8, "Programs", "file", running = true, resumable = true),
        Download(2, "Big Buck Bunny (4K).mp4", "https://www.youtube.com/watch?v=aqz-KE-bpKQ",
            "/storage/emulated/0/Download/MisterDownloadManager/Video/Big Buck Bunny (4K).mp4",
            "Downloading", "Merging", 642 * MB, 641 * MB, 0.999, 0, null, 0, "Video", "video", running = true),
        Download(3, "debian-12.7.0-amd64-netinst.iso [torrent]", "magnet:?xt=urn:btih:abc",
            "/storage/emulated/0/Download/MisterDownloadManager/Torrents/debian", "Downloading", "Downloading",
            661 * MB, 120 * MB, 0.18, 3_100_000, 174, 23, "Torrents", "torrent", running = true),
        Download(4, "Annual report 2025.pdf", "https://example.org/report.pdf", "/storage/emulated/0/Download/MisterDownloadManager/Documents/Annual report 2025.pdf",
            "Complete", null, 18 * MB, 18 * MB, 1.0, 0, null, 0, "Documents", "file", added = "Nov 15 10:02 2026"),
        Download(5, "podcast-episode-212.mp3", "https://cdn.example.net/ep212.mp3", "/storage/emulated/0/Download/MisterDownloadManager/Music/podcast-episode-212.mp3",
            "Stopped", null, 74 * MB, 31 * MB, 0.42, 0, null, 0, "Music", "file", canStart = true, resumable = true),
        Download(6, "photos-backup.zip", "https://files.example.com/photos-backup.zip", "/storage/emulated/0/Download/MisterDownloadManager/Compressed/photos-backup.zip",
            "Error", null, null, 0, null, 0, null, 0, "Compressed", "file", error = "The server refused the download (403 Forbidden)", canStart = true),
        Download(7, "nightly-build.apk", "https://example.dev/nightly.apk", "/storage/emulated/0/Download/MisterDownloadManager/Programs/nightly-build.apk",
            "Queued", null, 92 * MB, 0, 0.0, 0, null, 0, "Programs", "file", canStart = true),
    )
    val categories = listOf("General", "Compressed", "Documents", "Music", "Programs", "Video", "Torrents").map { CategoryInfo(it, it) }
    val snapshot = Snapshot(
        downloads = downloads,
        speed = 17_700_000,
        queues = listOf(QueueInfo("Main download queue", true, listOf(7)), QueueInfo("Synchronization queue", false)),
        categories = categories,
        free = 41L * 1024 * MB,
    )
    val video = com.misterdownloadmanager.android.core.VideoLookup(
        state = "ready",
        token = "t1",
        title = "Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film",
        choices = listOf(
            com.misterdownloadmanager.android.core.VideoChoice("2160p 60 fps · VP9 · mp4", "mp4", 1_620 * MB, height = 2160, fps = 60),
            com.misterdownloadmanager.android.core.VideoChoice("1080p 60 fps · AV1 · mp4", "mp4", 642 * MB, height = 1080, fps = 60),
            com.misterdownloadmanager.android.core.VideoChoice("720p 60 fps · H.264 · mp4", "mp4", 301 * MB, height = 720, fps = 60),
            com.misterdownloadmanager.android.core.VideoChoice("480p · H.264 · mp4", "mp4", 118 * MB, height = 480),
            com.misterdownloadmanager.android.core.VideoChoice("Original sound · Opus 160 kbit/s", "opus", 12 * MB, audio = true),
            com.misterdownloadmanager.android.core.VideoChoice("MP3 · 320 kbit/s", "mp3", 24 * MB, audio = true),
        ),
    )
    val empty = Snapshot(categories = categories, queues = snapshot.queues, free = 41L * 1024 * MB)
}

object DetailSamples {
    private const val MB = 1L shl 20
    val running = com.misterdownloadmanager.android.core.Details(
        download = Samples.downloads[0],
        segments = listOf(
            listOf(0L, 610 * MB, 737 * MB), listOf(737 * MB, 1050 * MB, 1475 * MB), listOf(1475 * MB, 1702 * MB, 2212 * MB),
            listOf(2212 * MB, 2500 * MB, 2950 * MB), listOf(2950 * MB, 3120 * MB, 3687 * MB), listOf(3687 * MB, 3801 * MB, 4425 * MB),
            listOf(4425 * MB, 4510 * MB, 5162 * MB), listOf(5162 * MB, 5199 * MB, 5900 * MB),
        ),
        server = com.misterdownloadmanager.android.core.Server(
            place = "United Kingdom",
            country = "GB",
            time = "about 14 ms (near)",
            reach = "near",
            about = "Address: 91.189.91.38\nThe time is a round trip, measured as this download's own connections opened.\nIP Geolocation by DB-IP (db-ip.com)",
        ),
        log = listOf(
            com.misterdownloadmanager.android.core.LogLine(0, "Connected to releases.ubuntu.com (91.189.91.38)"),
            com.misterdownloadmanager.android.core.LogLine(0, "The server accepts ranges: 8 connections"),
        ),
    )
}
