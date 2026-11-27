package com.misterdownloadmanager.android

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

/**
 * Keeps the process, and with it the app and its downloads, running while the app isn't in
 * front: a foreground service with one ongoing notification, from the moment downloads start (the
 * app's `keep_awake`, the desktop's hold on sleep) until they stop.
 */
class DownloadService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // The notification's Stop All: to the app, which stops every download (the service
        // stops when they have).
        if (intent?.action == ACTION_STOP_ALL) {
            nativeStopAll()
            return START_NOT_STICKY
        }
        // A download's own notification's Stop.
        if (intent?.action == ACTION_STOP) {
            nativeStop(intent.getLongExtra(EXTRA_ID, -1))
            return START_NOT_STICKY
        }
        val text = intent?.getStringExtra(EXTRA_TEXT) ?: getString(R.string.app_name)
        try {
            ServiceCompat.startForeground(
                this, ONGOING_ID, notification(this, text, stopLabel),
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC else 0,
            )
            running = true
        } catch (e: Exception) {
            Log.w("mdm", "foreground refused", e)
            stopSelf()
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        running = false
        super.onDestroy()
    }

    /** Android 15 and later give a data sync service 6 hours a day: the downloads go on while the app is in front. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        stopSelf()
    }

    /** libmdm.so: the notification's Stop All. */
    private external fun nativeStopAll()

    /** libmdm.so: a download's notification's Stop. */
    private external fun nativeStop(id: Long)

    companion object {
        const val CHANNEL = "downloads"
        private const val ONGOING_ID = 1
        private const val EXTRA_TEXT = "text"
        private const val ACTION_STOP_ALL = "com.misterdownloadmanager.android.STOP_ALL"
        private const val ACTION_STOP = "com.misterdownloadmanager.android.STOP"
        private const val EXTRA_ID = "id"

        /** What a download's notification's Stop sends: that download's id, to the app. */
        fun stopIntent(context: Context, id: Long): Intent =
            Intent(context, DownloadService::class.java).setAction(ACTION_STOP).putExtra(EXTRA_ID, id)

        /** Whether the service is in the foreground (its notification shown). */
        @Volatile
        private var running = false

        /** Stop All's label, in the app's language (the app gives it with the status). */
        @Volatile
        private var stopLabel: String? = null

        fun start(context: Context, text: String) {
            try {
                ContextCompat.startForegroundService(
                    context, Intent(context, DownloadService::class.java).putExtra(EXTRA_TEXT, text),
                )
            } catch (e: Exception) {
                // From the background, where Android forbids it: the downloads go on while the
                // process does.
                Log.w("mdm", "download service not started", e)
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, DownloadService::class.java))
        }

        /** How the downloads go ("2 downloading, 1.25 MB/sec"), in the notification while it shows. */
        fun status(context: Context, text: String, stop: String) {
            stopLabel = stop
            if (!running) return
            context.getSystemService(NotificationManager::class.java)
                .notify(ONGOING_ID, notification(context, text, stop))
        }

        fun channels(context: Context) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
            context.getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL, context.getString(R.string.app_name), NotificationManager.IMPORTANCE_LOW),
            )
        }

        private fun notification(context: Context, text: String, stop: String?): android.app.Notification {
            val builder = NotificationCompat.Builder(context, CHANNEL)
                .setSmallIcon(R.drawable.ic_stat_download)
                .setContentTitle(context.getString(R.string.app_name))
                .setContentText(text)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setContentIntent(
                    PendingIntent.getActivity(
                        context, 0,
                        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                    ),
                )
            if (stop != null) {
                builder.addAction(
                    0, stop,
                    PendingIntent.getService(
                        context, 1,
                        Intent(context, DownloadService::class.java).setAction(ACTION_STOP_ALL),
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                    ),
                )
            }
            return builder.build()
        }
    }
}
