package com.misterdownloadmanager.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.InsertDriveFile
import androidx.compose.material.icons.outlined.Apps
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.FolderZip
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material.icons.outlined.MusicNote
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.misterdownloadmanager.android.core.Download

/**
 * The board: the logo's blue under a screen's title, with its actions and whatever sits under the
 * title (fields, filters). Draws behind the status bar (edge to edge) and pads its content below it.
 */
@Composable
fun Board(
    title: String,
    modifier: Modifier = Modifier,
    navigation: (@Composable () -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
    below: @Composable ColumnScope.() -> Unit = {},
) {
    val b = brand
    Surface(color = b.board, contentColor = b.onBoard, modifier = modifier.fillMaxWidth()) {
        Column(Modifier.windowInsetsPadding(WindowInsets.statusBars)) {
            Row(
                Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = if (navigation == null) 20.dp else 4.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                navigation?.invoke()
                Text(
                    title,
                    style = MaterialTheme.typography.headlineMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f).padding(start = if (navigation == null) 0.dp else 4.dp),
                )
                actions()
            }
            below()
        }
    }
}

/** A field as the waybill prints it: its name in condensed caps, its value under it. */
@Composable
fun Field(
    name: String,
    value: String,
    modifier: Modifier = Modifier,
    nameColor: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    valueColor: Color = LocalContentColor.current,
    maxLines: Int = 1,
) {
    Column(modifier) {
        Text(name.uppercase(), style = MaterialTheme.typography.labelSmall, color = nameColor, maxLines = 1)
        Spacer(Modifier.height(2.dp))
        Text(value, style = FieldValue, color = valueColor, maxLines = maxLines, overflow = TextOverflow.Ellipsis)
    }
}

/** The kinds of tag: what a row's state is, at a glance. */
enum class TagKind { Neutral, Info, Error, Done }

/** A small label in condensed caps: a download's state (Paused, Queued, Failed, Done). */
@Composable
fun Tag(text: String, kind: TagKind = TagKind.Neutral) {
    val c = MaterialTheme.colorScheme
    val (fg, bg) = when (kind) {
        TagKind.Neutral -> c.onSurfaceVariant to Color.Transparent
        TagKind.Info -> c.primary to Color.Transparent
        TagKind.Error -> c.error to Color.Transparent
        TagKind.Done -> brand.stamp to Color.Transparent
    }
    Box(
        Modifier
            .background(bg, RoundedCornerShape(4.dp))
            .border(1.25.dp, fg.copy(alpha = 0.7f), RoundedCornerShape(4.dp))
            .padding(horizontal = 6.dp, vertical = 1.dp),
    ) {
        Text(text.uppercase(), style = MaterialTheme.typography.labelSmall, color = fg, maxLines = 1)
    }
}

/** A heading over a group of rows on a sheet: condensed caps, with its count. */
@Composable
fun SheetHeading(text: String, count: Int? = null, modifier: Modifier = Modifier) {
    Row(
        modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text.uppercase(), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (count != null) {
            Text(
                count.toString(),
                style = MaterialTheme.typography.labelMedium.merge(Numbers),
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
            )
        }
    }
}

/** The icon of a download's kind or category. */
fun iconFor(d: Download): ImageVector = when {
    d.kind == "torrent" -> Icons.Outlined.Hub
    d.kind == "video" -> Icons.Outlined.Movie
    d.kind == "gallery" -> Icons.Outlined.Image
    else -> iconForCategory(d.category)
}

fun iconForCategory(category: String): ImageVector = when (category) {
    "Video" -> Icons.Outlined.Movie
    "Music" -> Icons.Outlined.MusicNote
    "Compressed" -> Icons.Outlined.FolderZip
    "Documents" -> Icons.Outlined.Description
    "Programs" -> Icons.Outlined.Apps
    "Torrents" -> Icons.Outlined.Hub
    else -> Icons.AutoMirrored.Outlined.InsertDriveFile
}

/** A download's icon on a tile tinted by its state: green once it has arrived, red when it failed. */
@Composable
fun FileTile(d: Download, size: Dp = 44.dp) {
    val c = MaterialTheme.colorScheme
    val (bg, fg) = when {
        d.complete -> c.tertiaryContainer to c.onTertiaryContainer
        d.failed -> c.errorContainer to c.onErrorContainer
        d.running -> c.primaryContainer to c.onPrimaryContainer
        else -> c.surfaceContainerHigh to c.onSurfaceVariant
    }
    Box(Modifier.size(size).background(bg, RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
        Icon(iconFor(d), contentDescription = null, tint = fg, modifier = Modifier.size(size * 0.5f))
    }
}

/** Content colour on the board for nested composables (icons, buttons). */
@Composable
fun OnBoard(content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalContentColor provides brand.onBoard, content = content)
}
