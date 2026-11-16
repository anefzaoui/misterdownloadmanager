package com.misterdownloadmanager.android.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.webkit.MimeTypeMap
import android.widget.Toast
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.ContentPaste
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.FilterList
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedIconButton
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.runtime.toMutableStateList
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Download
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.core.Snapshot
import com.misterdownloadmanager.android.service.DownloadService
import kotlinx.coroutines.launch
import java.io.File

/** The list's state filters. */
enum class Filter { All, Active, Waiting, Done, Failed }

private fun Filter.matches(d: Download) = when (this) {
    Filter.All -> true
    Filter.Active -> d.running
    Filter.Waiting -> !d.running && !d.complete && !d.failed
    Filter.Done -> d.complete
    Filter.Failed -> d.failed
}

/** In progress first (running, failed, paused, queued), then what has arrived, newest first. */
private fun order(d: Download) = when {
    d.running -> 0
    d.failed -> 1
    d.status == "Stopped" -> 2
    d.complete -> 4
    else -> 3
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ListScreen(
    snapshot: Snapshot?,
    startError: String?,
    onOpen: (Long) -> Unit,
    onAdd: () -> Unit,
    bottomPadding: androidx.compose.ui.unit.Dp = 0.dp,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    var filter by rememberSaveable { mutableStateOf(Filter.All) }
    var category by rememberSaveable { mutableStateOf<String?>(null) }
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    val selected: SnapshotStateList<Long> = remember { emptyList<Long>().toMutableStateList() }
    var confirmDelete by remember { mutableStateOf(false) }

    LaunchedEffect(snapshot?.notice) {
        val n = snapshot?.notice ?: return@LaunchedEffect
        snackbar.showSnackbar(n.joinToString(": "))
        Mdm.dismissNotice()
    }

    val all = snapshot?.downloads.orEmpty()
    val shown = all.filter { d ->
        filter.matches(d) && (category == null || d.category == category) &&
            (query.isBlank() || d.file.contains(query, ignoreCase = true) || d.url.contains(query, ignoreCase = true))
    }.sortedWith(compareBy<Download> { order(it) }.thenByDescending { it.id })
    val (inProgress, finished) = shown.partition { !it.complete }
    selected.retainAll(all.map { it.id }.toSet())

    fun act(action: String, ids: Collection<Long>) = scope.launch {
        val answer = Mdm.act(action, ids)
        if (action == "start") DownloadService.start(context)
        answer["error"]?.let { snackbar.showSnackbar(it.toString()) }
    }

    Box(Modifier.fillMaxSize().background(brand.board)) {
        Column(Modifier.fillMaxSize()) {
            if (selected.isNotEmpty()) {
                SelectionBoard(
                    count = selected.size,
                    onClose = { selected.clear() },
                    onResume = { act("start", selected.toList()) },
                    onPause = { act("pause", selected.toList()) },
                    onDelete = { confirmDelete = true },
                )
            } else {
                Board(
                    title = stringResource(R.string.nav_downloads),
                    actions = {
                        OnBoard {
                            IconButton(onClick = { searching = !searching; if (!searching) query = "" }) {
                                Icon(if (searching) Icons.Default.Close else Icons.Outlined.Search, stringResource(R.string.action_search))
                            }
                            CategoryMenu(snapshot, all, category) { category = it }
                        }
                    },
                ) {
                    AnimatedVisibility(searching) { SearchField(query) { query = it } }
                    StatusFields(snapshot)
                    if (all.isNotEmpty()) FilterTabs(filter, all) { filter = it } else Spacer(Modifier.height(6.dp))
                }
            }
            Surface(
                color = MaterialTheme.colorScheme.surface,
                shape = RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp),
                modifier = Modifier.fillMaxSize(),
            ) {
                when {
                    startError != null -> Message(stringResource(R.string.core_failed, startError))
                    snapshot == null -> Message(stringResource(R.string.starting))
                    all.isEmpty() -> EmptyState(onAdd)
                    shown.isEmpty() -> Message(stringResource(R.string.empty_filter))
                    else -> LazyColumn(contentPadding = PaddingValues(bottom = 96.dp + bottomPadding)) {
                        if (inProgress.isNotEmpty()) {
                            item(key = "h-progress") { SheetHeading(stringResource(R.string.heading_in_progress), inProgress.size) }
                            items(inProgress, key = { it.id }) { d -> Row(d, selected, ::act, onOpen) }
                        }
                        if (finished.isNotEmpty()) {
                            item(key = "h-finished") { SheetHeading(stringResource(R.string.heading_finished), finished.size) }
                            items(finished, key = { it.id }) { d -> Row(d, selected, ::act, onOpen) }
                        }
                    }
                }
            }
        }
        SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).padding(bottom = 88.dp + bottomPadding))
    }

    if (confirmDelete) {
        DeleteDialog(selected.size, onDismiss = { confirmDelete = false }) { withFiles ->
            act(if (withFiles) "delete" else "remove", selected.toList())
            selected.clear()
            confirmDelete = false
        }
    }
}

