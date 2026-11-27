package com.misterdownloadmanager.android

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.util.Log
import java.io.File
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit

/**
 * Android's pickers for the app's "..." buttons (`Platform.call("pick")`, crates/filedialog):
 * a folder (its tree turned into a path on the shared storage, which MDM writes to as it does
 * to Download) or a file to open (copied into the cache, as MDM can't read another app's file
 * by its path). Called from a thread of the app's own, which waits here for the person's choice.
 */
object Picker {
    private const val REQUEST = 7301

    /** The answer the waiting thread takes: the picked address, or "" when cancelled. */
    @Volatile
    private var answer: ArrayBlockingQueue<String>? = null

    fun pick(activity: Activity, kind: String): String? {
        val queue = ArrayBlockingQueue<String>(1)
        answer = queue
        activity.runOnUiThread {
            val intent = if (kind == "folder") {
                Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
            } else {
                Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
            }
            try {
                @Suppress("DEPRECATION")
                activity.startActivityForResult(intent, REQUEST)
            } catch (e: ActivityNotFoundException) {
                queue.offer("")
            }
        }
        // The person may take their time; an activity gone meanwhile never answers.
        val picked = queue.poll(30, TimeUnit.MINUTES).orEmpty()
        answer = null
        if (picked.isEmpty()) return null
        val uri = Uri.parse(picked)
        return if (kind == "folder") folderPath(uri) else copied(activity, uri)
    }

    /** The activity's result: true when it was the picker's. */
    fun result(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST) return false
        val picked = if (resultCode == Activity.RESULT_OK) data?.dataString.orEmpty() else ""
        answer?.offer(picked)
        return true
    }

    /** A folder picked on the phone's storage (or a card): its path; null for another provider's. */
    private fun folderPath(tree: Uri): String? {
        if (tree.authority != "com.android.externalstorage.documents") return null
        val id = DocumentsContract.getTreeDocumentId(tree)
        val volume = id.substringBefore(':')
        val inside = id.substringAfter(':', "")
        val root = if (volume.equals("primary", ignoreCase = true)) {
            @Suppress("DEPRECATION")
            Environment.getExternalStorageDirectory()
        } else {
            File("/storage", volume)
        }
        return File(root, inside).path
    }

    /** A file picked to open, copied into the cache under its own name (16 MB at most). */
    private fun copied(activity: Activity, uri: Uri): String? = try {
        val name = activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { if (it.moveToFirst()) it.getString(0) else null }
            ?.replace('/', '_') ?: "picked"
        val dir = File(activity.cacheDir, "picked").apply { mkdirs() }
        val file = File(dir, name)
        activity.contentResolver.openInputStream(uri)?.use { input ->
            file.outputStream().use { output -> input.copyTo(output) }
        }
        if (file.length() in 1..(16L shl 20)) file.path else null
    } catch (e: Exception) {
        Log.w("mdm", "picked file", e)
        null
    }
}
