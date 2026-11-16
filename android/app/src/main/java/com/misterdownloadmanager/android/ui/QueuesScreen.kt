package com.misterdownloadmanager.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.AccessTime
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.PowerSettingsNew
import androidx.compose.material.icons.outlined.Stop
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberTimePickerState
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.core.Snapshot
import com.misterdownloadmanager.android.service.DownloadService
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.text.DateFormatSymbols

/** Sample queues for the screenshots (`preview`). */
val PreviewQueues: List<JsonObject> = listOf(
    buildJsonObject {
        put("name", JsonPrimitive("Main download queue"))
        put("files_at_once", JsonPrimitive(3))
        put("schedule", buildJsonObject {
            put("start_at", JsonArray(listOf(2, 30, 0).map(::JsonPrimitive)))
            put("stop_at", JsonArray(listOf(7, 0, 0).map(::JsonPrimitive)))
            put("days", JsonArray(listOf(false, true, true, true, true, true, false).map(::JsonPrimitive)))
            put("start_on_startup", JsonPrimitive(false))
        })
    },
    buildJsonObject {
        put("name", JsonPrimitive("Synchronization queue"))
        put("files_at_once", JsonPrimitive(4))
        put("schedule", buildJsonObject {})
    },
)

/** The queues and the Scheduler: each queue's downloads at once, when it starts and stops. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun QueuesScreen(snapshot: Snapshot?, preview: Boolean = false) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var queues by remember { mutableStateOf(if (preview) PreviewQueues else emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    var newQueue by remember { mutableStateOf(false) }
    var picking by remember { mutableStateOf<Pair<String, String>?>(null) }

    suspend fun reload() {
        queues = (Mdm.request("queues")["queues"] as? JsonArray)?.map { it.jsonObject }.orEmpty()
    }
    LaunchedEffect(Unit) { if (!preview) reload() }

    fun patch(queue: String, patch: JsonObject) = scope.launch {
        val r = Mdm.request("set_queue") {
            put("queue", JsonPrimitive(queue))
            put("patch", patch)
        }
        error = r["error"]?.jsonPrimitive?.content
        reload()
    }

    fun schedule(queue: String, key: String, value: JsonElement) =
        patch(queue, buildJsonObject { put("schedule", buildJsonObject { put(key, value) }) })

    Column(Modifier.fillMaxSize().background(brand.board)) {
        Board(
            title = stringResource(R.string.nav_queues),
            actions = { OnBoard { IconButton(onClick = { newQueue = true }) { Icon(Icons.Outlined.Add, stringResource(R.string.queue_new)) } } },
        ) {
            Text(
                stringResource(R.string.scheduler_note),
                style = MaterialTheme.typography.bodyMedium,
                color = brand.onBoard2,
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, bottom = 18.dp),
            )
        }
        Surface(
            color = MaterialTheme.colorScheme.surface,
            shape = RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            Column(Modifier.verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(20.dp)) }
                queues.forEachIndexed { i, q ->
                    val name = q["name"]?.jsonPrimitive?.content ?: return@forEachIndexed
                    val info = snapshot?.queues?.firstOrNull { it.name == name }
                    if (i > 0) HorizontalDivider(Modifier.padding(top = 12.dp), color = MaterialTheme.colorScheme.outlineVariant)
                    Queue(
                        name = name,
                        q = q,
                        running = info?.running == true,
                        count = info?.ids?.size ?: 0,
                        onRun = { run ->
                            scope.launch {
                                Mdm.runQueue(name, run)
                                if (run) DownloadService.start(context)
                            }
                        },
                        onAtOnce = { patch(name, buildJsonObject { put("files_at_once", JsonPrimitive(it)) }) },
                        onPick = { key -> picking = name to key },
                        onSchedule = { k, v -> schedule(name, k, v) },
                    )
                }
            }
        }
    }

    picking?.let { (queue, key) ->
        val now = queues.firstOrNull { it["name"]?.jsonPrimitive?.content == queue }?.get("schedule")?.jsonObject?.get(key)
        val (h, m) = hm(now) ?: (2 to 0)
        val state = rememberTimePickerState(initialHour = h, initialMinute = m)
        AlertDialog(
            onDismissRequest = { picking = null },
            text = { TimePicker(state) },
            confirmButton = {
                TextButton(onClick = {
                    schedule(queue, key, JsonArray(listOf(JsonPrimitive(state.hour), JsonPrimitive(state.minute), JsonPrimitive(0))))
                    picking = null
                }) { Text(stringResource(android.R.string.ok)) }
            },
            dismissButton = { TextButton(onClick = { picking = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }

    if (newQueue) {
        var name by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { newQueue = false },
            title = { Text(stringResource(R.string.queue_new)) },
            text = { OutlinedTextField(name, { name = it }, singleLine = true, label = { Text(stringResource(R.string.queue_name)) }) },
            confirmButton = {
                TextButton(enabled = name.isNotBlank(), onClick = {
                    patch(name.trim(), JsonObject(emptyMap()))
                    newQueue = false
                }) { Text(stringResource(android.R.string.ok)) }
            },
            dismissButton = { TextButton(onClick = { newQueue = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
private fun Queue(
    name: String,
    q: JsonObject,
    running: Boolean,
    count: Int,
    onRun: (Boolean) -> Unit,
    onAtOnce: (Int) -> Unit,
    onPick: (String) -> Unit,
    onSchedule: (String, JsonElement) -> Unit,
) {
    val s = q["schedule"]?.jsonObject ?: JsonObject(emptyMap())
    Column(Modifier.padding(horizontal = 20.dp).padding(top = 20.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(name, style = MaterialTheme.typography.titleLarge)
                Spacer(Modifier.height(4.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Tag(stringResource(if (running) R.string.queue_running else R.string.queue_stopped), if (running) TagKind.Info else TagKind.Neutral)
                    Text(
                        androidx.compose.ui.res.pluralStringResource(R.plurals.queue_count, count, count),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (running) {
                FilledTonalButton(onClick = { onRun(false) }, modifier = Modifier.heightIn(min = 44.dp)) {
                    Icon(Icons.Outlined.Stop, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text(stringResource(R.string.queue_stop))
                }
            } else {
                Button(onClick = { onRun(true) }, modifier = Modifier.heightIn(min = 44.dp)) {
                    Icon(Icons.Filled.PlayArrow, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text(stringResource(R.string.queue_start))
                }
            }
        }
        Spacer(Modifier.height(16.dp))
        var atOnce by remember(q) { mutableFloatStateOf((q["files_at_once"]?.jsonPrimitive?.intOrNull ?: 4).toFloat()) }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.queue_at_once_label), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Text(atOnce.toInt().toString(), style = FieldValue)
        }
        Slider(
            value = atOnce, onValueChange = { atOnce = Math.round(it).toFloat() }, valueRange = 1f..10f,
            onValueChangeFinished = { onAtOnce(atOnce.toInt()) },
        )
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            TimeField(stringResource(R.string.queue_start_at), s["start_at"], Modifier.weight(1f), { onPick("start_at") }) {
                onSchedule("start_at", JsonNull)
            }
            TimeField(stringResource(R.string.queue_stop_at), s["stop_at"], Modifier.weight(1f), { onPick("stop_at") }) {
                onSchedule("stop_at", JsonNull)
            }
        }
        Spacer(Modifier.height(12.dp))
        val days = (s["days"] as? JsonArray)?.map { it.jsonPrimitive.booleanOrNull ?: true } ?: List(7) { true }
        val names = DateFormatSymbols.getInstance().shortWeekdays // 1 = Sunday … 7 = Saturday
        val c = MaterialTheme.colorScheme
        Text(stringResource(R.string.queue_days).uppercase(), style = MaterialTheme.typography.labelSmall, color = c.onSurfaceVariant)
        Spacer(Modifier.height(6.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            (0 until 7).forEach { d ->
                val on = days.getOrElse(d) { true }
                androidx.compose.foundation.layout.Box(
                    Modifier
                        .weight(1f)
                        .heightIn(min = 44.dp)
                        .background(if (on) c.primary else c.surfaceContainer, RoundedCornerShape(10.dp))
                        .clickable {
                            val now = days.toMutableList().also { it[d] = !it[d] }
                            onSchedule("days", JsonArray(now.map(::JsonPrimitive)))
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        names[d + 1].take(2),
                        style = MaterialTheme.typography.labelLarge,
                        color = if (on) c.onPrimary else c.onSurfaceVariant,
                    )
                }
            }
        }
        Row(
            Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable {
                onSchedule("start_on_startup", JsonPrimitive(!(s["start_on_startup"]?.jsonPrimitive?.booleanOrNull ?: false)))
            },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Outlined.PowerSettingsNew, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.width(16.dp))
            Text(stringResource(R.string.queue_on_startup), Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
            Switch(
                checked = s["start_on_startup"]?.jsonPrimitive?.booleanOrNull ?: false,
                onCheckedChange = { onSchedule("start_on_startup", JsonPrimitive(it)) },
            )
        }
    }
}

private fun hm(v: JsonElement?): Pair<Int, Int>? {
    val a = v as? JsonArray ?: return null
    return (a.getOrNull(0)?.jsonPrimitive?.intOrNull ?: return null) to (a.getOrNull(1)?.jsonPrimitive?.intOrNull ?: 0)
}

/** A start or stop time: its name over the time, tapped to choose it; Clear when set. */
@Composable
private fun TimeField(label: String, value: JsonElement?, modifier: Modifier, onPick: () -> Unit, onClear: () -> Unit) {
    val t = hm(value)
    val c = MaterialTheme.colorScheme
    Row(
        modifier
            .heightIn(min = 64.dp)
            .background(c.surfaceContainer, RoundedCornerShape(10.dp))
            .clickable(onClick = onPick)
            .padding(start = 14.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Outlined.AccessTime, null, tint = if (t != null) c.primary else c.onSurfaceVariant, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(10.dp))
        Field(label, t?.let { "%02d:%02d".format(it.first, it.second) } ?: stringResource(R.string.queue_not_set), Modifier.weight(1f))
        if (t != null) {
            IconButton(onClick = onClear) { Icon(Icons.Outlined.Close, stringResource(R.string.queue_clear), Modifier.size(18.dp)) }
        }
    }
}
