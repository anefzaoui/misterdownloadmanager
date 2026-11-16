package com.misterdownloadmanager.android.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.misterdownloadmanager.android.R
import com.misterdownloadmanager.android.core.Mdm
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/** Debrid accounts (#44): file hosts' links through your own Real-Debrid, AllDebrid… account. */
@Composable
fun DebridScreen(onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var info by remember { mutableStateOf<JsonObject?>(null) }
    var adding by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    suspend fun reload() {
        info = Mdm.request("debrid")
    }
    LaunchedEffect(Unit) { reload() }

    val services = (info?.get("services") as? JsonArray)?.map { it.jsonObject }.orEmpty()
    val accounts = (info?.get("accounts") as? JsonArray)?.map { it.jsonObject }.orEmpty()

    Column(Modifier.fillMaxSize().background(brand.board)) {
        Board(
            title = stringResource(R.string.debrid),
            navigation = { OnBoard { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } } },
            actions = { OnBoard { IconButton(onClick = { adding = true }) { Icon(Icons.Outlined.Add, stringResource(R.string.debrid_add)) } } },
        ) {
            Text(
                stringResource(R.string.debrid_text),
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
            Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding()) {
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(20.dp)) }
                if (accounts.isEmpty()) {
                    Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                        Text(stringResource(R.string.debrid_none), style = MaterialTheme.typography.headlineSmall)
                        Text(stringResource(R.string.debrid_none_text), color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Button(onClick = { adding = true }, modifier = Modifier.heightIn(min = 48.dp)) {
                            Icon(Icons.Outlined.Add, null, Modifier.size(18.dp))
                            Spacer(Modifier.width(8.dp))
                            Text(stringResource(R.string.debrid_add))
                        }
                    }
                } else {
                    SheetHeading(stringResource(R.string.debrid_accounts), accounts.size)
                }
                accounts.forEach { a ->
                    Row(
                        Modifier.fillMaxWidth().padding(start = 20.dp, end = 8.dp, top = 10.dp, bottom = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(
                            Modifier.size(44.dp).background(MaterialTheme.colorScheme.primaryContainer, RoundedCornerShape(10.dp)),
                            contentAlignment = Alignment.Center,
                        ) { Icon(Icons.Outlined.Key, null, tint = MaterialTheme.colorScheme.onPrimaryContainer) }
                        Spacer(Modifier.width(14.dp))
                        Column(Modifier.weight(1f)) {
                            Text(a["name"]?.jsonPrimitive?.content ?: "", style = MaterialTheme.typography.titleMedium)
                            val detail = listOf("user", "state").mapNotNull { a[it]?.jsonPrimitive?.content?.takeIf(String::isNotEmpty) }
                            Text(
                                detail.joinToString(" · ").ifEmpty { stringResource(R.string.debrid_checking) },
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        IconButton(onClick = {
                            scope.launch {
                                Mdm.request("debrid_remove") { put("id", JsonPrimitive(a["id"]?.jsonPrimitive?.longOrNull ?: 0)) }
                                reload()
                            }
                        }) { Icon(Icons.Outlined.Delete, stringResource(R.string.action_remove)) }
                    }
                    HorizontalDivider(Modifier.padding(start = 78.dp), color = MaterialTheme.colorScheme.outlineVariant)
                }
            }
        }
    }

    if (adding) {
        AddAccountSheet(services, onDone = { msg ->
            adding = false
            error = msg
            scope.launch { reload() }
        })
    }
}

