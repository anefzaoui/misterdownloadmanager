package com.misterdownloadmanager.android.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Details
import com.misterdownloadmanager.android.core.Download
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.core.Server
import com.misterdownloadmanager.android.core.Snapshot
import com.misterdownloadmanager.android.service.DownloadService
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** One download: the desktop's progress window and Properties, as a waybill. */
@Composable
fun DetailsScreen(id: Long, snapshot: Snapshot?, onBack: () -> Unit, preview: Boolean = false) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var details by remember { mutableStateOf<Details?>(null) }
    var confirmDelete by remember { mutableStateOf(false) }
    val row = snapshot?.downloads?.firstOrNull { it.id == id }

    // Asked again every second while it runs: the parts and connections move.
    LaunchedEffect(id, row?.running) {
        if (preview) return@LaunchedEffect
        // A finished download's server waits for the location table, which loads once: asked
        // again a few times.
        var tries = 0
        while (true) {
            details = Mdm.details(id)
            val d = details
            val waiting = d != null && d.server == null && d.download.kind == "file" && tries++ < 5
            if (d?.download?.running != true && !waiting) break
            delay(1000)
        }
    }
    DetailsContent(
        row = row,
        details = details,
        onBack = onBack,
        onPause = { scope.launch { Mdm.act("pause", listOf(id)) } },
        onResume = {
            scope.launch {
                Mdm.act("start", listOf(id))
                DownloadService.start(context)
            }
        },
        onOpen = { row?.let { openFile(context, it) } },
        onShare = { row?.let { shareFile(context, it) } },
        onRemove = { confirmDelete = true },
    )
    if (confirmDelete) {
        DeleteDialog(1, onDismiss = { confirmDelete = false }) { withFiles ->
            confirmDelete = false
            scope.launch {
                Mdm.act(if (withFiles) "delete" else "remove", listOf(id))
                onBack()
            }
        }
    }
}

@Composable
fun DetailsContent(
    row: Download?,
    details: Details?,
    onBack: () -> Unit,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onOpen: () -> Unit,
    onShare: () -> Unit,
    onRemove: () -> Unit,
) {
    val b = brand
    Column(Modifier.fillMaxSize().background(b.board)) {
        Board(
            title = "",
            navigation = {
                OnBoard { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } }
            },
            actions = {
                OnBoard {
                    if (row?.complete == true) {
                        IconButton(onClick = onShare) { Icon(Icons.Outlined.Share, stringResource(R.string.action_share)) }
                    }
                    IconButton(onClick = onRemove) { Icon(Icons.Outlined.Delete, stringResource(R.string.action_remove)) }
                }
            },
        ) {
            if (row != null) Headline(row)
        }
        Surface(
            color = MaterialTheme.colorScheme.surface,
            shape = RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            if (row == null) {
                Text(stringResource(R.string.download_gone), Modifier.padding(24.dp))
                return@Surface
            }
            Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().padding(bottom = 24.dp)) {
                Actions(row, onPause, onResume, onOpen)
                val size = row.size
                val segments = details?.segments
                if (!row.complete && size != null && size > 0) {
                    SheetHeading(stringResource(R.string.prop_parts), segments?.size)
                    SegmentsBar(segments.orEmpty(), size, row.fraction ?: 0.0, Modifier.padding(horizontal = 20.dp))
                }
                SheetHeading(stringResource(R.string.prop_details))
                Fields(row, details)
                SheetHeading(stringResource(R.string.prop_where))
                LongField(stringResource(R.string.prop_address), row.url)
                LongField(stringResource(R.string.prop_saved_to), row.path)
                details?.server?.let { ServerField(it) }
                details?.page?.let { LongField(stringResource(R.string.prop_page), it) }
                row.error?.let { LongField(stringResource(R.string.status_error), it, error = true) }
                val log = details?.log.orEmpty()
                if (log.isNotEmpty()) {
                    SheetHeading(stringResource(R.string.prop_log), log.size)
                    SelectionContainer {
                        Column(Modifier.padding(horizontal = 20.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            log.forEach {
                                Text(it.text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
            }
        }
    }
}

/** On the board: the file, and how far it has come in figures large enough to read across a room. */
@Composable
private fun Headline(d: Download) {
    val b = brand
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 22.dp)) {
        Text(d.file, style = MaterialTheme.typography.headlineSmall, color = b.onBoard, maxLines = 3, overflow = TextOverflow.Ellipsis)
        Spacer(Modifier.height(16.dp))
        Row(verticalAlignment = Alignment.Bottom) {
            val big = when {
                d.complete -> stringResource(R.string.tag_done)
                d.failed -> stringResource(R.string.tag_failed)
                else -> d.fraction?.let { Format.percent(it) } ?: Format.size(d.downloaded)
            }
            if (d.complete) {
                // Arrived: the green stamp (green by law: the delivered state only).
                Box(
                    Modifier.padding(bottom = 8.dp).size(40.dp).background(b.go, androidx.compose.foundation.shape.CircleShape),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Filled.Check, null, tint = b.onGo) }
                Spacer(Modifier.width(12.dp))
            }
            Text(big, style = MaterialTheme.typography.displayMedium, color = b.onBoard)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.padding(bottom = 6.dp)) {
                val state = when {
                    d.running -> d.stage?.takeIf { it != "Downloading" }?.let { stageText(it) }
                        ?: (if (d.speed > 0) Format.speed(d.speed) else stringResource(R.string.stage_connecting))
                    d.complete -> d.size?.let(Format::size) ?: ""
                    d.status == "Queued" -> stringResource(R.string.tag_queued)
                    d.failed -> ""
                    else -> stringResource(R.string.tag_paused)
                }
                Text(state, style = MaterialTheme.typography.titleMedium.merge(Numbers), color = b.onBoard)
                if (d.running && d.eta != null) {
                    Text(
                        stringResource(R.string.eta_left, Format.eta(d.eta)),
                        style = MaterialTheme.typography.bodyMedium.merge(Numbers),
                        color = b.onBoard2,
                    )
                }
            }
        }
    }
}

