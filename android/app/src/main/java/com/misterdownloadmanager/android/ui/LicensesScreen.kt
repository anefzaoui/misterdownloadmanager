package com.misterdownloadmanager.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.misterdownloadmanager.android.R

/** The licence files the build puts in the app (assets/licenses), in the order they are shown. */
private val LICENCES = listOf(
    "NOTICE.txt" to null,
    "GPL-3.0.txt" to "Mister Download Manager — GPL-3.0",
    "7-Zip.txt" to "7-Zip",
    "Archivo-OFL.txt" to "Archivo — SIL Open Font License 1.1",
)

/**
 * Open-source licences: what the app carries by others, under which licence, and where its
 * source is (assets/licenses/NOTICE.txt), then the full texts the licences ask to go with it.
 */
@Composable
fun LicensesScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val texts = remember {
        LICENCES.mapNotNull { (file, title) ->
            runCatching { context.assets.open("licenses/$file").bufferedReader().use { it.readText() } }
                .getOrNull()?.let { title to it }
        }
    }
    Column(Modifier.fillMaxSize().background(brand.board)) {
        Board(
            title = stringResource(R.string.set_licenses),
            navigation = { OnBoard { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } } },
        )
        Surface(color = MaterialTheme.colorScheme.surface, shape = RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp), modifier = Modifier.fillMaxSize()) {
            SelectionContainer {
                Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().padding(bottom = 24.dp)) {
                    texts.forEach { (title, text) ->
                        title?.let { SheetHeading(it) }
                        Text(
                            text.trim(),
                            style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                            modifier = Modifier.padding(horizontal = 20.dp, vertical = 12.dp),
                        )
                    }
                }
            }
        }
    }
}
