package com.misterdownloadmanager.android.service

import android.annotation.SuppressLint
import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.wifi.WifiManager
import android.os.Build
import android.os.PowerManager
import android.util.Log

/** The Scheduler's start times (the core's `wake_at`): an exact alarm that starts the service. */
object Alarms {
    private const val NEXT = "next_alarm"

    /** After a restart: the Scheduler's next start again, if it is still ahead. */
    fun restore(context: Context) {
        val at = context.getSharedPreferences("mdm-android", Context.MODE_PRIVATE).getLong(NEXT, 0)
        if (at * 1000 > System.currentTimeMillis()) wakeAt(context, at)
    }

    private fun intent(context: Context) = PendingIntent.getBroadcast(
        context, 1, Intent(context, AlarmReceiver::class.java),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    /** At `at` (seconds since 1970, UTC), or cancel with null. */
    fun wakeAt(context: Context, at: Long?) {
        val am = context.getSystemService(AlarmManager::class.java)
        val pi = intent(context)
        // Kept, so a restart of the phone (which clears every alarm) sets it again (BootReceiver).
        context.getSharedPreferences("mdm-android", Context.MODE_PRIVATE).edit().apply {
            if (at == null) remove(NEXT) else putLong(NEXT, at)
        }.apply()
        if (at == null) {
            am.cancel(pi)
            return
        }
        val millis = at * 1000
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S || am.canScheduleExactAlarms()) {
            am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, millis, pi)
        } else {
            // Exact alarms not allowed (Settings → Alarms & reminders): within a few minutes.
            am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, millis, pi)
        }
    }
}

/** The alarm: the service starts, and with it the core, whose Scheduler starts the queue. */
class AlarmReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        DownloadService.start(context)
    }
}

/** After a restart or an update: the core resumes what was running and sets its alarms again. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Alarms.restore(context)
        // Only when something was downloading: otherwise nothing needs the app now.
        if (!Watcher.wasRunning(context)) return
        // Android 15 lets no dataSync service start from a restart: say so, and let a tap go on.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.VANILLA_ICE_CREAM &&
            intent.action == Intent.ACTION_BOOT_COMPLETED
        ) {
            Watcher.notifyInterrupted(context)
            return
        }
        DownloadService.start(context)
    }
}

/** A partial wake lock and a Wi-Fi lock while downloads run (the core's `keep_awake`). */
object Locks {
    private var wake: PowerManager.WakeLock? = null
    private var wifi: WifiManager.WifiLock? = null
    private var multicast: WifiManager.MulticastLock? = null

    @SuppressLint("WakelockTimeout")
    @Synchronized
    fun hold(context: Context, reason: String?): Boolean {
        val app = context.applicationContext
        if (reason == null) {
            wake?.takeIf { it.isHeld }?.release()
            wifi?.takeIf { it.isHeld }?.release()
            multicast?.takeIf { it.isHeld }?.release()
            wake = null
            wifi = null
            multicast = null
            return true
        }
        return try {
            if (wake == null) {
                wake = app.getSystemService(PowerManager::class.java)
                    .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "mdm:downloads")
                    .apply { setReferenceCounted(false); acquire() }
            }
            val wm = app.getSystemService(WifiManager::class.java)
            if (wifi == null) {
                @Suppress("DEPRECATION")
                val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                } else {
                    WifiManager.WIFI_MODE_FULL_HIGH_PERF
                }
                wifi = wm.createWifiLock(mode, "mdm:downloads").apply { setReferenceCounted(false); acquire() }
            }
            // Torrents' UPnP and local peer discovery need multicast packets on Wi-Fi.
            if (multicast == null) {
                multicast = wm.createMulticastLock("mdm:torrents").apply { setReferenceCounted(false); acquire() }
            }
            true
        } catch (e: Exception) {
            Log.w("mdm", "locks", e)
            false
        }
    }
}