@Composable
private fun Row(
    d: Download,
    selected: SnapshotStateList<Long>,
    act: (String, Collection<Long>) -> Unit,
    onOpen: (Long) -> Unit,
) {
    val context = LocalContext.current
    DownloadRow(
        d = d,
        selected = d.id in selected,
        selecting = selected.isNotEmpty(),
        onClick = {
            when {
                selected.isNotEmpty() -> if (d.id in selected) selected.remove(d.id) else selected.add(d.id)
                else -> onOpen(d.id)
            }
        },
        onLongClick = { if (d.id !in selected) selected.add(d.id) },
        onToggle = { act(if (d.running) "pause" else "start", listOf(d.id)) },
        onOpenFile = { openFile(context, d) },
    )
}

/** Speed, how many run, free space: the list's state as fields on the board. */
@Composable
private fun StatusFields(s: Snapshot?) {
    val b = brand
    Row(
        Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 2.dp, bottom = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(28.dp),
    ) {
        Field(
            stringResource(R.string.field_speed),
            if ((s?.running ?: 0) > 0) Format.speed(s?.speed ?: 0) else "—",
            nameColor = b.onBoard2, valueColor = b.onBoard,
        )
        Field(
            stringResource(R.string.field_active),
            Format.of("${s?.running ?: 0}", "${s?.downloads?.count { !it.complete } ?: 0}"),
            nameColor = b.onBoard2, valueColor = b.onBoard,
        )
        s?.free?.let {
            Field(stringResource(R.string.field_free), Format.size(it), nameColor = b.onBoard2, valueColor = b.onBoard)
        }
    }
}

