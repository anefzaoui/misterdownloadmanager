package com.misterdownloadmanager.android.core

import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.content.FileProvider
import com.misterdownloadmanager.android.BuildConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/** A newer Android release on GitHub (`android-v*` tags of [BuildConfig.UPDATE_REPO]). */
data class Update(
    val version: String,
    val page: String,
    /** The APK for this phone's processor (else the universal one), and the release's checksums. */
    val apk: String?,
    val apkName: String?,
    val sums: String?,
)

object Updates {
    private fun get(url: String): HttpURLConnection =
        (URL(url).openConnection() as HttpURLConnection).apply {
            setRequestProperty("Accept", "application/vnd.github+json")
            connectTimeout = 15_000
            readTimeout = 30_000
            instanceFollowRedirects = true
        }

    /** The newest release newer than this copy, or null; throws when GitHub can't be asked. */
    suspend fun check(): Update? = withContext(Dispatchers.IO) {
        val text = get("https://api.github.com/repos/${BuildConfig.UPDATE_REPO}/releases?per_page=20")
            .inputStream.use { it.bufferedReader().readText() }
        CoreJson.parseToJsonElement(text).jsonArray
            .map { it.jsonObject }
            .mapNotNull(::release)
            .filter { newer(it.version, BuildConfig.VERSION_NAME) }
            .maxWithOrNull { a, b -> if (newer(a.version, b.version)) 1 else if (newer(b.version, a.version)) -1 else 0 }
    }

    private fun release(r: JsonObject): Update? {
        val tag = r["tag_name"]?.jsonPrimitive?.content ?: return null
        if (!tag.startsWith("android-v")) return null
        val assets = r["assets"]?.jsonArray?.map { it.jsonObject }.orEmpty()
        fun asset(name: String) = assets.firstOrNull { it["name"]?.jsonPrimitive?.content == name }
        val abi = Build.SUPPORTED_ABIS.firstOrNull { a -> asset("app-$a-release.apk") != null }
        val apk = asset("app-$abi-release.apk") ?: asset("app-universal-release.apk")
        return Update(
            version = tag.removePrefix("android-v"),
            page = r["html_url"]?.jsonPrimitive?.content ?: return null,
            apk = apk?.get("browser_download_url")?.jsonPrimitive?.content,
            apkName = apk?.get("name")?.jsonPrimitive?.content,
            sums = asset("SHA256SUMS.txt")?.get("browser_download_url")?.jsonPrimitive?.content,
        )
    }

    /**
     * Download the update's APK, check it against the release's SHA256SUMS.txt, and hand it to
     * Android's installer (which asks the person, and checks it is signed as this copy is).
     * Returns null once the installer is open, else why not.
     */
    suspend fun install(context: Context, u: Update, progress: (Float) -> Unit): String? = withContext(Dispatchers.IO) {
        val url = u.apk ?: return@withContext "no APK for this phone in the release"
        val name = u.apkName ?: return@withContext "no APK name"
        val sumsUrl = u.sums ?: return@withContext "the release has no checksums"
        val sums = get(sumsUrl).inputStream.use { it.bufferedReader().readText() }
        val expected = sums.lines().firstNotNullOfOrNull { line ->
            val parts = line.trim().split(Regex("\\s+"), limit = 2)
            if (parts.size == 2 && parts[1].trimStart('*') == name) parts[0].lowercase() else null
        } ?: return@withContext "no checksum for $name"
        val dir = File(context.cacheDir, "updates").apply { deleteRecursively(); mkdirs() }
        val file = File(dir, name)
        val digest = MessageDigest.getInstance("SHA-256")
        val conn = get(url)
        val total = conn.contentLengthLong.takeIf { it > 0 }
        conn.inputStream.use { input ->
            file.outputStream().use { out ->
                val buf = ByteArray(64 * 1024)
                var done = 0L
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    out.write(buf, 0, n)
                    digest.update(buf, 0, n)
                    done += n
                    if (total != null) progress(done.toFloat() / total)
                }
            }
        }
        val got = digest.digest().joinToString("") { "%02x".format(it) }
        if (got != expected) {
            file.delete()
            return@withContext "the download doesn't match the release's checksum"
        }
        val uri = FileProvider.getUriForFile(context, context.packageName + ".files", file)
        val intent = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        withContext(Dispatchers.Main) { context.startActivity(intent) }
        null
    }

    /** Versions compare by their numbers in order: 0.5.10 is newer than 0.5.9. */
    fun newer(a: String, b: String): Boolean {
        val pa = a.split('.', '-').map { it.toIntOrNull() ?: 0 }
        val pb = b.split('.', '-').map { it.toIntOrNull() ?: 0 }
        for (i in 0 until maxOf(pa.size, pb.size)) {
            val x = pa.getOrElse(i) { 0 }
            val y = pb.getOrElse(i) { 0 }
            if (x != y) return x > y
        }
        return false
    }
}
