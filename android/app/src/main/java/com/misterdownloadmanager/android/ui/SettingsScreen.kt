package com.misterdownloadmanager.android.ui

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.CallReceived
import androidx.compose.material.icons.automirrored.outlined.PlaylistAdd
import androidx.compose.material.icons.outlined.BatteryAlert
import androidx.compose.material.icons.outlined.Cable
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.Dns
import androidx.compose.material.icons.outlined.VpnLock
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.FolderZip
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.Gavel
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material.icons.outlined.Lan
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material.icons.outlined.Router
import androidx.compose.material.icons.outlined.SmartDisplay
import androidx.compose.material.icons.outlined.Speed
import androidx.compose.material.icons.outlined.SystemUpdate
import androidx.compose.material.icons.outlined.Translate
import androidx.compose.material.icons.outlined.Upload
import androidx.compose.material.icons.outlined.Wifi
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.misterdownloadmanager.android.BuildConfig
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Helpers
import com.misterdownloadmanager.android.core.Language
import com.misterdownloadmanager.android.core.Languages
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.core.Updates
import com.misterdownloadmanager.android.service.NetworkWatch
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/** Settings for the screenshots (`preview`). */
private val PreviewSettings = buildJsonObject {
    put("max_connections", JsonPrimitive(8))
    put("only_add_to_queue", JsonPrimitive(false))
    put("speed_limit_on", JsonPrimitive(false))
    put("speed_limit_kb", JsonPrimitive(2048))
    put("torrent_keep_sharing", JsonPrimitive(true))
    put("torrent_dht", JsonPrimitive(true))
    put("torrent_upnp", JsonPrimitive(true))
    put("torrent_lsd", JsonPrimitive(true))
    put("torrent_incoming", JsonPrimitive(true))
    put("extract_auto", JsonPrimitive(true))
    put("extract_delete", JsonPrimitive(false))
}