/** The state filters as segmented tabs on the board: the chosen one printed on paper. */
@Composable
private fun FilterTabs(filter: Filter, all: List<Download>, onChoose: (Filter) -> Unit) {
    val b = brand
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Filter.entries.forEach { f ->
            val count = all.count(f::matches)
            if (f != Filter.All && f != filter && count == 0) return@forEach
            val on = f == filter
            val bg by animateColorAsState(if (on) MaterialTheme.colorScheme.surface else b.board, label = "tab")
            Row(
                Modifier
                    .heightIn(min = 40.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .background(bg)
                    .border(1.25.dp, if (on) bg else b.onBoard2.copy(alpha = 0.55f), RoundedCornerShape(8.dp))
                    .clickable { onChoose(f) }
                    .padding(horizontal = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                val fg = if (on) MaterialTheme.colorScheme.primary else b.onBoard
                Text(
                    stringResource(
                        when (f) {
                            Filter.All -> R.string.filter_all
                            Filter.Active -> R.string.filter_active
                            Filter.Waiting -> R.string.filter_waiting
                            Filter.Done -> R.string.filter_done
                            Filter.Failed -> R.string.filter_failed
                        },
                    ),
                    style = MaterialTheme.typography.labelLarge,
                    color = fg,
                )
                if (f != Filter.All) {
                    Text(count.toString(), style = MaterialTheme.typography.labelLarge.merge(Numbers), color = fg.copy(alpha = 0.65f))
                }
            }
        }
    }
}

@Composable
private fun SearchField(query: String, onChange: (String) -> Unit) {
    val b = brand
    Row(
        Modifier
            .fillMaxWidth()
            .padding(start = 16.dp, end = 16.dp, bottom = 12.dp)
            .heightIn(min = 48.dp)
            .background(b.boardEdge, RoundedCornerShape(10.dp))
            .padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Outlined.Search, null, tint = b.onBoard2, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(10.dp))
        Box(Modifier.weight(1f)) {
            if (query.isEmpty()) Text(stringResource(R.string.search_hint), color = b.onBoard2, style = MaterialTheme.typography.bodyLarge)
            BasicTextField(
                value = query,
                onValueChange = onChange,
                singleLine = true,
                textStyle = MaterialTheme.typography.bodyLarge.copy(color = b.onBoard),
                cursorBrush = SolidColor(b.onBoard),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@Composable
private fun CategoryMenu(s: Snapshot?, all: List<Download>, chosen: String?, onChoose: (String?) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) {
            Icon(Icons.Outlined.FilterList, stringResource(R.string.action_filter))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text(stringResource(R.string.category_all)) },
                leadingIcon = { if (chosen == null) Icon(Icons.Default.Check, null) },
                onClick = { onChoose(null); open = false },
            )
            s?.categories?.filter { c -> all.any { it.category == c.key } }?.forEach { c ->
                DropdownMenuItem(
                    text = { Text(c.name) },
                    leadingIcon = { Icon(if (chosen == c.key) Icons.Default.Check else iconForCategory(c.key), null) },
                    onClick = { onChoose(c.key); open = false },
                )
            }
        }
    }
}

@Composable
private fun SelectionBoard(count: Int, onClose: () -> Unit, onResume: () -> Unit, onPause: () -> Unit, onDelete: () -> Unit) {
    Board(
        title = pluralStringResource(R.plurals.selected, count, count),
        navigation = {
            OnBoard { IconButton(onClick = onClose) { Icon(Icons.Default.Close, stringResource(R.string.action_clear_selection)) } }
        },
        actions = {
            OnBoard {
                IconButton(onClick = onResume) { Icon(Icons.Filled.PlayArrow, stringResource(R.string.action_resume)) }
                IconButton(onClick = onPause) { Icon(Icons.Filled.Pause, stringResource(R.string.action_pause)) }
                IconButton(onClick = onDelete) { Icon(Icons.Outlined.Delete, stringResource(R.string.action_delete)) }
            }
        },
    ) { Spacer(Modifier.height(12.dp)) }
}

@Composable
private fun Message(text: String) {
    Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
        Text(text, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** No download yet: how to bring one, and the one action to do it. */
@Composable
private fun EmptyState(onAdd: () -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(horizontal = 28.dp, vertical = 36.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Text(stringResource(R.string.empty_title), style = MaterialTheme.typography.headlineSmall)
        Text(stringResource(R.string.empty_text), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.height(4.dp))
        Way(Icons.Outlined.Share, stringResource(R.string.empty_share))
        Way(Icons.Outlined.ContentPaste, stringResource(R.string.empty_paste))
        Way(Icons.Outlined.Hub, stringResource(R.string.empty_torrent))
        Spacer(Modifier.height(8.dp))
        Button(
            onClick = onAdd,
            colors = ButtonDefaults.buttonColors(containerColor = brand.go, contentColor = brand.onGo),
            modifier = Modifier.heightIn(min = 48.dp),
        ) {
            Icon(Icons.Outlined.Download, null, Modifier.size(20.dp))
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.action_paste_link))
        }
    }
}

