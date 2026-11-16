package com.misterdownloadmanager.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.ViewTimeline
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.ViewTimeline
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.LayoutDirection
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.core.Snapshot

/** The app's three places. */
enum class Tab(val label: Int, val on: ImageVector, val off: ImageVector) {
    Downloads(R.string.nav_downloads, Icons.Filled.Download, Icons.Outlined.Download),
    Queues(R.string.nav_queues, Icons.Filled.ViewTimeline, Icons.Outlined.ViewTimeline),
    Settings(R.string.nav_settings, Icons.Filled.Settings, Icons.Outlined.Settings),
}

@Composable
fun App(
    incoming: List<String>?,
    onIncomingTaken: () -> Unit,
    showDownload: Long?,
    onShown: () -> Unit,
    clipboardLink: () -> String?,
    incomingTorrent: ByteArray? = null,
    onTorrentTaken: () -> Unit = {},
) {
    val snapshot by Mdm.snapshot.collectAsStateWithLifecycle()
    val startError by Mdm.startError.collectAsStateWithLifecycle()
    var addLinks by remember { mutableStateOf<List<String>?>(null) }
    var addTorrent by remember { mutableStateOf<ByteArray?>(null) }
    var detailsId by rememberSaveable { mutableStateOf<Long?>(null) }

    LaunchedEffect(incomingTorrent) {
        if (incomingTorrent != null) {
            addTorrent = incomingTorrent
            onTorrentTaken()
        }
    }
    LaunchedEffect(incoming) {
        if (incoming != null) {
            addLinks = incoming
            onIncomingTaken()
        }
    }
    LaunchedEffect(showDownload) {
        if (showDownload != null) {
            detailsId = showDownload
            onShown()
        }
    }

    AppFrame(
        snapshot = snapshot,
        startError = startError,
        detailsId = detailsId,
        onDetails = { detailsId = it },
        onAdd = { addLinks = listOfNotNull(clipboardLink()) },
    )
    addLinks?.let { links ->
        AddSheet(initial = links, snapshot = snapshot, onDismiss = { addLinks = null })
    }
    addTorrent?.let { data ->
        AddSheet(initial = emptyList(), snapshot = snapshot, onDismiss = { addTorrent = null }, torrentData = data)
    }
}

/** The frame: a navigation bar (a rail on wide screens), the place shown, a download's details over it. */
@Composable
fun AppFrame(
    snapshot: Snapshot?,
    startError: String?,
    detailsId: Long?,
    onDetails: (Long?) -> Unit,
    onAdd: () -> Unit,
    initialTab: Tab = Tab.Downloads,
    preview: Boolean = false,
) {
    var tab by rememberSaveable { mutableStateOf(initialTab) }
    var debrid by rememberSaveable { mutableStateOf(false) }
    var licences by rememberSaveable { mutableStateOf(false) }
    val wide = LocalConfiguration.current.screenWidthDp >= 600
    // The interface language's direction (Arabic, Hebrew…), as the core says it.
    val direction = if (snapshot?.rtl == true) LayoutDirection.Rtl else LocalLayoutDirection.current

    CompositionLocalProvider(LocalLayoutDirection provides direction) {
        if (detailsId != null) {
            BackHandler { onDetails(null) }
            DetailsScreen(id = detailsId, snapshot = snapshot, onBack = { onDetails(null) }, preview = preview)
            return@CompositionLocalProvider
        }
        if (debrid) {
            BackHandler { debrid = false }
            DebridScreen(onBack = { debrid = false })
            return@CompositionLocalProvider
        }
        if (licences) {
            BackHandler { licences = false }
            LicensesScreen(onBack = { licences = false })
            return@CompositionLocalProvider
        }
        if (tab != Tab.Downloads) BackHandler { tab = Tab.Downloads }
        Scaffold(
            contentWindowInsets = WindowInsets(0),
            bottomBar = {
                if (!wide) {
                    NavigationBar(containerColor = MaterialTheme.colorScheme.surfaceContainer) {
                        Tab.entries.forEach { t ->
                            NavigationBarItem(
                                selected = tab == t,
                                onClick = { tab = t },
                                icon = { Icon(if (tab == t) t.on else t.off, null) },
                                label = { Text(stringResource(t.label)) },
                                colors = NavigationBarItemDefaults.colors(
                                    indicatorColor = MaterialTheme.colorScheme.primaryContainer,
                                    selectedIconColor = MaterialTheme.colorScheme.onPrimaryContainer,
                                ),
                            )
                        }
                    }
                }
            },
            floatingActionButton = {
                if (tab == Tab.Downloads && (snapshot?.downloads?.isNotEmpty() == true)) {
                    ExtendedFloatingActionButton(
                        onClick = onAdd,
                        icon = { Icon(Icons.Filled.Add, null) },
                        text = { Text(stringResource(R.string.action_add_short)) },
                        containerColor = brand.go,
                        contentColor = brand.onGo,
                    )
                }
            },
        ) { padding ->
            Row(Modifier.fillMaxSize().padding(bottom = padding.calculateBottomPadding())) {
                if (wide) {
                    NavigationRail(containerColor = MaterialTheme.colorScheme.surfaceContainer) {
                        Tab.entries.forEach { t ->
                            NavigationRailItem(
                                selected = tab == t,
                                onClick = { tab = t },
                                icon = { Icon(if (tab == t) t.on else t.off, null) },
                                label = { Text(stringResource(t.label)) },
                            )
                        }
                    }
                }
                Box(Modifier.weight(1f)) {
                    AnimatedContent(tab, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "tab") { t ->
                        when (t) {
                            Tab.Downloads -> ListScreen(snapshot, startError, onOpen = { onDetails(it) }, onAdd = onAdd)
                            Tab.Queues -> QueuesScreen(snapshot, preview = preview)
                            Tab.Settings -> SettingsScreen(onDebrid = { debrid = true }, onLicenses = { licences = true }, preview = preview)
                        }
                    }
                }
            }
        }
    }
}
