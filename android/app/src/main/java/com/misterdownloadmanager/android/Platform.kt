package com.misterdownloadmanager.android

import android.annotation.SuppressLint
import android.app.Activity
import android.app.AlarmManager
import android.app.DownloadManager
import android.app.PendingIntent
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.PowerManager
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import java.io.File
import java.lang.ref.WeakReference
import java.security.KeyStore
import java.util.concurrent.atomic.AtomicLong
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * What only Android's Java side can do for the app (crates/app/src/android.rs `Host`, called
 * from libmdm.so's `JavaHost`): the keystore for sign-ins, the Scheduler's exact alarm, and while
 * downloads run the wake and Wi-Fi locks and the foreground service that keeps the process alive.
 *
 * One entry, [call], so the native side needs a single method signature; it answers `ok:<value>`,
 * `none`, or `err:<why>`.
 */
object Platform {
    private lateinit var app: Context

    /** The activity on screen, for what needs one (Back sending the app to the background). */
    private var activity = WeakReference<Activity>(null)

    /** How many clips were copied since the start: the watch for copied links reads only on a change. */
    private val clips = AtomicLong()

    private var listening = false

    fun init(activity: Activity) {
        app = activity.applicationContext
        this.activity = WeakReference(activity)
        DownloadService.channels(app)
        if (!listening) {
            app.getSystemService(ClipboardManager::class.java)
                ?.addPrimaryClipChangedListener { clips.incrementAndGet() }
            listening = true
        }
    }

    /** The clipboard: `get` its text (Android lets only the app in front read it), `set` it, or `changes`. */
    private fun clipboard(op: String, text: String?): String {
        val board = app.getSystemService(ClipboardManager::class.java) ?: return "err:no clipboard"
        return when (op) {
            "get" -> board.primaryClip?.takeIf { it.itemCount > 0 }
                ?.getItemAt(0)?.coerceToText(app)?.toString()
                ?.let { "ok:$it" } ?: "none"
            "set" -> {
                board.setPrimaryClip(ClipData.newPlainText(app.getString(R.string.app_name), text.orEmpty()))
                "ok:"
            }
            "changes" -> "ok:${clips.get()}"
            else -> "err:unknown clipboard op $op"
        }
    }

    // An instance method (the object's): libmdm.so calls it on the object it was given.
    fun call(what: String, a: String?, b: String?, c: String?, d: String?): String? = try {
        when (what) {
            "keychain" -> keychain(a.orEmpty(), b.orEmpty(), c.orEmpty(), d)
            "wakeAt" -> {
                Alarms.wakeAt(app, a?.toLongOrNull())
                "ok:"
            }
            "keepAwake" -> if (Locks.hold(app, a)) {
                if (a != null) DownloadService.start(app, a) else DownloadService.stop(app)
                "ok:"
            } else {
                "err:the locks were refused"
            }
            "clipboard" -> clipboard(a.orEmpty(), b)
            "downloads" -> {
                Downloads.update(app, a.orEmpty())
                "ok:"
            }
            "pick" -> activity.get()?.let { Picker.pick(it, a.orEmpty()) }?.let { "ok:$it" } ?: "none"
            "status" -> {
                DownloadService.status(app, a.orEmpty(), b.orEmpty())
                "ok:"
            }
            "open" -> {
                Opener.open(activity.get() ?: app, a.orEmpty(), b.orEmpty())
                "ok:"
            }
            "toBack" -> activity.get()?.let {
                it.runOnUiThread { it.moveTaskToBack(true) }
                "ok:"
            } ?: "err:no activity"
            else -> "err:unknown call $what"
        }
    } catch (e: Exception) {
        Log.w("mdm", "platform $what", e)
        "err:${e.message ?: e.javaClass.simpleName}"
    }

    private fun keychain(op: String, service: String, account: String, secret: String?): String {
        val secrets = Secrets(app)
        return when (op) {
            "probe" -> "none"
            "read" -> secrets.read(service, account)?.let { "ok:$it" } ?: "none"
            "write" -> {
                secrets.write(service, account, secret.orEmpty())
                "ok:"
            }
            "delete" -> {
                secrets.delete(service, account)
                "ok:"
            }
            "list" -> "ok:" + secrets.list(service).joinToString("\n")
            else -> "err:unknown keychain op $op"
        }
    }
}

/**
 * Sign-ins encrypted with an AES key that never leaves Android's keystore, kept in the app's
 * private preferences; by service and account, as on the desktop's keychains.
 */