@Composable
private fun Way(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier.size(36.dp).background(MaterialTheme.colorScheme.surfaceContainer, CircleShape),
            contentAlignment = Alignment.Center,
        ) { Icon(icon, null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary) }
        Spacer(Modifier.width(14.dp))
        Text(text, style = MaterialTheme.typography.bodyMedium)
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun DownloadRow(
    d: Download,
    selected: Boolean,
    selecting: Boolean,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
    onToggle: () -> Unit,
    onOpenFile: () -> Unit,
) {
    val c = MaterialTheme.colorScheme
    val bg by animateColorAsState(if (selected) c.secondaryContainer else c.surface, label = "row")
    Column(Modifier.background(bg)) {
        Row(
            Modifier
                .fillMaxWidth()
                .combinedClickable(onClick = onClick, onLongClick = onLongClick)
                .padding(start = 20.dp, end = 8.dp, top = 12.dp, bottom = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (selected) {
                Box(Modifier.size(44.dp).background(c.primary, RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
                    Icon(Icons.Default.Check, null, tint = c.onPrimary)
                }
            } else {
                FileTile(d)
            }
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(d.file, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Spacer(Modifier.height(6.dp))
                if (!d.complete && !d.failed) {
                    Progress(d)
                    Spacer(Modifier.height(6.dp))
                }
                RowMeta(d)
            }
            Spacer(Modifier.width(4.dp))
            if (!selecting) {
                when {
                    d.running -> {
                        val label = stringResource(R.string.action_pause)
                        FilledTonalIconButton(onClick = onToggle, modifier = Modifier.semantics { contentDescription = label }) {
                            Icon(Icons.Filled.Pause, null)
                        }
                    }
                    d.canStart -> {
                        val label = stringResource(R.string.action_resume)
                        OutlinedIconButton(
                            onClick = onToggle,
                            modifier = Modifier.semantics { contentDescription = label },
                            colors = IconButtonDefaults.outlinedIconButtonColors(contentColor = c.primary),
                        ) { Icon(Icons.Filled.PlayArrow, null) }
                    }
                    d.complete -> {
                        val label = stringResource(R.string.action_open)
                        IconButton(onClick = onOpenFile, modifier = Modifier.semantics { contentDescription = label }) {
                            Icon(Icons.AutoMirrored.Outlined.OpenInNew, null, tint = c.onSurfaceVariant)
                        }
                    }
                    else -> Spacer(Modifier.width(48.dp))
                }
            }
        }
        HorizontalDivider(Modifier.padding(start = 78.dp), color = c.outlineVariant)
    }
}

@Composable
private fun Progress(d: Download) {
    val c = MaterialTheme.colorScheme
    val color = if (d.running) c.primary else c.outline
    val f = d.fraction
    val m = Modifier.fillMaxWidth().height(6.dp).clip(RoundedCornerShape(3.dp))
    if (d.running && (f == null || d.stage == "Probing")) {
        LinearProgressIndicator(m, color = color, trackColor = c.surfaceContainerHighest, strokeCap = StrokeCap.Round)
    } else {
        LinearProgressIndicator(
            progress = { (f ?: 0.0).toFloat() },
            modifier = m,
            color = color,
            trackColor = c.surfaceContainerHighest,
            strokeCap = StrokeCap.Round,
            gapSize = 0.dp,
            drawStopIndicator = {},
        )
    }
}

/** The row's second line: its state as a tag, and the numbers that matter in that state. */
@Composable
private fun RowMeta(d: Download) {
    val c = MaterialTheme.colorScheme
    val small = MaterialTheme.typography.bodySmall
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        when {
            d.complete -> Tag(stringResource(R.string.tag_done), TagKind.Done)
            d.failed -> Tag(stringResource(R.string.tag_failed), TagKind.Error)
            d.running && d.stage != null && d.stage != "Downloading" -> Tag(stageShort(d.stage), TagKind.Info)
            d.status == "Queued" -> Tag(stringResource(R.string.tag_queued))
            !d.running -> Tag(stringResource(R.string.tag_paused))
        }
        val text = when {
            d.failed -> d.error ?: stringResource(R.string.status_error)
            d.complete -> listOfNotNull(d.size?.let(Format::size), d.added).joinToString(" · ")
            d.running -> listOfNotNull(
                d.size?.let { Format.of(Format.size(d.downloaded), Format.size(it)) } ?: Format.size(d.downloaded),
                Format.speed(d.speed).takeIf { d.speed > 0 },
                d.eta?.let { stringResource(R.string.eta_left, Format.eta(it)) },
            ).joinToString(" · ")
            else -> d.size?.let { if (d.downloaded > 0) Format.of(Format.size(d.downloaded), Format.size(it)) else Format.size(it) } ?: ""
        }
        Text(
            text,
            style = small,
            color = if (d.failed) c.error else c.onSurfaceVariant,
            maxLines = if (d.failed) 2 else 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (!d.complete && !d.failed && d.fraction != null) {
            Text(
                Format.percent(d.fraction),
                style = MaterialTheme.typography.labelMedium.merge(Numbers),
                color = if (d.running) c.primary else c.onSurfaceVariant,
            )
        }
    }
}

@Composable
fun statusLine(d: Download): String {
    val size = d.size?.let(Format::size)
    return when {
        d.complete -> listOfNotNull(size, stringResource(R.string.status_complete)).joinToString(" · ")
        d.failed -> d.error ?: stringResource(R.string.status_error)
        d.running -> listOfNotNull(
            d.stage?.takeIf { it != "Downloading" }?.let { stageText(it) },
            size?.let { Format.of(Format.size(d.downloaded), it) } ?: Format.size(d.downloaded),
            Format.speed(d.speed),
            d.eta?.let(Format::eta),
        ).joinToString(" · ")
        d.status == "Queued" -> listOfNotNull(size, stringResource(R.string.status_queued)).joinToString(" · ")
        else -> listOfNotNull(size?.let { Format.of(Format.size(d.downloaded), it) }, stringResource(R.string.status_stopped)).joinToString(" · ")
    }
}

/** A stage in one word, for a row's tag. */
@Composable
fun stageShort(stage: String): String = when (stage) {
    "Probing" -> stringResource(R.string.stage_short_connecting)
    "Verifying" -> stringResource(R.string.stage_short_verifying)
    "Merging" -> stringResource(R.string.stage_short_merging)
    "Completing" -> stringResource(R.string.stage_short_finishing)
    else -> stage
}

@Composable
fun stageText(stage: String): String = when (stage) {
    "Probing" -> stringResource(R.string.stage_connecting)
    "Verifying" -> stringResource(R.string.stage_verifying)
    "Merging" -> stringResource(R.string.stage_merging)
    "Completing" -> stringResource(R.string.stage_completing)
    else -> stage
}

/** Open a finished file with the app that takes its type. */
fun openFile(context: Context, d: Download) {
    val file = File(d.path)
    if (!file.exists()) {
        Toast.makeText(context, R.string.file_missing, Toast.LENGTH_LONG).show()
        return
    }
    val uri = FileProvider.getUriForFile(context, context.packageName + ".files", file)
    val type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(file.extension.lowercase()) ?: "*/*"
    val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, type)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    try {
        context.startActivity(Intent.createChooser(intent, file.name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    } catch (_: ActivityNotFoundException) {
        Toast.makeText(context, R.string.no_app_for_file, Toast.LENGTH_LONG).show()
    }
}

/** Share a finished file. */
fun shareFile(context: Context, d: Download) {
    val file = File(d.path)
    if (!file.exists()) return
    val uri = FileProvider.getUriForFile(context, context.packageName + ".files", file)
    val type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(file.extension.lowercase()) ?: "*/*"
    val intent = Intent(Intent.ACTION_SEND).setType(type).putExtra(Intent.EXTRA_STREAM, uri)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(Intent.createChooser(intent, file.name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}
