package com.misterdownloadmanager.android.service

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.FileProvider
import com.misterdownloadmanager.android.BuildConfig
import com.misterdownloadmanager.android.R
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Crash reports, as the desktop app's crash window offers them: when MDM ended in a crash since
 * the last start (Android 11+ keeps the reason, and a native crash's tombstone), the report is
 * written to the app's files with the end of the core's log, and a notification offers to share it.
 */
object CrashReports {
    private const val PREFS = "mdm-android"
    private const val SEEN = "crash_seen_until"

    fun check(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
        val app = context.applicationContext
        Thread({ runCatching { look(app) } }, "crash-reports").start()
    }

    private fun look(app: Context) {
        val prefs = app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val seen = prefs.getLong(SEEN, 0)
        val am = app.getSystemService(ActivityManager::class.java) ?: return
        val exits = am.getHistoricalProcessExitReasons(app.packageName, 0, 10)
        val crash = exits.firstOrNull {
            it.timestamp > seen && it.reason in setOf(
                ApplicationExitInfo.REASON_CRASH,
                ApplicationExitInfo.REASON_CRASH_NATIVE,
                ApplicationExitInfo.REASON_ANR,
            )
        }
        prefs.edit().putLong(SEEN, exits.maxOfOrNull { it.timestamp } ?: seen).apply()
        crash ?: return
        val dir = File(app.filesDir, "crash-reports").apply { mkdirs() }
        val at = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.ROOT).format(Date(crash.timestamp))
        val file = File(dir, "mdm-crash-${crash.timestamp}.txt")
        file.bufferedWriter().use { w ->
            w.appendLine("Mister Download Manager ${BuildConfig.VERSION_NAME} (Android)")
            w.appendLine("Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}), ${Build.MANUFACTURER} ${Build.MODEL}, ${Build.SUPPORTED_ABIS.joinToString()}")
            w.appendLine("Ended: $at, reason ${crash.reason}, ${crash.description ?: ""}")
            w.appendLine()
            runCatching {
                crash.traceInputStream?.use { input ->
                    if (crash.reason == ApplicationExitInfo.REASON_CRASH_NATIVE) {
                        w.appendLine("(native crash: tombstone attached in binary form, ${input.readBytes().size} bytes)")
                    } else {
                        w.appendLine(input.bufferedReader().readText())
                    }
                }
            }
            // The end of the core's log (crates/crash logs), never secrets: as on the desktop.
            val logs = File(app.filesDir, "mdm").walkTopDown().filter { it.isFile && it.name.endsWith(".log") }
                .maxByOrNull { it.lastModified() }
            if (logs != null) {
                w.appendLine("--- ${logs.name} (end)")
                w.appendLine(logs.readLines().takeLast(200).joinToString("\n"))
            }
        }
        val uri = FileProvider.getUriForFile(app, app.packageName + ".files", file)
        val share = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_STREAM, uri)
            .putExtra(Intent.EXTRA_SUBJECT, "Mister Download Manager crash report")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        val pi = android.app.PendingIntent.getActivity(
            app, 7, Intent.createChooser(share, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            android.app.PendingIntent.FLAG_IMMUTABLE or android.app.PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val nm = NotificationManagerCompat.from(app)
        if (!nm.areNotificationsEnabled()) return
        try {
            nm.notify(
                3,
                NotificationCompat.Builder(app, DownloadService.CHANNEL_DONE)
                    .setSmallIcon(R.drawable.ic_stat_download)
                    .setContentTitle(app.getString(R.string.crash_title))
                    .setContentText(app.getString(R.string.crash_text))
                    .setContentIntent(pi)
                    .setAutoCancel(true)
                    .build(),
            )
        } catch (_: SecurityException) {
        }
    }
}
