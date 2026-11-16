package com.misterdownloadmanager.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AutoAwesome
import androidx.compose.material.icons.outlined.ContentPaste
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.GraphicEq
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlinx.coroutines.delay
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.CategoryInfo
import com.misterdownloadmanager.android.core.VideoChoice
import com.misterdownloadmanager.android.core.VideoLookup

/** A sheet's title, condensed as the board's. */
@Composable
fun SheetTitle(text: String, sub: String? = null) {
    Column {
        Text(text, style = MaterialTheme.typography.headlineSmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
        if (sub != null) {
            Spacer(Modifier.height(4.dp))
            Text(sub, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Cancel, and the sheet's one action: green when it downloads (the logo's green, by law). */
@Composable
fun SheetActions(action: String, enabled: Boolean, onCancel: () -> Unit, download: Boolean = true, onAction: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        OutlinedButton(onClick = onCancel, modifier = Modifier.weight(1f).heightIn(min = 52.dp), shape = RoundedCornerShape(12.dp)) {
            Text(stringResource(R.string.cancel))
        }
        Button(
            onClick = onAction,
            enabled = enabled,
            modifier = Modifier.weight(1.4f).heightIn(min = 52.dp),
            shape = RoundedCornerShape(12.dp),
            colors = if (download) ButtonDefaults.buttonColors(containerColor = brand.go, contentColor = brand.onGo) else ButtonDefaults.buttonColors(),
        ) {
            if (download) {
                Icon(Icons.Outlined.Download, null, Modifier.size(20.dp))
                Spacer(Modifier.width(8.dp))
            }
            Text(action, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

/** The first step: the links, where they go, now or later. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LinksStep(
    text: String,
    onText: (String) -> Unit,
    onPaste: (() -> Unit)?,
    categories: List<CategoryInfo>,
    category: String?,
    onCategory: (String?) -> Unit,
    later: Boolean,
    onLater: (Boolean) -> Unit,
    onCancel: () -> Unit,
    onGo: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        SheetTitle(stringResource(R.string.add_title), stringResource(R.string.add_sub))
        OutlinedTextField(
            value = text,
            onValueChange = onText,
            label = { Text(stringResource(R.string.add_links_label)) },
            placeholder = { Text("https://…") },
            minLines = 2,
            maxLines = 6,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            trailingIcon = onPaste?.let { paste ->
                { IconButton(onClick = paste) { Icon(Icons.Outlined.ContentPaste, stringResource(R.string.action_paste)) } }
            },
            shape = RoundedCornerShape(12.dp),
            modifier = Modifier.fillMaxWidth(),
        )
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(stringResource(R.string.category).uppercase(), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilterChip(
                    selected = category == null,
                    onClick = { onCategory(null) },
                    label = { Text(stringResource(R.string.category_auto_short)) },
                    leadingIcon = { Icon(Icons.Outlined.AutoAwesome, null, Modifier.size(18.dp)) },
                    colors = chipColors(),
                )
                categories.filter { it.key != "General" }.forEach { c ->
                    FilterChip(
                        selected = category == c.key,
                        onClick = { onCategory(c.key) },
                        label = { Text(c.name) },
                        leadingIcon = { Icon(iconForCategory(c.key), null, Modifier.size(18.dp)) },
                        colors = chipColors(),
                    )
                }
            }
        }
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            SegmentedButton(
                selected = !later,
                onClick = { onLater(false) },
                shape = SegmentedButtonDefaults.itemShape(0, 2),
            ) { Text(stringResource(R.string.add_now)) }
            SegmentedButton(
                selected = later,
                onClick = { onLater(true) },
                shape = SegmentedButtonDefaults.itemShape(1, 2),
            ) { Text(stringResource(R.string.add_queue)) }
        }
        SheetActions(
            stringResource(if (later) R.string.add_queue else R.string.add_start),
            enabled = text.isNotBlank(),
            onCancel = onCancel,
            onAction = onGo,
        )
    }
}

@Composable
private fun chipColors() = FilterChipDefaults.filterChipColors(
    selectedContainerColor = MaterialTheme.colorScheme.primaryContainer,
    selectedLabelColor = MaterialTheme.colorScheme.onPrimaryContainer,
    selectedLeadingIconColor = MaterialTheme.colorScheme.onPrimaryContainer,
)

/** A look-up or an install under way. */
@Composable
fun WorkingStep(text: String) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 40.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CircularProgressIndicator(Modifier.size(28.dp))
        Spacer(Modifier.size(16.dp))
        Text(text, style = MaterialTheme.typography.bodyLarge)
    }
}

/**
 * A video page being looked up: yt-dlp answers only once it has every quality, so the bar runs
 * without a share; the seconds say it is alive, and a long wait is explained.
 */
@Composable
fun LookingUpStep(address: String, onCancel: () -> Unit) {
    var seconds by remember(address) { mutableIntStateOf(0) }
    LaunchedEffect(address) {
        while (true) {
            delay(1000)
            seconds++
        }
    }
    val site = remember(address) {
        runCatching { java.net.URI(address).host?.removePrefix("www.")?.removePrefix("m.") }.getOrNull() ?: address
    }
    Column(Modifier.fillMaxWidth().padding(vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.video_looking_up, site),
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.weight(1f),
            )
            Text(
                Format.ltr(stringResource(R.string.elapsed_seconds, seconds)),
                style = MaterialTheme.typography.bodyMedium.copy(fontFeatureSettings = "tnum"),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        LinearProgressIndicator(Modifier.fillMaxWidth())
        if (seconds >= 8) {
            Text(
                stringResource(R.string.video_looking_up_slow),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        OutlinedButton(onClick = onCancel, modifier = Modifier.align(Alignment.End)) { Text(stringResource(R.string.cancel)) }
    }
}

/** A video page's qualities: the pictures first, then the sound alone, each with its size. */
@Composable
fun VideoStep(
    lookup: VideoLookup,
    address: String,
    choice: Int,
    onChoice: (Int) -> Unit,
    onCancel: () -> Unit,
    onDownload: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        SheetTitle(lookup.title ?: address, stringResource(R.string.video_choose))
        lookup.notice?.text?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
                modifier = Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.secondaryContainer, RoundedCornerShape(10.dp)).padding(12.dp),
            )
        }
        val indexed = lookup.choices.withIndex().toList()
        val (sound, picture) = indexed.partition { it.value.audio }
        LazyColumn(Modifier.heightIn(max = 380.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (picture.isNotEmpty()) item { ChoiceHeading(Icons.Outlined.Movie, stringResource(R.string.video_video)) }
            itemsIndexed(picture, key = { _, c -> "v${c.index}" }) { _, c -> ChoiceRow(c.value, choice == c.index) { onChoice(c.index) } }
            if (sound.isNotEmpty()) item { ChoiceHeading(Icons.Outlined.GraphicEq, stringResource(R.string.video_audio)) }
            itemsIndexed(sound, key = { _, c -> "a${c.index}" }) { _, c -> ChoiceRow(c.value, choice == c.index) { onChoice(c.index) } }
        }
        SheetActions(stringResource(R.string.add_start), enabled = lookup.choices.isNotEmpty(), onCancel = onCancel, onAction = onDownload)
    }
}

@Composable
private fun ChoiceHeading(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String) {
    Row(Modifier.padding(top = 8.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.width(6.dp))
        Text(text.uppercase(), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** One quality: its height (or format) large, the rest of its label, its size. */
@Composable
private fun ChoiceRow(c: VideoChoice, chosen: Boolean, onClick: () -> Unit) {
    val col = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .background(if (chosen) col.primaryContainer else col.surfaceContainerLow)
            .clickable(role = Role.RadioButton, onClick = onClick)
            .heightIn(min = 56.dp)
            .padding(start = 4.dp, end = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = chosen, onClick = onClick)
        val badge = c.height?.let { if (it >= 2160) "4K" else "${it}p" } ?: c.ext.uppercase()
        Text(
            badge,
            style = MaterialTheme.typography.titleLarge,
            color = if (chosen) col.onPrimaryContainer else col.onSurface,
            modifier = Modifier.widthIn(min = 64.dp),
        )
        Text(
            c.label,
            style = MaterialTheme.typography.bodyMedium,
            color = if (chosen) col.onPrimaryContainer else col.onSurfaceVariant,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        c.size?.let {
            Spacer(Modifier.width(8.dp))
            Text(Format.size(it), style = FieldValue, color = if (chosen) col.onPrimaryContainer else col.onSurface)
        }
    }
}
