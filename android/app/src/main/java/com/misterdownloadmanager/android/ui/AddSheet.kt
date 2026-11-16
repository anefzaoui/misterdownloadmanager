package com.misterdownloadmanager.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.core.Snapshot
import com.misterdownloadmanager.android.core.PlaylistListing
import com.misterdownloadmanager.android.core.TorrentLookup
import com.misterdownloadmanager.android.core.VideoLookup
import com.misterdownloadmanager.android.service.DownloadService
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** What the sheet shows. */
private sealed interface Step {
    data object Links : Step
    data object Working : Step
    data class LookingUp(val address: String) : Step
    data class Video(val address: String, val lookup: VideoLookup) : Step
    data class Problem(val text: String, val address: String?) : Step
    data class NeedYtdlp(val address: String) : Step
    data class NeedGalleryDl(val address: String) : Step
    data class Torrent(val token: String, val lookup: TorrentLookup) : Step
    data class Playlist(val token: String, val listing: PlaylistListing) : Step
}

/** The clipboard's first link, if any (only the app in front may read it). */
private fun clipboardText(context: android.content.Context): String? {
    val cm = context.getSystemService(android.content.ClipboardManager::class.java) ?: return null
    val clip = runCatching { cm.primaryClip }.getOrNull() ?: return null
    if (clip.itemCount == 0) return null
    return Links.find(clip.getItemAt(0).coerceToText(context)?.toString() ?: return null).firstOrNull()
}

