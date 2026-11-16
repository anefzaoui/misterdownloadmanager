package com.misterdownloadmanager.android.service

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ProcessLifecycleOwner
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.ui.MainActivity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Keeps the process in the foreground while downloads run, where the download job can't
 * ([DownloadJob]: Android 14+, started from the app in front): before Android 14, and for a
 * start without the app (the Scheduler's alarm, after a restart). Lets go once nothing has run
 * for a moment; the core keeps running in the process.
 */
class DownloadService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var watching = false
    private var inForeground = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        channels(this)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Foreground at once: Android requires it within seconds of startForegroundService.
        try {
            ServiceCompat.startForeground(
                this, Watcher.ONGOING_ID, Watcher.progress(this, Mdm.snapshot.value),
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC else 0,
            )
            inForeground = true
        } catch (e: Exception) {
            Log.w("mdm", "foreground refused", e)
        }
        scope.launch(Dispatchers.IO) { if (!Mdm.start(applicationContext)) stopSelf() }
        Watcher.start(this)
        if (!watching) {
            watching = true
            scope.launch {
                Watcher.ongoing(this@DownloadService).collect { n ->
                    if (inForeground) notify(this@DownloadService, n)
                    delay(1000)
                }
            }
            scope.launch {
                idle()
                ServiceCompat.stopForeground(this@DownloadService, ServiceCompat.STOP_FOREGROUND_REMOVE)
                inForeground = false
                stopSelf()
            }
        }
        return START_STICKY
    }

    /** Android 15+: a dataSync service may run 6 hours a day. Pause, and say so. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        scope.launch(Dispatchers.IO) { pauseRunning() }
        val nm = NotificationManagerCompat.from(this)
        if (nm.areNotificationsEnabled()) {
            try {
                nm.notify(
                    TIMEOUT_ID,
                    NotificationCompat.Builder(this, CHANNEL_DONE)
                        .setSmallIcon(R.drawable.ic_stat_download)
                        .setContentTitle(getString(R.string.notify_time_limit))
                        .setContentText(getString(R.string.notify_time_limit_text))
                        .setContentIntent(openApp(this, null))
                        .setAutoCancel(true)
                        .build(),
                )
            } catch (_: SecurityException) {
            }
        }
        stopSelf()
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        const val CHANNEL_ONGOING = "downloads"
        const val CHANNEL_DONE = "finished"
        private const val TIMEOUT_ID = 2
        private const val IDLE_STOP_MS = 30_000L

        /** Returns once nothing has run for [IDLE_STOP_MS]. */
        suspend fun idle() {
            var since = 0L
            while (true) {
                delay(5_000)
                val s = Mdm.snapshot.value ?: continue
                if (s.running > 0) {
                    since = 0
                } else if (since == 0L) {
                    since = System.currentTimeMillis()
                } else if (System.currentTimeMillis() - since > IDLE_STOP_MS) {
                    return
                }
            }
        }

        suspend fun pauseRunning() {
            val ids = Mdm.snapshot.value?.downloads?.filter { it.running }?.map { it.id }.orEmpty()
            if (ids.isNotEmpty()) Mdm.act("pause", ids)
        }

        fun notify(context: Context, n: android.app.Notification) {
            val nm = NotificationManagerCompat.from(context)
            if (!nm.areNotificationsEnabled()) return
            try {
                nm.notify(Watcher.ONGOING_ID, n)
            } catch (_: SecurityException) {
            }
        }

        /**
         * Keep downloads running while the app isn't in front: the download job on Android 14+
         * when the app is in front now (Android allows starting one only then), else this service.
         */
        fun start(context: Context) {
            Watcher.start(context)
            val visible = ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE && visible && DownloadJob.schedule(context)) {
                return
            }
            try {
                ContextCompat.startForegroundService(context, Intent(context, DownloadService::class.java))
            } catch (e: Exception) {
                // From the background where Android forbids it: the core still runs while the
                // process does.
                Log.w("mdm", "service not started", e)
                val app = context.applicationContext
                Thread({ Mdm.start(app) }, "mdm-start").start()
            }
        }

        fun channels(context: Context) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
            val nm = context.getSystemService(NotificationManager::class.java)
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL_ONGOING, context.getString(R.string.channel_ongoing), NotificationManager.IMPORTANCE_LOW),
            )
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL_DONE, context.getString(R.string.channel_done), NotificationManager.IMPORTANCE_DEFAULT),
            )
        }

        fun openApp(context: Context, id: Long?): PendingIntent {
            val i = Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                if (id != null) putExtra(MainActivity.EXTRA_DOWNLOAD, id)
            }
            return PendingIntent.getActivity(
                context, (id ?: 0).toInt(), i,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }

        fun pauseAll(context: Context): PendingIntent = PendingIntent.getBroadcast(
            context, 11,
            Intent(context, ActionReceiver::class.java).setAction(ActionReceiver.PAUSE_ALL),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }
}

/** The notification's buttons. */
class ActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                if (intent.action == PAUSE_ALL) DownloadService.pauseRunning()
            } finally {
                pending.finish()
            }
        }
    }

    companion object {
        const val PAUSE_ALL = "com.misterdownloadmanager.android.PAUSE_ALL"
    }
}

/**
 * Android 14+: downloads the user started run as a user-initiated data transfer job, which
 * Android lets run as long as it needs (a dataSync service only 6 hours a day on Android 15).
 */
class DownloadJob : android.app.job.JobService() {
    private var scope: CoroutineScope? = null

    override fun onStartJob(params: android.app.job.JobParameters): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return false
        val s = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        scope = s
        setNotification(
            params, Watcher.ONGOING_ID, Watcher.progress(this, Mdm.snapshot.value),
            JOB_END_NOTIFICATION_POLICY_REMOVE,
        )
        Watcher.start(this)
        s.launch(Dispatchers.IO) { Mdm.start(applicationContext) }
        s.launch {
            Watcher.ongoing(this@DownloadJob).collect { n ->
                setNotification(params, Watcher.ONGOING_ID, n, JOB_END_NOTIFICATION_POLICY_REMOVE)
                delay(1000)
            }
        }
        s.launch {
            DownloadService.idle()
            jobFinished(params, false)
            s.cancel()
        }
        return true
    }

    /** Stopped by the system or by the person (Task Manager): the downloads pause where they are. */
    override fun onStopJob(params: android.app.job.JobParameters): Boolean {
        CoroutineScope(Dispatchers.IO).launch { DownloadService.pauseRunning() }
        scope?.cancel()
        return false
    }

    companion object {
        private const val JOB_ID = 4242

        /** Schedule the job unless it runs; false when Android refuses it (then the service). */
        fun schedule(context: Context): Boolean {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return false
            val js = context.getSystemService(JobScheduler::class.java) ?: return false
            if (js.getPendingJob(JOB_ID) != null) return true
            return try {
                val info = JobInfo.Builder(JOB_ID, ComponentName(context, DownloadJob::class.java))
                    .setUserInitiated(true)
                    .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                    .build()
                js.schedule(info) == JobScheduler.RESULT_SUCCESS
            } catch (e: Exception) {
                Log.w("mdm", "download job refused", e)
                false
            }
        }
    }
}
