package com.misterdownloadmanager.android.service

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.util.Log
import com.misterdownloadmanager.android.core.Mdm
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/**
 * "Download on Wi-Fi only": on a metered network (mobile data, a hotspot) the running downloads
 * pause and the main queue stops; back on an unmetered one, the same ones go on.
 */
object NetworkWatch {
    private const val PREFS = "mdm-android"
    private const val WIFI_ONLY = "wifi_only"
    private const val HELD = "held_for_network"
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    @Volatile
    var metered: Boolean = false
        private set

    fun wifiOnly(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(WIFI_ONLY, false)

    fun setWifiOnly(context: Context, on: Boolean) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(WIFI_ONLY, on).apply()
        apply(context.applicationContext)
    }

    fun start(context: Context) {
        val app = context.applicationContext
        val cm = app.getSystemService(ConnectivityManager::class.java) ?: return
        cm.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
            override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
                val now = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
                if (now != metered) {
                    metered = now
                    apply(app)
                }
            }
        })
    }

    private fun apply(app: Context) {
        val prefs = app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        scope.launch {
            if (!Mdm.isStarted) return@launch
            try {
                if (metered && wifiOnly(app)) {
                    val s = Mdm.snapshot.value ?: return@launch
                    val running = s.downloads.filter { it.running }.map { it.id }
                    if (running.isNotEmpty()) Mdm.act("pause", running)
                    val queue = s.queues.firstOrNull()?.takeIf { it.running }
                    if (queue != null) Mdm.runQueue(queue.name, false)
                    val held = (prefs.getStringSet(HELD, emptySet()).orEmpty() + running.map { it.toString() })
                    prefs.edit().putStringSet(HELD, held)
                        .putBoolean("$HELD.queue", queue != null || prefs.getBoolean("$HELD.queue", false)).apply()
                } else {
                    val held = prefs.getStringSet(HELD, emptySet()).orEmpty().mapNotNull { it.toLongOrNull() }
                    if (held.isNotEmpty()) Mdm.act("start", held)
                    if (prefs.getBoolean("$HELD.queue", false)) Mdm.runQueue(null, true)
                    prefs.edit().remove(HELD).remove("$HELD.queue").apply()
                }
            } catch (e: Exception) {
                Log.w("mdm", "network watch", e)
            }
        }
    }
}
