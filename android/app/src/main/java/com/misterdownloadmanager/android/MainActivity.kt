package com.misterdownloadmanager.android

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.view.View
import androidx.activity.OnBackPressedCallback
import androidx.core.content.IntentCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.google.androidgamesdk.GameActivity
import java.io.File

/**
 * The app's one activity: GameActivity loads libmdm.so (the manifest's `android.app.lib_name`)
 * and runs its `android_main` on a thread of its own, where MDM's interface draws (crates/android).
 */
class MainActivity : GameActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // The keystore, alarms, locks and the downloads' service, for the app (libmdm.so).
        Platform.init(this)
        nativeHost(Platform)
        // Started for a link (Share, or one opened with MDM): it goes to the app.
        links(intent)
        // Android 13 and later show the downloads' notification only with this permission;
        // Android 10 and older write to the Download folder only with storage's.
        val wanted = when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU -> Manifest.permission.POST_NOTIFICATIONS
            Build.VERSION.SDK_INT <= Build.VERSION_CODES.Q -> Manifest.permission.WRITE_EXTERNAL_STORAGE
            else -> null
        }
        if (wanted != null && checkSelfPermission(wanted) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(wanted), 1)
        }
        // The interface draws inside the status and navigation bars, the display's cutout and the
        // keyboard, not under them: the content is padded by them, and the surface MDM draws on
        // with it.
        val content = findViewById<View>(android.R.id.content)
        ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
            val bars = insets.getInsets(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout(),
            )
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, ime.bottom))
            WindowInsetsCompat.CONSUMED
        }
        // Back (the gesture, the navigation bar's button) closes the window in front, as Esc
        // does on the desktop; with nothing in front, the app goes to the background (Platform's
        // toBack, as the app decides).
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = nativeBack()
        })
    }

    /** The soft keyboard's action key (Done, Go): Enter for the app's text box. */
    override fun onEditorAction(action: Int) {
        nativeEditorAction()
        super.onEditorAction(action)
    }

    /** A picker's answer (the "..." buttons, [Picker]). */
    @Deprecated("The picker's result, as startActivityForResult gives it")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (!Picker.result(requestCode, resultCode, data)) {
            @Suppress("DEPRECATION")
            super.onActivityResult(requestCode, resultCode, data)
        }
    }

    /** A link handed over while the app runs (singleTask: the activity on screen gets it). */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        links(intent)
    }

    /**
     * The text of a Share, or the address of a link opened with MDM, to the app's Add URL; a
     * `.torrent` file opened or shared (a content: address), copied into the cache, to its Add
     * torrent.
     */
    private fun links(intent: Intent?) {
        val file = when (intent?.action) {
            Intent.ACTION_VIEW -> intent.data?.takeIf { it.scheme == "content" || it.scheme == "file" }
            Intent.ACTION_SEND -> IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java)
            else -> null
        }
        if (file != null) {
            torrent(file)
            return
        }
        val text = when (intent?.action) {
            Intent.ACTION_SEND -> intent.getStringExtra(Intent.EXTRA_TEXT)
            Intent.ACTION_VIEW -> intent.dataString
            else -> null
        }
        if (!text.isNullOrBlank()) nativeLinks(text)
    }

    /** A shared `.torrent` (16 MB at most) copied into the cache, its path to the app. */
    private fun torrent(uri: Uri) {
        try {
            val dir = File(cacheDir, "shared").apply { mkdirs() }
            val file = File(dir, "shared-${System.currentTimeMillis()}.torrent")
            contentResolver.openInputStream(uri)?.use { input ->
                file.outputStream().use { output -> input.copyTo(output) }
            }
            if (file.length() in 1..(16L shl 20)) nativeFile(file.absolutePath) else file.delete()
        } catch (e: Exception) {
            Log.w("mdm", "shared torrent", e)
        }
    }

    override fun onDestroy() {
        // GameActivity's destruction waits for the app's thread to end: the app is told first,
        // and closes (saving the list, as Exit does).
        nativeEnding()
        super.onDestroy()
    }

    private external fun nativeHost(platform: Any)

    private external fun nativeBack()

    private external fun nativeEnding()

    private external fun nativeEditorAction()

    private external fun nativeLinks(text: String)

    private external fun nativeFile(path: String)
}
