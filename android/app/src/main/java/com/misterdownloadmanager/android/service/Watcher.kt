package com.misterdownloadmanager.android.service

import android.app.Notification
import android.content.Context
import android.media.MediaScannerConnection
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Download
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.core.Snapshot
import com.misterdownloadmanager.android.ui.Format
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.distinctUntilChangedBy
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/**
 * What happens whatever keeps the process running (the download job, the service, the app in
 * front): a finished download is scanned so Files and Gallery see it and told in a notification,
 * a failed one too, and the wake and Wi-Fi locks are held while anything runs.
 */
object Watcher {
    const val ONGOING_ID = 1
    private val lastStatus = HashMap<Long, String>()
    private var started = false
    private var lastRunning: Boolean? = null
    private const val WAS_RUNNING = "was_running"
    private const val INTERRUPTED_ID = 4

    /** Whether downloads were running when the process last saw them. */
    fun wasRunning(context: Context) =
        context.getSharedPreferences("mdm-android", Context.MODE_PRIVATE).getBoolean(WAS_RUNNING, false)

    /** After a restart where Android won't let downloads go on by themselves: a tap does. */
    fun notifyInterrupted(context: Context) {
        val nm = NotificationManagerCompat.from(context)
        if (!nm.areNotificationsEnabled()) return
        try {
            nm.notify(
                INTERRUPTED_ID,
                NotificationCompat.Builder(context, DownloadService.CHANNEL_DONE)
                    .setSmallIcon(R.drawable.ic_stat_download)
                    .setContentTitle(context.getString(R.string.notify_restart))
                    .setContentText(context.getString(R.string.notify_restart_text))
                    .setContentIntent(DownloadService.openApp(context, null))
                    .setAutoCancel(true)
                    .build(),
            )
        } catch (_: SecurityException) {
        }
    }

    fun start(context: Context) {
        if (started) return
        started = true
        val app = context.applicationContext
        CoroutineScope(SupervisorJob() + Dispatchers.Main).launch {
            Mdm.snapshot.filterNotNull().collect { onSnapshot(app, it) }
        }
    }

    private fun onSnapshot(app: Context, s: Snapshot) {
        Locks.hold(app, if (s.running > 0) "downloads" else null)
        // Remembered across a restart of the phone (BootReceiver).
        val running = s.running > 0
        if (running != lastRunning) {
            lastRunning = running
            app.getSharedPreferences("mdm-android", Context.MODE_PRIVATE).edit().putBoolean(WAS_RUNNING, running).apply()
        }
        for (d in s.downloads) {
            val before = lastStatus.put(d.id, d.status)
            if (before != null && before != d.status) {
                when (d.status) {
                    "Complete" -> {
                        MediaScannerConnection.scanFile(app, arrayOf(d.path), null, null)
                        notifyOne(app, d, app.getString(R.string.notify_complete), d.file)
                    }
                    "Error" -> notifyOne(
                        app, d, app.getString(R.string.notify_failed),
                        listOfNotNull(d.file, d.error).joinToString(" — "),
                    )
                }
            }
        }
    }

    private fun notifyOne(app: Context, d: Download, title: String, text: String) {
        val nm = NotificationManagerCompat.from(app)
        if (!nm.areNotificationsEnabled()) return
        val n = NotificationCompat.Builder(app, DownloadService.CHANNEL_DONE)
            .setSmallIcon(R.drawable.ic_stat_download)
            .setContentTitle(title)
            .setContentText(text)
            .setAutoCancel(true)
            .setContentIntent(DownloadService.openApp(app, d.id))
            .build()
        try {
            nm.notify((d.id % Int.MAX_VALUE).toInt() + 1000, n)
        } catch (_: SecurityException) {
        }
    }

    /**
     * The ongoing notification as downloads go: at most once a second, and only when what it
     * shows changed. The core sends up to 4 snapshots a second; Android drops an app's
     * notification updates past a few a second (some phone makers sooner), which left the
     * notification frozen until the app was opened.
     */
    fun ongoing(context: Context): Flow<Notification> =
        Mdm.snapshot.filterNotNull()
            .distinctUntilChangedBy { shown(context, it) }
            .conflate()
            .map { progress(context, it) }

    /** What the ongoing notification shows of [s]: when this changes, it is posted again. */
    private fun shown(context: Context, s: Snapshot): List<String> {
        val running = s.downloads.filter { it.running }
        return listOf(Format.speed(s.speed), percent(running).toString()) +
            running.take(5).map { line(it) }
    }

    private fun percent(running: List<Download>): Int {
        val total = running.sumOf { it.size ?: 0 }
        return if (total > 0) (running.sumOf { it.downloaded } * 100 / total).toInt() else -1
    }

    private fun line(d: Download) = "${d.file} · ${d.fraction?.let { Format.percent(it) } ?: Format.size(d.downloaded)}"

    /** The ongoing notification: how many run, how fast, how far; Pause all. */
    fun progress(context: Context, s: Snapshot?): Notification {
        val running = s?.downloads?.filter { it.running }.orEmpty()
        val title = if (running.isEmpty()) {
            context.getString(R.string.notify_idle)
        } else {
            context.resources.getQuantityString(R.plurals.notify_running, running.size, running.size)
        }
        val total = running.sumOf { it.size ?: 0 }
        val done = running.sumOf { it.downloaded }
        val b = NotificationCompat.Builder(context, DownloadService.CHANNEL_ONGOING)
            .setSmallIcon(R.drawable.ic_stat_download)
            .setContentTitle(title)
            // The share done beside the speed: some phones hide a notification's bar.
            .setContentText(
                if (running.isEmpty()) {
                    ""
                } else {
                    listOfNotNull(
                        percent(running).takeIf { it >= 0 }?.let { Format.percent(it / 100.0) },
                        Format.speed(s?.speed ?: 0),
                    ).joinToString(" · ")
                },
            )
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setShowWhen(false)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(DownloadService.openApp(context, null))
        if (running.isNotEmpty()) {
            if (total > 0) b.setProgress(1000, (done * 1000 / total).toInt(), false) else b.setProgress(0, 0, true)
            b.addAction(0, context.getString(R.string.action_pause_all), DownloadService.pauseAll(context))
            b.setStyle(
                NotificationCompat.InboxStyle().also { st ->
                    running.take(5).forEach { d -> st.addLine(line(d)) }
                },
            )
        }
        return b.build()
    }
}
