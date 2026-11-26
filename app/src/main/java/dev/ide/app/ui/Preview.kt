package dev.ide.app.ui

import android.annotation.SuppressLint
import android.webkit.ConsoleMessage
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import dev.ide.app.IdeViewModel
import dev.ide.app.LineKind

/**
 * In-app preview of a web project served by the local server. Android WebView can never be
 * cross-origin isolated, so projects needing SharedArrayBuffer are opened in Chrome instead
 * (decided by the Rust `analyze_preview`, see IdeViewModel.preview()).
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun PreviewScreen(vm: IdeViewModel) {
    val url = vm.previewUrl
    var web by remember { mutableStateOf<WebView?>(null) }
    var console by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize().background(C.Bg).statusBarsPadding()) {
        Header("Preview") { vm.overlay = null } // the server keeps running in the background
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Chip("Reload") { web?.reload() }
            Chip(if (console) "Hide console" else "Console (${vm.webConsole.size})") { console = !console }
            Chip("Open in Chrome") { vm.openExternal = url }
            Chip("Stop server") { vm.stopPreview() }
        }
        Text(url ?: "", style = Mono.copy(color = C.Muted, fontSize = 12.sp), modifier = Modifier.padding(horizontal = 16.dp))

        Box(Modifier.weight(1f).fillMaxWidth().padding(top = 6.dp)) {
            if (url != null) {
                AndroidView(
                    modifier = Modifier.fillMaxSize(),
                    factory = { ctx ->
                        WebView(ctx).apply {
                            settings.javaScriptEnabled = true
                            settings.domStorageEnabled = true
                            settings.mediaPlaybackRequiresUserGesture = false // web games with audio
                            webChromeClient = object : WebChromeClient() {
                                override fun onConsoleMessage(m: ConsoleMessage): Boolean {
                                    val src = m.sourceId().orEmpty().substringAfterLast('/')
                                    vm.logWeb(m.messageLevel().name, "${m.message()}  ($src:${m.lineNumber()})")
                                    return true
                                }
                            }
                            webViewClient = object : WebViewClient() {
                                override fun onReceivedError(v: WebView, r: WebResourceRequest, e: WebResourceError) {
                                    if (r.isForMainFrame) vm.logWeb("ERROR", "Page failed to load: ${e.description}")
                                }
                            }
                            loadUrl(url)
                            web = this
                        }
                    },
                    onRelease = { it.destroy() },
                )
            }
        }

        if (console) {
            Column(Modifier.fillMaxWidth().height(200.dp).background(C.Panel).padding(8.dp)) {
                Text("JavaScript console", color = C.Muted, fontSize = 12.sp, fontWeight = FontWeight.Bold)
                LazyColumn(Modifier.weight(1f)) {
                    items(vm.webConsole.toList()) { l ->
                        Text(
                            l.text,
                            style = Mono.copy(fontSize = 12.sp, color = if (l.kind == LineKind.Error) C.Error else C.Ok),
                        )
                    }
                }
            }
        }
    }
}

/** Pick a template (web / Macroquad / Bevy) and a name. */
@Composable
fun NewProjectScreen(vm: IdeViewModel) {
    val templates = remember { vm.templates() }
    var selected by remember { mutableStateOf(templates.firstOrNull()?.id ?: "") }
    var name by remember { mutableStateOf("") }

    Column(Modifier.fillMaxSize().background(C.Bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Header("New project") { vm.overlay = null }
        Section("TEMPLATE")
        templates.forEach { t ->
            val on = t.id == selected
            Column(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)
                    .clip(RoundedCornerShape(14.dp))
                    .background(if (on) C.CursorKey else C.Bar)
                    .clickable { selected = t.id }
                    .padding(14.dp),
            ) {
                Text(
                    (if (on) "● " else "○ ") + t.name, color = C.Text, fontWeight = FontWeight.SemiBold, fontSize = 17.sp,
                )
                Text(t.description, color = C.Muted, fontSize = 13.sp)
            }
        }
        Section("NAME")
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Field(name, "my-project", Modifier.fillMaxWidth()) { name = it }
            Chip("Create project") { if (name.isNotBlank() && selected.isNotEmpty()) vm.createProject(selected, name.trim()) }
        }
    }
}

/** Problems tab: parsed compiler errors; tapping one jumps to the line. */
@Composable
fun ProblemsView(vm: IdeViewModel) {
    val d = vm.diagnostics
    if (d.isEmpty()) {
        val errs = vm.output.filter { it.kind == LineKind.Error }
        if (errs.isEmpty()) {
            Text("No problems", color = C.Muted, modifier = Modifier.padding(top = 12.dp))
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(errs) { Text(it.text, style = Mono.copy(color = C.Error)) }
            }
        }
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(d) { x ->
            val isErr = x.severity == uniffi.ide_core.Severity.ERROR
            Column(
                Modifier.fillMaxWidth().touchTarget()
                    .clickable { vm.gotoLine(x.path, x.line.toInt(), x.column.toInt()) }
                    .padding(vertical = 4.dp),
            ) {
                // Text label + colour (never colour alone, spec §13).
                Text(
                    (if (isErr) "error  " else "warning  ") + x.message,
                    style = Mono.copy(fontSize = 13.sp, color = if (isErr) C.Error else C.Accent),
                )
                Text("${x.path}:${x.line}:${x.column}", style = Mono.copy(fontSize = 12.sp, color = C.Muted))
            }
        }
    }
}