class Secrets(context: Context) {
    private val prefs = context.getSharedPreferences("mdm-secrets", Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    private fun name(service: String, account: String) = "$service\u0000$account"

    fun read(service: String, account: String): String? {
        val stored = prefs.getString(name(service, account), null) ?: return null
        val bytes = Base64.decode(stored, Base64.NO_WRAP)
        if (bytes.size < IV) return null
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, IV))
        return String(cipher.doFinal(bytes, IV, bytes.size - IV), Charsets.UTF_8)
    }

    fun write(service: String, account: String, secret: String) {
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.iv + cipher.doFinal(secret.toByteArray(Charsets.UTF_8))
        prefs.edit().putString(name(service, account), Base64.encodeToString(sealed, Base64.NO_WRAP)).apply()
    }

    fun delete(service: String, account: String) {
        prefs.edit().remove(name(service, account)).apply()
    }

    fun list(service: String): List<String> =
        prefs.all.keys.filter { it.startsWith("$service\u0000") }.map { it.substringAfter('\u0000') }

    private companion object {
        const val ALIAS = "mdm-secrets"
        const val TRANSFORM = "AES/GCM/NoPadding"
        const val IV = 12
    }
}

/** The Scheduler's start time: an exact alarm that opens the app, whose Scheduler starts the queue. */
object Alarms {
    private fun intent(context: Context) = PendingIntent.getActivity(
        context, 1,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    /** At `at` (seconds since 1970, UTC), or cancel with null. */
    fun wakeAt(context: Context, at: Long?) {
        val am = context.getSystemService(AlarmManager::class.java)
        val pi = intent(context)
        if (at == null) {
            am.cancel(pi)
            return
        }
        // An alarm clock's alarm may open an activity from the background, and is exact.
        am.setAlarmClock(AlarmManager.AlarmClockInfo(at * 1000, pi), pi)
    }
}

/** A partial wake lock and a Wi-Fi lock while downloads run; multicast for torrents' peers. */
object Locks {
    private var wake: PowerManager.WakeLock? = null
    private var wifi: WifiManager.WifiLock? = null
    private var multicast: WifiManager.MulticastLock? = null

    @SuppressLint("WakelockTimeout")
    @Synchronized
    fun hold(context: Context, reason: String?): Boolean {
        if (reason == null) {
            wake?.takeIf { it.isHeld }?.release()
            wifi?.takeIf { it.isHeld }?.release()
            multicast?.takeIf { it.isHeld }?.release()
            wake = null
            wifi = null
            multicast = null
            return true
        }
        if (wake == null) {
            wake = context.getSystemService(PowerManager::class.java)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "mdm:downloads")
                .apply { setReferenceCounted(false); acquire() }
        }
        val wm = context.getSystemService(WifiManager::class.java)
        if (wifi == null) {
            @Suppress("DEPRECATION")
            val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                WifiManager.WIFI_MODE_FULL_LOW_LATENCY
            } else {
                WifiManager.WIFI_MODE_FULL_HIGH_PERF
            }
            wifi = wm.createWifiLock(mode, "mdm:downloads").apply { setReferenceCounted(false); acquire() }
        }
        if (multicast == null) {
            multicast = wm.createMulticastLock("mdm:torrents").apply { setReferenceCounted(false); acquire() }
        }
        return true
    }
}

/**
 * A page, a file or a folder handed to the app Android picks for it (`Platform.call("open")`):
 * `url` a web page, `view` a file by the app set for its type, `chooser` a file with the
 * system's choice of apps (Open with), `folder` the system's Downloads (Android has no folder
 * view to give a file to). Files go as content: addresses (the manifest's FileProvider).
 */
object Opener {
    /** The intent that views the file at `path` with the app set for its type (its content: address). */
    fun viewIntent(context: Context, path: String): Intent? = try {
        val file = File(path)
        val uri = FileProvider.getUriForFile(context, context.packageName + ".files", file)
        val type = MimeTypeMap.getSingleton()
            .getMimeTypeFromExtension(file.extension.lowercase())
            ?: "application/octet-stream"
        Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, type)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    } catch (e: IllegalArgumentException) {
        // Outside the folders the FileProvider serves.
        null
    }

    fun open(context: Context, kind: String, target: String) {
        val intent = when (kind) {
            "url" -> Intent(Intent.ACTION_VIEW, Uri.parse(target))
            "folder" -> Intent(DownloadManager.ACTION_VIEW_DOWNLOADS)
            "view", "chooser" -> {
                val view = viewIntent(context, target)
                    ?: throw IllegalStateException("no app on this phone opens it")
                if (kind == "chooser") Intent.createChooser(view, null) else view
            }
            // A finished download's file to Android's share sheet.
            "share" -> {
                val view = viewIntent(context, target)
                    ?: throw IllegalStateException("no app on this phone opens it")
                val send = Intent(Intent.ACTION_SEND)
                    .setType(view.type)
                    .putExtra(Intent.EXTRA_STREAM, view.data)
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                Intent.createChooser(send, null)
            }
            else -> throw IllegalArgumentException("unknown kind $kind")
        }
        if (context !is Activity) intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            context.startActivity(intent)
        } catch (e: ActivityNotFoundException) {
            throw IllegalStateException("no app on this phone opens it")
        }
    }
}

