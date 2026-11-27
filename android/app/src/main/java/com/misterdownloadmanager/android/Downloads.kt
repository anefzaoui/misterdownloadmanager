package com.misterdownloadmanager.android

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import org.json.JSONObject

/**
 * Each download's own notification (`Platform.call("downloads")`, android.rs `Notices`): one per
 * running download (its name, a progress bar, its figures, Stop; a tap opens the app), and a "Download
 * complete" one with Open for each that completes while the app runs. Re-posted only when the app
 * says something changed (at most once a second); a download that stops loses its notification.
 */
object Downloads {
    /** Notification ids: the downloads' ids above the service's own (1). */
    private const val BASE = 1000

    /** The running downloads shown now, to take away those that stop. */
    private val shown = mutableSetOf<Int>()

    fun update(context: Context, json: String) {
        val words = JSONObject(json)
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        val open = PendingIntent.getActivity(
            context, 0,
            Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val running = words.getJSONArray("running")
        val now = mutableSetOf<Int>()
        for (i in 0 until running.length()) {
            val d = running.getJSONObject(i)
            val download = d.getLong("id")
            val id = BASE + download.toInt()
            now += id
            val percent = if (d.isNull("percent")) -1 else d.getInt("percent")
            val builder = NotificationCompat.Builder(context, DownloadService.CHANNEL)
                .setSmallIcon(R.drawable.ic_stat_download)
                .setContentTitle(d.getString("name"))
                .setContentText(d.getString("line"))
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setSilent(true)
                .setGroup(GROUP)
                .setContentIntent(open)
                .setProgress(100, percent.coerceAtLeast(0), percent < 0)
                .addAction(
                    0, words.getString("stop"),
                    PendingIntent.getService(
                        context, id, DownloadService.stopIntent(context, download),
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                    ),
                )
            manager.notify(id, builder.build())
        }
        for (gone in shown - now) manager.cancel(gone)
        shown.clear()
        shown += now
        val done = words.getJSONArray("done")
        for (i in 0 until done.length()) {
            val d = done.getJSONObject(i)
            val id = BASE + d.getLong("id").toInt()
            val builder = NotificationCompat.Builder(context, DownloadService.CHANNEL)
                .setSmallIcon(R.drawable.ic_stat_download)
                .setContentTitle(words.getString("complete"))
                .setContentText(d.getString("name"))
                .setSubText(d.getString("line"))
                .setAutoCancel(true)
                .setContentIntent(open)
            Opener.viewIntent(context, d.getString("path"))?.let { view ->
                builder.addAction(
                    0, words.getString("open"),
                    PendingIntent.getActivity(
                        context, id, view,
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                    ),
                )
            }
            manager.notify(id, builder.build())
        }
    }

    private const val GROUP = "com.misterdownloadmanager.android.DOWNLOADS"
}