/** Add an account: a service, then its sign-in by code in the browser, or its API token pasted. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AddAccountSheet(services: List<JsonObject>, onDone: (String?) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var service by remember { mutableStateOf(services.firstOrNull()) }
    var token by remember { mutableStateOf("") }
    var signin by remember { mutableStateOf<JsonObject?>(null) }
    val byCode = service?.get("by_code")?.jsonPrimitive?.booleanOrNull == true

    // While a sign-in runs: ask how it goes each second, until it's done or failed.
    LaunchedEffect(signin != null) {
        while (signin != null) {
            val s = Mdm.request("debrid_signin_state")
            signin = s
            when (s["state"]?.jsonPrimitive?.content) {
                "done" -> { onDone(null); return@LaunchedEffect }
                "failed" -> { onDone(s["message"]?.jsonPrimitive?.content); return@LaunchedEffect }
                "idle" -> return@LaunchedEffect
            }
            delay(1000)
        }
    }

    ModalBottomSheet(
        onDismissRequest = {
            scope.launch { Mdm.request("debrid_signin_cancel") }
            onDone(null)
        },
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = MaterialTheme.colorScheme.surface,
    ) {
        Column(
            Modifier.padding(horizontal = 24.dp).padding(bottom = 24.dp).navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            val s = signin
            val state = s?.get("state")?.jsonPrimitive?.content
            if (state == "waiting") {
                val code = s["code"]?.jsonPrimitive?.content ?: ""
                val page = s["page"]?.jsonPrimitive?.content ?: ""
                SheetTitle(service?.get("name")?.jsonPrimitive?.content ?: "", stringResource(R.string.debrid_code_text))
                Text(
                    code,
                    style = MaterialTheme.typography.displayMedium,
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(12.dp))
                        .clickable {
                            context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("code", code))
                        }
                        .padding(vertical = 18.dp, horizontal = 20.dp),
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(10.dp))
                    Text(
                        stringResource(R.string.debrid_waiting, Format.eta(s["seconds"]?.jsonPrimitive?.longOrNull ?: 0)),
                        style = MaterialTheme.typography.bodyMedium.merge(Numbers),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                SheetActions(stringResource(R.string.debrid_open_page), enabled = page.isNotEmpty(), download = false, onCancel = {
                    scope.launch { Mdm.request("debrid_signin_cancel") }
                    onDone(null)
                }) {
                    context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(page)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                }
                return@Column
            }
            if (state == "starting" || state == "checking") {
                WorkingStep(stringResource(R.string.working))
                return@Column
            }
            SheetTitle(stringResource(R.string.debrid_add))
            services.forEach { sv ->
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable { service = sv },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = service == sv, onClick = { service = sv })
                    Text(sv["name"]?.jsonPrimitive?.content ?: "", style = MaterialTheme.typography.bodyLarge)
                }
            }
            if (byCode) {
                Text(stringResource(R.string.debrid_by_code), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                SheetActions(stringResource(R.string.debrid_sign_in), enabled = true, download = false, onCancel = { onDone(null) }) {
                    scope.launch {
                        val r = Mdm.request("debrid_signin") { put("code", service!!["key"]!!) }
                        if (r.containsKey("error")) onDone(r["error"]?.jsonPrimitive?.content) else signin = JsonObject(mapOf("state" to JsonPrimitive("starting")))
                    }
                }
            } else {
                OutlinedTextField(
                    value = token,
                    onValueChange = { token = it.trim() },
                    label = { Text(stringResource(R.string.debrid_token)) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    trailingIcon = service?.get("token_page")?.jsonPrimitive?.content?.let { page ->
                        {
                            IconButton(onClick = {
                                context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(page)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                            }) { Icon(Icons.AutoMirrored.Outlined.OpenInNew, stringResource(R.string.debrid_get_token)) }
                        }
                    },
                    modifier = Modifier.fillMaxWidth(),
                )
                SheetActions(stringResource(R.string.debrid_add_short), enabled = token.isNotEmpty(), download = false, onCancel = { onDone(null) }) {
                    scope.launch {
                        val r = Mdm.request("debrid_add") {
                            put("code", service!!["key"]!!)
                            put("data", JsonPrimitive(token))
                        }
                        onDone(r["error"]?.jsonPrimitive?.content)
                    }
                }
            }
        }
    }
}