@Composable
private fun Actions(d: Download, onPause: () -> Unit, onResume: () -> Unit, onOpen: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        val m = Modifier.weight(1f).heightIn(min = 48.dp)
        when {
            d.complete -> Button(onClick = onOpen, modifier = m) {
                Icon(Icons.AutoMirrored.Outlined.OpenInNew, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.action_open))
            }
            d.running -> FilledTonalButton(onClick = onPause, modifier = m) {
                Icon(Icons.Filled.Pause, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.action_pause))
            }
            d.canStart -> Button(onClick = onResume, modifier = m) {
                Icon(Icons.Filled.PlayArrow, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(if (d.failed) R.string.action_retry else R.string.action_resume))
            }
        }
    }
}

/** Name over value, two to a line, the way the waybill prints its fields. */
@Composable
private fun Fields(d: Download, details: Details?) {
    val pairs = buildList {
        d.size?.let { add(stringResource(R.string.prop_size) to Format.size(it)) }
        if (!d.complete) add(stringResource(R.string.prop_downloaded) to Format.size(d.downloaded))
        if (d.running) {
            add(stringResource(R.string.field_speed) to Format.speed(d.speed))
            d.eta?.let { add(stringResource(R.string.prop_time_left) to Format.eta(it)) }
            add(stringResource(R.string.prop_connections) to d.connections.toString())
        }
        d.resumable?.let { add(stringResource(R.string.prop_resume) to stringResource(if (it) R.string.yes else R.string.no)) }
        add(stringResource(R.string.category) to d.category)
        d.added?.let { add(stringResource(R.string.prop_added) to it) }
    }
    Column(Modifier.padding(horizontal = 20.dp)) {
        pairs.chunked(2).forEachIndexed { i, line ->
            if (i > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            Row(Modifier.fillMaxWidth().padding(vertical = 10.dp)) {
                line.forEach { (n, v) -> Field(n, v, Modifier.weight(1f)) }
                if (line.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun LongField(name: String, value: String, error: Boolean = false) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 8.dp)) {
        Text(name.uppercase(), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.height(2.dp))
        SelectionContainer {
            Text(
                value,
                style = MaterialTheme.typography.bodyMedium,
                color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}

/**
 * Where the server is, as the desktop's progress window says it: the country's flag and name and
 * the round trip; under it what the desktop's tooltip says (the address, a CDN or a proxy, and the
 * location data's credit, which its licence asks for wherever its results show).
 */
@Composable
private fun ServerField(server: Server) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 8.dp)) {
        Text(
            stringResource(R.string.prop_server).uppercase(),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(2.dp))
        val flag = server.country?.let(::flagOf)
        val line = listOfNotNull(flag?.let { "$it ${server.place}" } ?: server.place, server.time)
        DotLine(line, MaterialTheme.typography.bodyMedium, MaterialTheme.colorScheme.onSurface)
        if (server.about.isNotBlank()) {
            Spacer(Modifier.height(2.dp))
            SelectionContainer {
                Text(server.about, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

/** A country's flag emoji from its ISO 3166 code (its two regional indicator letters). */
private fun flagOf(code: String): String? {
    if (code.length != 2 || !code.all { it in 'A'..'Z' || it in 'a'..'z' }) return null
    return code.uppercase().map { String(Character.toChars(0x1F1E6 + (it - 'A'))) }.joinToString("")
}

/**
 * The file's parts, as the desktop's progress window draws them: each connection's share of the
 * file, filled as far as it has come. Without the parts (a stream, a torrent) the whole bar fills.
 */
@Composable
private fun SegmentsBar(segments: List<List<Long>>, size: Long, fraction: Double, modifier: Modifier = Modifier) {
    val done = MaterialTheme.colorScheme.primary
    val empty = MaterialTheme.colorScheme.surfaceContainerHighest
    val edge = MaterialTheme.colorScheme.surface
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    Box(modifier.fillMaxWidth().height(28.dp)) {
        Canvas(Modifier.fillMaxSize()) {
            val r = CornerRadius(6.dp.toPx())
            drawRoundRect(empty, cornerRadius = r)
            val w = this.size.width
            fun x(at: Float, width: Float) = if (rtl) w - at - width else at
            if (segments.isEmpty()) {
                val fw = (fraction.toFloat() * w).coerceIn(0f, w)
                drawRoundRect(done, topLeft = Offset(x(0f, fw), 0f), size = Size(fw, this.size.height), cornerRadius = r)
                return@Canvas
            }
            for (s in segments) {
                if (s.size < 3) continue
                val start = s[0].toFloat() / size * w
                val cursor = s[1].toFloat() / size * w
                val end = s[2].toFloat() / size * w
                val fw = (cursor - start).coerceAtLeast(0f)
                if (fw > 0f) drawRect(done, topLeft = Offset(x(start, fw), 0f), size = Size(fw, this.size.height))
                // Where one part ends and the next begins.
                drawRect(edge, topLeft = Offset(x(end, 0f) - 1f, 0f), size = Size(2f, this.size.height))
            }
        }
    }
}