/** Add downloads: links typed, pasted or shared; a video page's quality chosen first. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddSheet(initial: List<String>, snapshot: Snapshot?, onDismiss: () -> Unit, torrentData: ByteArray? = null) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var text by remember { mutableStateOf(initial.joinToString("\n")) }
    var later by remember { mutableStateOf(false) }
    var category by remember { mutableStateOf<String?>(null) }
    var step by remember { mutableStateOf<Step>(if (torrentData != null) Step.Working else Step.Links) }
    var choice by remember { mutableIntStateOf(0) }
    val chosen = remember { mutableStateListOf<Long>() }
    var inOrder by remember { mutableStateOf(false) }
    val lookupFailed = stringResource(R.string.video_lookup_failed)
    val isList = stringResource(R.string.video_is_list)
    var lookup by remember { mutableStateOf<kotlinx.coroutines.Job?>(null) }

    fun close() = scope.launch { sheet.hide() }.invokeOnCompletion { onDismiss() }

    /** Plain downloads: files, magnet links, torrents' addresses. */
    fun addPlain(links: List<String>) = scope.launch {
        step = Step.Working
        val answer = Mdm.add(links, later = later, category = category)
        if (!later) DownloadService.start(context)
        if (answer.errors.isNotEmpty() && answer.ids.isEmpty() && answer.pending == 0) {
            step = Step.Problem(answer.errors.joinToString("\n"), null)
        } else {
            close()
        }
    }

    /** A playlist or a channel: its videos listed (they come in as yt-dlp finds them). */
    fun playlist(address: String) = scope.launch {
        val token = Mdm.playlistStart(address)
        if (token == null) {
            step = Step.Problem(isList, address)
            return@launch
        }
        chosen.clear()
        var known = 0
        while (true) {
            val l = Mdm.playlistPoll(token)
            if (l.state == "gone") break
            l.entries.drop(known).filter { !it.unavailable }.forEach { chosen.add(it.index) }
            known = l.entries.size
            if (step is Step.Working || step is Step.Playlist) step = Step.Playlist(token, l)
            if (l.state == "done") break
            delay(1000)
        }
    }

    /** An image gallery's page: every picture, through gallery-dl (installed first when needed). */
    fun gallery(address: String) = scope.launch {
        step = Step.Working
        if (Mdm.helpers().gallery == null) {
            step = Step.NeedGalleryDl(address)
            return@launch
        }
        val r = Mdm.addGallery(address, later)
        if (r.containsKey("error")) {
            step = Step.Problem(r["error"].toString(), address)
        } else {
            if (!later) DownloadService.start(context)
            close()
        }
    }

    /** A video page: look its qualities up with yt-dlp, asking until they're known. */
    suspend fun lookUpNow(address: String) {
        step = Step.LookingUp(address)
        var l = Mdm.videoLookup(address)
        while (l.state == "pending" && l.token != null) {
            delay(800)
            l = Mdm.videoPoll(l.token)
        }
        step = when (l.state) {
            "ready" -> {
                choice = 0
                Step.Video(address, l)
            }
            "list" -> {
                playlist(address)
                return
            }
            "failed" if l.reason == "no-ytdlp" -> Step.NeedYtdlp(address)
            else -> Step.Problem(l.message ?: l.error ?: "$lookupFailed (${l.reason ?: l.state})", address)
        }
    }

    /** A video page: look its qualities up with yt-dlp, asking until they're known. */
    fun lookUp(address: String) {
        lookup?.cancel()
        lookup = scope.launch { lookUpNow(address) }
    }

    val torrentFailed = stringResource(R.string.torrent_unreadable)

    /** A torrent: its files looked up (a magnet link asks the swarm), then chosen. */
    fun torrent(magnet: String?, data: ByteArray?) = scope.launch {
        step = Step.Working
        val token = Mdm.torrentLookup(magnet, data)
        if (token == null) {
            step = Step.Problem(torrentFailed, null)
            return@launch
        }
        var t = Mdm.torrentPoll(token)
        while (t.state == "pending") {
            delay(700)
            t = Mdm.torrentPoll(token)
        }
        if (t.state == "ready") {
            chosen.clear()
            chosen.addAll(t.files.map { it.index })
            step = Step.Torrent(token, t)
        } else {
            step = Step.Problem(t.message ?: torrentFailed, null)
        }
    }

    LaunchedEffect(torrentData) {
        if (torrentData != null) torrent(null, torrentData)
    }

    fun go() = scope.launch {
        val links = Links.find(text)
        if (links.isEmpty()) {
            step = Step.Problem(context.getString(R.string.no_links), null)
            return@launch
        }
        val classified = if (links.size == 1) Mdm.classify(links[0]) else Mdm.Kind("many")
        val kind = classified.kind
        when {
            kind == "protected" -> step = Step.Problem(classified.message ?: lookupFailed, null)
            kind == "video" -> lookUp(links[0])
            kind == "gallery" -> gallery(links[0])
            kind == "torrent" && links[0].startsWith("magnet:", ignoreCase = true) -> torrent(links[0], null)
            else -> addPlain(links)
        }
    }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = MaterialTheme.colorScheme.surface) {
        Column(
            Modifier.padding(horizontal = 24.dp).padding(bottom = 24.dp).navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            when (val s = step) {
                Step.Links -> LinksStep(
                    text = text,
                    onText = { text = it },
                    onPaste = clipboardText(context)?.let { clip -> { text = (text.lines().filter { it.isNotBlank() } + clip).distinct().joinToString("\n") } },
                    categories = snapshot?.categories.orEmpty(),
                    category = category,
                    onCategory = { category = it },
                    later = later,
                    onLater = { later = it },
                    onCancel = { close() },
                    onGo = { go() },
                )
                Step.Working -> WorkingStep(stringResource(R.string.working))
                // Cancel stops asking; the look-up ends by itself in the core.
                is Step.LookingUp -> LookingUpStep(s.address) {
                    lookup?.cancel()
                    step = Step.Links
                }
                is Step.Video -> VideoStep(
                    lookup = s.lookup,
                    address = s.address,
                    choice = choice,
                    onChoice = { choice = it },
                    onCancel = { close() },
                    onDownload = {
                        scope.launch {
                            step = Step.Working
                            val r = Mdm.videoPick(s.lookup.token ?: "", choice)
                            if (r.state in setOf("started", "queued", "sent")) {
                                DownloadService.start(context)
                                close()
                            } else {
                                step = Step.Problem(r.message ?: r.error ?: lookupFailed, s.address)
                            }
                        }
                    },
                )
                is Step.Playlist -> {
                    val l = s.listing
                    SheetTitle(l.title ?: stringResource(R.string.playlist))
                    Text(
                        if (l.state == "done") {
                            stringResource(R.string.playlist_count, chosen.size, l.entries.size)
                        } else {
                            stringResource(R.string.playlist_listing, l.entries.size)
                        },
                        style = MaterialTheme.typography.bodySmall,
                    )
                    l.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                    Row {
                        TextButton(onClick = { chosen.clear(); chosen.addAll(l.entries.filter { !it.unavailable }.map { it.index }) }) {
                            Text(stringResource(R.string.select_all))
                        }
                        TextButton(onClick = { chosen.clear() }) { Text(stringResource(R.string.select_none)) }
                    }
                    LazyColumn(Modifier.heightIn(max = 260.dp)) {
                        itemsIndexed(l.entries, key = { _, e -> e.index }) { _, e ->
                            Row(
                                Modifier.fillMaxWidth()
                                    .clickable(enabled = !e.unavailable, role = Role.Checkbox) {
                                        if (e.index in chosen) chosen.remove(e.index) else chosen.add(e.index)
                                    }
                                    .padding(vertical = 2.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Checkbox(checked = e.index in chosen, onCheckedChange = null, enabled = !e.unavailable)
                                Text("${e.index}. ${e.title}", Modifier.weight(1f).padding(start = 8.dp), maxLines = 2, overflow = TextOverflow.Ellipsis)
                                e.duration?.let { Text(Format.eta(it.toLong()), style = MaterialTheme.typography.bodySmall) }
                            }
                        }
                    }
                    var pick by remember { mutableIntStateOf(0) }
                    var open by remember { mutableStateOf(false) }
                    ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }) {
                        OutlinedTextField(
                            value = l.picks.getOrElse(pick) { "" },
                            onValueChange = {},
                            readOnly = true,
                            label = { Text(stringResource(R.string.playlist_quality)) },
                            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(open) },
                            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable),
                        )
                        ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                            l.picks.forEachIndexed { i, p -> DropdownMenuItem(text = { Text(p) }, onClick = { pick = i; open = false }) }
                        }
                    }
                    SheetActions(
                        stringResource(if (later) R.string.add_queue else R.string.add_start),
                        enabled = chosen.isNotEmpty(),
                        onCancel = { close() },
                        onAction = {
                            scope.launch {
                                val ids = chosen.toList()
                                step = Step.Working
                                val r = Mdm.playlistAdd(s.token, ids, pick, later)
                                if (r.containsKey("error")) {
                                    step = Step.Problem(r["error"].toString(), null)
                                } else {
                                    if (!later) DownloadService.start(context)
                                    close()
                                }
                            }
                        },
                    )
                }
                is Step.Torrent -> {
                    SheetTitle(s.lookup.name)
                    val sum = s.lookup.files.filter { it.index in chosen }.sumOf { it.size }
                    Text(
                        stringResource(R.string.torrent_chosen, chosen.size, s.lookup.files.size, Format.size(sum)),
                        style = MaterialTheme.typography.bodySmall,
                    )
                    if (s.lookup.known) {
                        Text(stringResource(R.string.torrent_known), color = MaterialTheme.colorScheme.error)
                    }
                    Row {
                        TextButton(onClick = { chosen.clear(); chosen.addAll(s.lookup.files.map { it.index }) }) {
                            Text(stringResource(R.string.select_all))
                        }
                        TextButton(onClick = { chosen.clear() }) { Text(stringResource(R.string.select_none)) }
                    }
                    LazyColumn(Modifier.heightIn(max = 320.dp)) {
                        itemsIndexed(s.lookup.files, key = { _, f -> f.index }) { _, f ->
                            Row(
                                Modifier.fillMaxWidth()
                                    .clickable(role = Role.Checkbox) { if (f.index in chosen) chosen.remove(f.index) else chosen.add(f.index) }
                                    .padding(vertical = 2.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Checkbox(checked = f.index in chosen, onCheckedChange = null)
                                Text(f.path, Modifier.weight(1f).padding(start = 8.dp), maxLines = 2, overflow = TextOverflow.Ellipsis)
                                Text(Format.size(f.size), style = MaterialTheme.typography.bodySmall)
                            }
                        }
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(stringResource(R.string.torrent_in_order), Modifier.weight(1f))
                        Switch(checked = inOrder, onCheckedChange = { inOrder = it })
                    }
                    SheetActions(
                        stringResource(if (later) R.string.add_queue else R.string.add_start),
                        enabled = chosen.isNotEmpty() && !s.lookup.known,
                        onCancel = { close() },
                        onAction = {
                            scope.launch {
                                step = Step.Working
                                val all = chosen.size == s.lookup.files.size
                                val r = Mdm.torrentAdd(s.token, if (all) emptyList() else chosen.toList(), later, category, inOrder)
                                if (r.containsKey("error")) {
                                    step = Step.Problem(r["error"].toString(), null)
                                } else {
                                    if (!later) DownloadService.start(context)
                                    close()
                                }
                            }
                        },
                    )
                }
                is Step.NeedGalleryDl -> {
                    SheetTitle(stringResource(R.string.gallery_needed_title), stringResource(R.string.gallery_needed_text))
                    SheetActions(stringResource(R.string.ytdlp_install), enabled = true, download = false, onCancel = { addPlain(listOf(s.address)) }) {
                        scope.launch {
                            step = Step.Working
                            val h = Mdm.installGallery()
                            if (h.gallery != null) gallery(s.address) else step = Step.Problem(h.galleryError ?: lookupFailed, s.address)
                        }
                    }
                }
                is Step.NeedYtdlp -> {
                    SheetTitle(stringResource(R.string.ytdlp_needed_title))
                    Text(stringResource(R.string.ytdlp_needed_text), style = MaterialTheme.typography.bodyMedium)
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        OutlinedButton(onClick = { addPlain(listOf(s.address)) }) {
                            Text(stringResource(R.string.add_as_file))
                        }
                        Button(onClick = {
                            scope.launch {
                                step = Step.Working
                                val h = Mdm.installYtdlp()
                                if (h.ytdlp != null) lookUp(s.address) else step = Step.Problem(h.ytdlpError ?: lookupFailed, s.address)
                            }
                        }) { Text(stringResource(R.string.ytdlp_install)) }
                    }
                }
                is Step.Problem -> {
                    Text(s.text, color = MaterialTheme.colorScheme.error)
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        TextButton(onClick = { step = Step.Links }) { Text(stringResource(R.string.back)) }
                        if (s.address != null) {
                            OutlinedButton(onClick = { addPlain(listOf(s.address)) }) {
                                Text(stringResource(R.string.add_as_file))
                            }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CategoryPicker(snapshot: Snapshot?, chosen: String?, onChoose: (String?) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val auto = stringResource(R.string.category_auto)
    val categories = snapshot?.categories.orEmpty()
    ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }) {
        OutlinedTextField(
            value = categories.firstOrNull { it.key == chosen }?.name ?: auto,
            onValueChange = {},
            readOnly = true,
            label = { Text(stringResource(R.string.category)) },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(open) },
            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable),
        )
        ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(text = { Text(auto) }, onClick = { onChoose(null); open = false })
            categories.forEach { c ->
                DropdownMenuItem(text = { Text(c.name) }, onClick = { onChoose(c.key); open = false })
            }
        }
    }
}