/** The Options that matter on a phone, grouped as the desktop's Options pages are. */
@Composable
fun SettingsScreen(onDebrid: () -> Unit = {}, onLicenses: () -> Unit = {}, preview: Boolean = false) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var settings by remember { mutableStateOf(if (preview) PreviewSettings else null) }
    var languages by remember {
        mutableStateOf(if (preview) Languages(listOf(Language("en", "English")), null, "en") else null)
    }
    var helpers by remember { mutableStateOf(if (preview) Helpers(ytdlp = "2026.10.26", ffmpeg = "bundled") else null) }
    var pickLanguage by remember { mutableStateOf(false) }
    var pickFolder by remember { mutableStateOf(false) }
    var folder by remember { mutableStateOf(Mdm.downloadsDir().path) }
    var update by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var askAccess by remember { mutableStateOf(false) }
    var editProxy by remember { mutableStateOf(false) }
    var editDns by remember { mutableStateOf(false) }
    val pickTree = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocumentTree(),
    ) { uri ->
        val dir = uri?.let { treePath(it) }
        if (uri != null && (dir == null || !(dir.isDirectory || dir.mkdirs()) || !dir.canWrite())) {
            error = context.getString(R.string.folder_unusable)
        } else if (dir != null) {
            scope.launch {
                Mdm.setDownloadsDir(dir)
                folder = Mdm.downloadsDir().path
                error = null
            }
        }
    }

    LaunchedEffect(Unit) {
        if (preview) return@LaunchedEffect
        settings = Mdm.settings()
        languages = Mdm.languages()
        helpers = Mdm.helpers()
    }

    fun set(key: String, value: JsonElement) = scope.launch {
        val answer = Mdm.setSettings(buildJsonObject { put(key, value) })
        error = answer["error"]?.jsonPrimitive?.content
        settings = Mdm.settings()
    }

    fun bool(key: String) = settings?.get(key)?.jsonPrimitive?.booleanOrNull ?: false
    fun long(key: String) = settings?.get(key)?.jsonPrimitive?.longOrNull ?: 0

    Column(Modifier.fillMaxSize().background(brand.board)) {
        Board(stringResource(R.string.nav_settings)) { Spacer(Modifier.padding(bottom = 6.dp)) }
        Surface(
            color = MaterialTheme.colorScheme.surface,
            shape = RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            val s = settings
            if (s == null) {
                Text(stringResource(R.string.working), Modifier.padding(24.dp))
                return@Surface
            }
            Column(Modifier.verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(20.dp)) }

                SheetHeading(stringResource(R.string.set_downloads))
                Item(Icons.Outlined.Folder, stringResource(R.string.set_folder), shortPath(context, folder)) { pickFolder = true }
                var connections by remember(s) { mutableFloatStateOf(long("max_connections").coerceIn(1, 32).toFloat()) }
                Item(Icons.Outlined.Cable, stringResource(R.string.set_connections_title), trailing = {
                    Text(connections.toInt().toString(), style = FieldValue)
                })
                Slider(
                    value = connections,
                    onValueChange = { connections = Math.round(it).toFloat() },
                    onValueChangeFinished = { set("max_connections", JsonPrimitive(connections.toInt())) },
                    valueRange = 1f..32f,
                    modifier = Modifier.padding(start = 72.dp, end = 24.dp),
                )
                Toggle(Icons.AutoMirrored.Outlined.PlaylistAdd, stringResource(R.string.set_queue_only), bool("only_add_to_queue")) {
                    set("only_add_to_queue", JsonPrimitive(it))
                }
                var wifiOnly by remember { mutableStateOf(NetworkWatch.wifiOnly(context)) }
                Toggle(Icons.Outlined.Wifi, stringResource(R.string.set_wifi_only), wifiOnly) {
                    wifiOnly = it
                    NetworkWatch.setWifiOnly(context, it)
                }

                SheetHeading(stringResource(R.string.set_speed))
                Toggle(Icons.Outlined.Speed, stringResource(R.string.set_limit_on), bool("speed_limit_on")) {
                    scope.launch {
                        Mdm.speedLimit(it, null)
                        settings = Mdm.settings()
                    }
                }
                if (bool("speed_limit_on")) {
                    var kb by remember(s) { mutableStateOf(long("speed_limit_kb").toString()) }
                    OutlinedTextField(
                        value = kb,
                        onValueChange = { v ->
                            kb = v.filter(Char::isDigit).take(9)
                            kb.toLongOrNull()?.let { scope.launch { Mdm.speedLimit(null, it) } }
                        },
                        label = { Text(stringResource(R.string.set_limit_kb)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth().padding(start = 72.dp, end = 24.dp, bottom = 8.dp),
                    )
                }

                SheetHeading(stringResource(R.string.set_network))
                val proxyMode = long("proxy_mode").toInt()
                Item(
                    Icons.Outlined.VpnLock, stringResource(R.string.set_proxy),
                    when (proxyMode) {
                        3 -> "${str(s, "proxy_host")}:${str(s, "proxy_port")}"
                        2 -> stringResource(R.string.proxy_script)
                        else -> stringResource(R.string.proxy_none)
                    },
                ) { editProxy = true }
                val dnsMode = long("dns_mode").toInt()
                Item(
                    Icons.Outlined.Dns, stringResource(R.string.set_dns),
                    if (dnsMode == 3) {
                        (DNS_PROVIDERS.firstOrNull { it.first == str(s, "dns_provider") }?.second ?: str(s, "dns_doh_url")) + " · " + stringResource(R.string.dns_doh)
                    } else {
                        stringResource(R.string.dns_system)
                    },
                ) { editDns = true }

                SheetHeading(stringResource(R.string.set_video))
                val h = helpers
                Item(
                    Icons.Outlined.SmartDisplay, "yt-dlp",
                    when {
                        h == null -> ""
                        h.installing == "ytdlp" -> stringResource(R.string.working)
                        h.ytdlp != null -> stringResource(R.string.ytdlp_version, h.ytdlp)
                        else -> stringResource(R.string.ytdlp_missing)
                    } + (h?.ytdlpError?.let { "\n$it" } ?: ""),
                ) {
                    if (preview || h?.installing != null) return@Item
                    scope.launch {
                        helpers = h?.copy(installing = "ytdlp")
                        helpers = Mdm.installYtdlp()
                    }
                }
                Item(Icons.Outlined.Movie, "ffmpeg", stringResource(if (h?.ffmpeg != null) R.string.ffmpeg_bundled else R.string.ffmpeg_missing))
                Item(
                    Icons.Outlined.Image, "gallery-dl",
                    when {
                        h == null -> ""
                        h.installing == "gallery" -> stringResource(R.string.working)
                        h.gallery != null -> stringResource(R.string.ytdlp_version, h.gallery)
                        else -> stringResource(R.string.ytdlp_missing)
                    } + (h?.galleryError?.let { "\n$it" } ?: ""),
                ) {
                    if (preview || h?.installing != null) return@Item
                    scope.launch {
                        helpers = h?.copy(installing = "gallery")
                        helpers = Mdm.installGallery()
                    }
                }

                SheetHeading(stringResource(R.string.set_torrents))
                Toggle(Icons.Outlined.Upload, stringResource(R.string.set_keep_sharing), bool("torrent_keep_sharing")) {
                    set("torrent_keep_sharing", JsonPrimitive(it))
                }
                Toggle(Icons.Outlined.Hub, stringResource(R.string.set_dht), bool("torrent_dht")) { set("torrent_dht", JsonPrimitive(it)) }
                Toggle(Icons.Outlined.Router, stringResource(R.string.set_upnp), bool("torrent_upnp")) { set("torrent_upnp", JsonPrimitive(it)) }
                Toggle(Icons.Outlined.Lan, stringResource(R.string.set_lsd), bool("torrent_lsd")) { set("torrent_lsd", JsonPrimitive(it)) }
                Toggle(Icons.AutoMirrored.Outlined.CallReceived, stringResource(R.string.set_incoming), bool("torrent_incoming")) {
                    set("torrent_incoming", JsonPrimitive(it))
                }

                SheetHeading(stringResource(R.string.set_archives))
                Toggle(Icons.Outlined.FolderZip, stringResource(R.string.set_extract), bool("extract_auto")) { set("extract_auto", JsonPrimitive(it)) }
                Toggle(Icons.Outlined.DeleteSweep, stringResource(R.string.set_extract_delete), bool("extract_delete")) {
                    set("extract_delete", JsonPrimitive(it))
                }

                SheetHeading(stringResource(R.string.set_accounts))
                Item(Icons.Outlined.Key, stringResource(R.string.debrid), stringResource(R.string.debrid_item), onClick = onDebrid)

                SheetHeading(stringResource(R.string.set_app))
                val langs = languages
                Item(
                    Icons.Outlined.Translate, stringResource(R.string.set_language),
                    langs?.languages?.firstOrNull { it.code == langs.current }?.name ?: "",
                ) { pickLanguage = true }
                Item(Icons.Outlined.BatteryAlert, stringResource(R.string.set_battery), stringResource(R.string.set_battery_text)) {
                    context.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                }
                Item(Icons.Outlined.SystemUpdate, stringResource(R.string.set_updates), update ?: stringResource(R.string.set_updates_text)) {
                    scope.launch { update = checkForUpdate(context) { update = it } }
                }
                Item(Icons.Outlined.Info, stringResource(R.string.set_about), "Mister Download Manager ${BuildConfig.VERSION_NAME} · GPL-3.0") {
                    context.startActivity(
                        Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/anefzaoui/misterdownloadmanager")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                    )
                }
                Item(Icons.Outlined.Gavel, stringResource(R.string.set_licenses), stringResource(R.string.set_licenses_text), onClick = onLicenses)
            }
        }
    }

    if (pickFolder) {
        val choices = listOf(Mdm.phoneDir()) + Mdm.cardDirs(context)
        AlertDialog(
            onDismissRequest = { pickFolder = false },
            icon = { Icon(Icons.Outlined.Folder, null) },
            title = { Text(stringResource(R.string.set_folder)) },
            text = {
                Column {
                    choices.forEachIndexed { i, dir ->
                        Choice(
                            stringResource(if (i == 0) R.string.folder_phone else R.string.folder_card),
                            shortPath(context, dir.path),
                            folder == dir.path,
                        ) {
                            scope.launch {
                                Mdm.setDownloadsDir(if (i == 0) null else dir)
                                folder = Mdm.downloadsDir().path
                                pickFolder = false
                            }
                        }
                    }
                    if (!preview) {
                        Choice(stringResource(R.string.folder_other), stringResource(R.string.folder_other_text), choices.none { it.path == folder }) {
                            pickFolder = false
                            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && !android.os.Environment.isExternalStorageManager()) {
                                askAccess = true
                            } else {
                                pickTree.launch(null)
                            }
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { pickFolder = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }

    if (editProxy) {
        ProxyDialog(settings ?: JsonObject(emptyMap()), onDismiss = { editProxy = false }) { patch ->
            editProxy = false
            scope.launch {
                val answer = Mdm.setSettings(patch)
                error = answer["error"]?.jsonPrimitive?.content
                settings = Mdm.settings()
            }
        }
    }
    if (editDns) {
        DnsDialog(settings ?: JsonObject(emptyMap()), onDismiss = { editDns = false }) { patch ->
            editDns = false
            scope.launch {
                val answer = Mdm.setSettings(patch)
                error = answer["error"]?.jsonPrimitive?.content
                settings = Mdm.settings()
            }
        }
    }

    if (askAccess) {
        AlertDialog(
            onDismissRequest = { askAccess = false },
            icon = { Icon(Icons.Outlined.Folder, null) },
            title = { Text(stringResource(R.string.folder_access)) },
            text = { Text(stringResource(R.string.folder_access_text)) },
            confirmButton = {
                TextButton(onClick = {
                    askAccess = false
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                        context.startActivity(
                            Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:${context.packageName}"))
                                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                        )
                    }
                }) { Text(stringResource(android.R.string.ok)) }
            },
            dismissButton = { TextButton(onClick = { askAccess = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }

    if (pickLanguage) {
        val langs = languages
        AlertDialog(
            onDismissRequest = { pickLanguage = false },
            icon = { Icon(Icons.Outlined.Translate, null) },
            title = { Text(stringResource(R.string.set_language)) },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    Choice(stringResource(R.string.language_system), null, langs?.chosen == null) {
                        scope.launch { Mdm.setLanguage(null); appLocale(context, null); languages = Mdm.languages(); pickLanguage = false }
                    }
                    langs?.languages?.forEach { l ->
                        Choice(l.name, null, langs.chosen == l.code) {
                            scope.launch { Mdm.setLanguage(l.code); appLocale(context, l.code); languages = Mdm.languages(); pickLanguage = false }
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { pickLanguage = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/**
 * The folder behind a picked tree (`primary:Download/Films` → the phone's storage, a card's
 * `1234-ABCD:Films` → /storage/1234-ABCD), which MDM writes with plain paths once it may
 * manage all files.
 */
private fun treePath(uri: Uri): java.io.File? {
    val id = runCatching { android.provider.DocumentsContract.getTreeDocumentId(uri) }.getOrNull() ?: return null
    val volume = id.substringBefore(':')
    val rest = id.substringAfter(':', "")
    val root = if (volume == "primary") android.os.Environment.getExternalStorageDirectory() else java.io.File("/storage/$volume")
    return if (rest.isEmpty()) root else java.io.File(root, rest)
}

/** A folder as people know it: "Download/MisterDownloadManager", or the card's own folder. */
private fun shortPath(context: Context, path: String): String {
    val root = android.os.Environment.getExternalStorageDirectory().path
    return when {
        path.startsWith("$root/") -> path.removePrefix("$root/")
        path.contains("/Android/data/") -> context.getString(R.string.folder_card_short) + " · " + path.substringAfter("/Android/data/").substringAfter("/files/")
        else -> path
    }
}

private fun str(s: JsonObject?, key: String) = s?.get(key)?.jsonPrimitive?.content ?: ""

/** DNS over HTTPS services MDM knows (mdm_engine::dns::PRESETS' ids). */
private val DNS_PROVIDERS = listOf(
    "cloudflare" to "Cloudflare", "google" to "Google", "quad9" to "Quad9", "adguard" to "AdGuard", "nextdns" to "NextDNS",
)

/** Options → Proxy / Socks: none, an automatic configuration script, or a server. */
@Composable
private fun ProxyDialog(s: JsonObject, onDismiss: () -> Unit, onSave: (JsonObject) -> Unit) {
    var mode by remember { mutableStateOf(s["proxy_mode"]?.jsonPrimitive?.longOrNull?.toInt()?.takeIf { it in setOf(0, 2, 3) } ?: 0) }
    var host by remember { mutableStateOf(str(s, "proxy_host")) }
    var port by remember { mutableStateOf(str(s, "proxy_port")) }
    var user by remember { mutableStateOf(str(s, "proxy_user")) }
    var pass by remember { mutableStateOf("") }
    var script by remember { mutableStateOf(str(s, "proxy_script")) }
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.VpnLock, null) },
        title = { Text(stringResource(R.string.set_proxy)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Choice(stringResource(R.string.proxy_none), null, mode == 0) { mode = 0 }
                Choice(stringResource(R.string.proxy_manual), stringResource(R.string.proxy_manual_text), mode == 3) { mode = 3 }
                Choice(stringResource(R.string.proxy_script), null, mode == 2) { mode = 2 }
                if (mode == 3) {
                    OutlinedTextField(host, { host = it.trim() }, label = { Text(stringResource(R.string.proxy_host)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
                    OutlinedTextField(
                        port, { port = it.filter(Char::isDigit).take(5) }, label = { Text(stringResource(R.string.proxy_port)) }, singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth(),
                    )
                    OutlinedTextField(user, { user = it }, label = { Text(stringResource(R.string.proxy_user)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
                    OutlinedTextField(
                        pass, { pass = it }, label = { Text(stringResource(R.string.proxy_pass)) }, singleLine = true,
                        visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(), modifier = Modifier.fillMaxWidth(),
                    )
                }
                if (mode == 2) {
                    OutlinedTextField(script, { script = it.trim() }, label = { Text(stringResource(R.string.proxy_script_url)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = mode != 3 || (host.isNotBlank() && port.isNotBlank()),
                onClick = {
                    onSave(buildJsonObject {
                        put("proxy_mode", JsonPrimitive(mode))
                        if (mode == 3) {
                            put("proxy_host", JsonPrimitive(host))
                            put("proxy_port", JsonPrimitive(port))
                            put("proxy_user", JsonPrimitive(user))
                            if (pass.isNotEmpty()) put("proxy_pass", JsonPrimitive(pass))
                            put("proxy_http", JsonPrimitive(true))
                            put("proxy_https", JsonPrimitive(true))
                            put("proxy_ftp", JsonPrimitive(true))
                        }
                        if (mode == 2) put("proxy_script", JsonPrimitive(script))
                    })
                },
            ) { Text(stringResource(android.R.string.ok)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** Options → Connection → DNS: the system's, or a DNS over HTTPS service (or address). */
@Composable
private fun DnsDialog(s: JsonObject, onDismiss: () -> Unit, onSave: (JsonObject) -> Unit) {
    val mode0 = s["dns_mode"]?.jsonPrimitive?.longOrNull?.toInt() ?: 0
    var provider by remember { mutableStateOf(if (mode0 == 3) str(s, "dns_provider").ifEmpty { "cloudflare" } else "") }
    var url by remember { mutableStateOf(str(s, "dns_doh_url")) }
    var nextId by remember { mutableStateOf(str(s, "dns_nextdns_id")) }
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.Dns, null) },
        title = { Text(stringResource(R.string.set_dns)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Choice(stringResource(R.string.dns_system), stringResource(R.string.dns_system_text), provider.isEmpty()) { provider = "" }
                DNS_PROVIDERS.forEach { (id, name) ->
                    Choice(name, stringResource(R.string.dns_doh), provider == id) { provider = id }
                }
                Choice(stringResource(R.string.dns_custom), stringResource(R.string.dns_doh), provider == "custom") { provider = "custom" }
                if (provider == "nextdns") {
                    OutlinedTextField(nextId, { nextId = it.trim() }, label = { Text(stringResource(R.string.dns_nextdns_id)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
                }
                if (provider == "custom") {
                    OutlinedTextField(url, { url = it.trim() }, label = { Text("https://…/dns-query") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = provider != "custom" || url.startsWith("https://"),
                onClick = {
                    onSave(buildJsonObject {
                        put("dns_mode", JsonPrimitive(if (provider.isEmpty()) 0 else 3))
                        if (provider.isNotEmpty()) put("dns_provider", JsonPrimitive(provider))
                        if (provider == "custom") put("dns_doh_url", JsonPrimitive(url))
                        if (provider == "nextdns") put("dns_nextdns_id", JsonPrimitive(nextId))
                        put("dns_fallback", JsonPrimitive(true))
                    })
                },
            ) { Text(stringResource(android.R.string.ok)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** Check the fork's releases; install the newer one when there is. Returns what to say. */
private suspend fun checkForUpdate(context: Context, progress: (String) -> Unit): String {
    progress(context.getString(R.string.working))
    return try {
        val u = Updates.check() ?: return context.getString(R.string.update_none)
        if (u.apk == null) {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(u.page)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            return context.getString(R.string.update_available, u.version)
        }
        val failed = Updates.install(context, u) { f -> progress(context.getString(R.string.update_downloading, u.version, (f * 100).toInt())) }
        failed?.let { context.getString(R.string.update_failed, it) } ?: context.getString(R.string.update_available, u.version)
    } catch (e: Exception) {
        context.getString(R.string.update_failed, e.message ?: e.toString())
    }
}

/**
 * The app's own texts (Android's resources) in the language chosen for the core's, through
 * Android 13's per-app language; earlier versions follow the system's.
 */
private fun appLocale(context: Context, code: String?) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
    val tag = code?.split('-')?.mapIndexed { i, p ->
        when {
            i == 0 -> p
            p.length == 4 -> p.replaceFirstChar(Char::titlecase)
            else -> p.uppercase()
        }
    }?.joinToString("-")
    context.getSystemService(android.app.LocaleManager::class.java)?.applicationLocales =
        if (tag == null) android.os.LocaleList.getEmptyLocaleList() else android.os.LocaleList.forLanguageTags(tag)
}

@Composable
private fun Choice(name: String, detail: String?, chosen: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable(onClick = onClick), verticalAlignment = Alignment.CenterVertically) {
        RadioButton(selected = chosen, onClick = onClick)
        Column(Modifier.weight(1f)) {
            Text(name, style = MaterialTheme.typography.bodyLarge)
            if (detail != null) Text(detail, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun Item(
    icon: ImageVector,
    title: String,
    value: String? = null,
    trailing: (@Composable () -> Unit)? = null,
    onClick: (() -> Unit)? = null,
) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = value?.takeIf { it.isNotEmpty() }?.let { { Text(it) } },
        leadingContent = { Icon(icon, null, tint = MaterialTheme.colorScheme.primary) },
        trailingContent = trailing,
        colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surface),
        modifier = if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier,
    )
}

@Composable
private fun Toggle(icon: ImageVector, title: String, on: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        leadingContent = { Icon(icon, null, tint = MaterialTheme.colorScheme.primary) },
        trailingContent = { Switch(checked = on, onCheckedChange = onChange) },
        colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surface),
        modifier = Modifier.clickable { onChange(!on) },
    )
}
