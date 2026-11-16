package com.misterdownloadmanager.android.ui

import android.Manifest
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.mutableStateOf
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.misterdownloadmanager.android.service.CrashReports
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.service.DownloadService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    /** Links handed over (shared, opened with MDM) waiting for the Add sheet. */
    private val incoming = mutableStateOf<List<String>?>(null)

    /** A download to show (from a notification). */
    private val showDownload = mutableStateOf<Long?>(null)

    /** A .torrent file opened with MDM, waiting for its files to be chosen. */
    private val incomingTorrent = mutableStateOf<ByteArray?>(null)

    private val askPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        askPermissions()
        handle(intent)
        setContent {
            MdmTheme {
                App(
                    incoming = incoming.value,
                    onIncomingTaken = { incoming.value = null },
                    showDownload = showDownload.value,
                    onShown = { showDownload.value = null },
                    clipboardLink = ::clipboardLink,
                    incomingTorrent = incomingTorrent.value,
                    onTorrentTaken = { incomingTorrent.value = null },
                )
            }
        }
        // Keep the service up while something runs (downloads started from the list, a queue).
        // Only while in front: Android refuses a foreground service started from the background.
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                Mdm.snapshot.collect { s ->
                    if (s != null && s.running > 0) DownloadService.start(this@MainActivity)
                }
            }
        }
        CrashReports.check(this)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    private fun askPermissions() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            askPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        if (Build.VERSION.SDK_INT <= Build.VERSION_CODES.P &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED
        ) {
            askPermission.launch(Manifest.permission.WRITE_EXTERNAL_STORAGE)
        }
    }

    private fun handle(intent: Intent?) {
        intent ?: return
        intent.getLongExtra(EXTRA_DOWNLOAD, -1).takeIf { it >= 0 }?.let { showDownload.value = it }
        when (intent.action) {
            Intent.ACTION_SEND, Intent.ACTION_SEND_MULTIPLE -> {
                val text = buildList {
                    intent.getStringExtra(Intent.EXTRA_TEXT)?.let(::add)
                    intent.getStringArrayListExtra(Intent.EXTRA_TEXT)?.let(::addAll)
                }.joinToString("\n")
                val links = Links.find(text)
                if (links.isEmpty()) {
                    Toast.makeText(this, R.string.no_links, Toast.LENGTH_LONG).show()
                } else {
                    incoming.value = links
                }
            }
            Intent.ACTION_VIEW -> {
                val uri = intent.data ?: return
                when (uri.scheme) {
                    "content", "file" -> addTorrentFile(uri)
                    else -> incoming.value = listOf(uri.toString())
                }
            }
        }
        // Handled once: a configuration change mustn't hand it over again.
        setIntent(Intent(this, MainActivity::class.java))
    }

    private fun addTorrentFile(uri: Uri) {
        lifecycleScope.launch {
            val bytes = withContext(Dispatchers.IO) {
                runCatching { contentResolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull()
            }
            if (bytes == null || bytes.isEmpty() || bytes.size > 32 shl 20) {
                Toast.makeText(this@MainActivity, R.string.torrent_unreadable, Toast.LENGTH_LONG).show()
                return@launch
            }
            withContext(Dispatchers.IO) { Mdm.start(this@MainActivity) }
            incomingTorrent.value = bytes
        }
    }

    /** A link on the clipboard, for the Add sheet (only the app in front may read it). */
    private fun clipboardLink(): String? {
        val cm = getSystemService(ClipboardManager::class.java) ?: return null
        val clip = cm.primaryClip ?: return null
        if (clip.itemCount == 0) return null
        val text = clip.getItemAt(0).coerceToText(this)?.toString() ?: return null
        return Links.find(text).firstOrNull()
    }

    companion object {
        const val EXTRA_DOWNLOAD = "download"
    }
}

/** Links in a text: http(s), ftp(s) and magnet. */
object Links {
    private val pattern = Regex("""(?i)\b(?:https?|ftps?)://[^\s<>"']+|magnet:\?[^\s<>"']+""")

    fun find(text: String): List<String> =
        pattern.findAll(text).map { it.value.trimEnd('.', ',', ')', ']', ';', '!', '?') }.distinct().toList()
}
