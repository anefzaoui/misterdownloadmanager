package com.misterdownloadmanager.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import com.misterdownloadmanager.android.R

/**
 * Asks before downloads leave the list, the way the desktop's Delete does: the list only, or
 * with their files too (unticked by default). [onDelete] gets the choice.
 */
@Composable
fun DeleteDialog(count: Int, onDismiss: () -> Unit, onDelete: (withFiles: Boolean) -> Unit) {
    var withFiles by remember { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.Delete, null) },
        title = { Text(pluralStringResource(R.plurals.delete_title, count, count)) },
        text = {
            Row(
                Modifier.fillMaxWidth().clickable { withFiles = !withFiles },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Checkbox(checked = withFiles, onCheckedChange = { withFiles = it })
                Text(stringResource(R.string.delete_files_too))
            }
        },
        confirmButton = { TextButton(onClick = { onDelete(withFiles) }) { Text(stringResource(R.string.action_delete)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
