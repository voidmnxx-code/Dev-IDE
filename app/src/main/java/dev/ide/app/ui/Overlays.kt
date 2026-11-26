package dev.ide.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.ide.app.Action
import dev.ide.app.FoldMode
import dev.ide.app.IdeViewModel

/** File tree: opened from the Files action; takes no permanent screen space (spec §5). */
@Composable
fun FilesDrawer(vm: IdeViewModel) {
    Box(Modifier.fillMaxSize().background(C.Scrim).clickable { vm.overlay = null }) {
        var expanded by remember { mutableStateOf(setOf<String>()) }
        Column(
            Modifier.fillMaxHeight().fillMaxWidth(0.82f).align(Alignment.CenterStart).statusBarsPadding()
                .background(C.Bar).clickable(enabled = false) {}.padding(12.dp),
        ) {
            Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Files", color = C.Text, fontWeight = FontWeight.Bold, fontSize = 18.sp,
                    modifier = Modifier.weight(1f))
                Chip("New project") { vm.overlay = IdeViewModel.Overlay.NewProject }
                Box(Modifier.size(6.dp))
                Chip("Packages") { vm.perform(dev.ide.app.Action.Packages) }
            }
            val rows = remember(vm.version, expanded) { flatten(vm, "", 0, expanded) }
            LazyColumn {
                items(rows) { (e, depth) ->
                    Row(
                        Modifier.fillMaxWidth().touchTarget().clickable {
                            if (e.isDir) expanded = if (e.path in expanded) expanded - e.path else expanded + e.path
                            else vm.openFile(e.path)
                        }.padding(start = (depth * 16).dp, end = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(if (e.isDir) (if (e.path in expanded) "v " else "> ") else "  ", color = C.Muted)
                        Text(e.name, color = if (e.isDir) C.Accent else C.Text, fontSize = 16.sp)
                    }
                }
            }
        }
    }
}

private fun flatten(vm: IdeViewModel, rel: String, depth: Int, open: Set<String>):
    List<Pair<uniffi.ide_core.FileEntry, Int>> =
    vm.listDir(rel).flatMap { e ->
        listOf(e to depth) + if (e.isDir && e.path in open) flatten(vm, e.path, depth + 1, open) else emptyList()
    }

@Composable
fun NewFileDialog(vm: IdeViewModel) {
    var name by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = { vm.overlay = null },
        title = { Text("New file") },
        text = {
            BasicTextField(
                value = name, onValueChange = { name = it }, singleLine = true,
                textStyle = Mono, cursorBrush = SolidColor(C.Accent),
                modifier = Modifier.fillMaxWidth().background(C.Panel).padding(12.dp),
                decorationBox = { inner ->
                    if (name.isEmpty()) Text("src/main.js", style = Mono.copy(color = C.Muted))
                    inner()
                },
            )
        },
        confirmButton = {
            TextButton(onClick = { if (name.isNotBlank()) { vm.createFile(name.trim()); vm.overlay = null } }) {
                Text("Create")
            }
        },
        dismissButton = { TextButton(onClick = { vm.overlay = null }) { Text("Cancel") } },
    )
}

/** Find (and replace) bar shown over the top of the editor. */
@Composable
fun FindBar(vm: IdeViewModel, modifier: Modifier = Modifier) {
    Column(modifier.fillMaxWidth().background(C.Bar).padding(8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Field(vm.findText, "Find", Modifier.weight(1f)) { vm.updateFind(it) }
            Text("${vm.findMatches.size / 2}", color = C.Muted)
            Chip("Next") { vm.findNext() }
            Chip("✕") { vm.overlay = null }
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Field(vm.replaceText, "Replace with", Modifier.weight(1f)) { vm.replaceText = it }
            Chip("Replace all") { vm.replaceAll() }
        }
    }
}

@Composable
internal fun Field(
    value: String,
    hint: String,
    modifier: Modifier,
    secret: Boolean = false,
    onChange: (String) -> Unit,
) {
    BasicTextField(
        value = value, onValueChange = onChange, singleLine = true,
        visualTransformation = if (secret) androidx.compose.ui.text.input.PasswordVisualTransformation()
        else androidx.compose.ui.text.input.VisualTransformation.None,
        textStyle = Mono, cursorBrush = SolidColor(C.Accent),
        modifier = modifier.heightIn44().clip(RoundedCornerShape(8.dp)).background(C.Panel).padding(horizontal = 10.dp),
        decorationBox = { inner ->
            Box(Modifier.fillMaxWidth().heightIn44(), contentAlignment = Alignment.CenterStart) {
                if (value.isEmpty()) Text(hint, style = Mono.copy(color = C.Muted))
                inner()
            }
        },
    )
}

private fun Modifier.heightIn44() = this.then(Modifier.touchTarget())

@Composable
internal fun Chip(label: String, onClick: () -> Unit) {
    Box(
        Modifier.touchTarget().clip(RoundedCornerShape(10.dp)).background(C.Key).clickable(onClick = onClick)
            .padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = C.Text, fontWeight = FontWeight.SemiBold) }
}

// ---------------------------------------------------------------------
// Customize actions (spec §11). Reordering uses up/down buttons rather than drag.
// ---------------------------------------------------------------------

@Composable
fun CustomizeScreen(vm: IdeViewModel) {
    val s = vm.settings
    var l1 by remember { mutableStateOf(s.layer1) }
    var l2 by remember { mutableStateOf(s.layer2) }
    var longPress by remember { mutableStateOf(s.longPressAction) }
    val available = Action.entries.filter { it !in l1 && it !in l2 }

    fun commit() { s.layer1 = l1; s.layer2 = l2; s.longPressAction = longPress }

    Column(Modifier.fillMaxSize().background(C.Bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Header("Customize actions") { commit(); vm.overlay = null }
        Section("LAYER 1 · UP TO 4")
        l1.forEachIndexed { i, a ->
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp).clip(RoundedCornerShape(14.dp))
                    .background(C.Bar).touchTarget().padding(horizontal = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(a.label, color = C.Text, fontSize = 16.sp, modifier = Modifier.weight(1f))
                if (a == longPress) Text("Long-press ^  ", color = C.Muted, fontSize = 13.sp)
                SmallBtn("↑", enabled = i > 0) { l1 = l1.toMutableList().also { it.add(i - 1, it.removeAt(i)) } }
                SmallBtn("↓", enabled = i < l1.lastIndex) { l1 = l1.toMutableList().also { it.add(i + 1, it.removeAt(i)) } }
                // "More" must stay reachable, otherwise layer 2 can't be opened.
                SmallBtn("−", enabled = a != Action.More) { l1 = l1 - a; l2 = l2 + a }
            }
        }
        Section("LAYER 2 (tap to move up)")
        ChipFlow(l2.map { it.label }) { idx ->
            val a = l2[idx]
            if (l1.size < 4) { l2 = l2 - a; l1 = l1 + a }
        }
        Section("AVAILABLE (tap to add)")
        ChipFlow(available.map { "+ ${it.label}" }) { idx -> l2 = l2 + available[idx] }
        Section("LONG-PRESS ^ RUNS")
        ChipFlow((l1 + l2).filter { it != Action.More }.map { (if (it == longPress) "● " else "") + it.label }) { idx ->
            longPress = (l1 + l2).filter { it != Action.More }[idx]
        }
        Section("SETTINGS")
        Row(
            Modifier.fillMaxWidth().touchTarget().clickable { commit(); vm.overlay = IdeViewModel.Overlay.AutoFold }
                .padding(horizontal = 24.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) { Text("Auto-fold ›", color = C.Accent, fontSize = 16.sp) }
    }
}

@Composable
private fun ChipFlow(labels: List<String>, onTap: (Int) -> Unit) {
    // Simple wrapping layout without experimental FlowRow.
    val rows = labels.withIndex().chunked(3)
    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        rows.forEach { r ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                r.forEach { (i, l) -> Chip(l) { onTap(i) } }
            }
        }
    }
}

@Composable
private fun SmallBtn(label: String, enabled: Boolean = true, onClick: () -> Unit) {
    Box(
        Modifier.touchTarget().clickable(enabled = enabled, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = if (enabled) C.Accent else C.Muted.copy(alpha = 0.3f), fontSize = 20.sp) }
}

// ---------------------------------------------------------------------
// Auto-fold settings (spec §11)
// ---------------------------------------------------------------------

@Composable
fun AutoFoldScreen(vm: IdeViewModel) {
    val s = vm.settings
    var mode by remember { mutableStateOf(s.foldMode) }
    var keepTabs by remember { mutableStateOf(s.keepTabsInPortrait) }
    var redDot by remember { mutableStateOf(s.redDotOnFold) }
    var peek by remember { mutableStateOf(s.tapFoldToPeek) }
    var cursorFold by remember { mutableStateOf(s.foldInCursorMode) }
    var closePeek by remember { mutableStateOf(s.closePeekWhenTyping) }

    Column(Modifier.fillMaxSize().background(C.Bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Header("Auto-fold") { vm.overlay = IdeViewModel.Overlay.Customize }
        Section("FOLD WHEN TYPING")
        Radio("Always", "Portrait and landscape", mode == FoldMode.Always) { mode = FoldMode.Always; s.foldMode = mode }
        Radio("Landscape only", "Portrait never folds", mode == FoldMode.LandscapeOnly) {
            mode = FoldMode.LandscapeOnly; s.foldMode = mode
        }
        Radio("Off", "Layout never changes", mode == FoldMode.Off) { mode = FoldMode.Off; s.foldMode = mode }
        Section("BEHAVIOR")
        Check("Keep file tabs in portrait", keepTabs) { keepTabs = it; s.keepTabsInPortrait = it }
        Check("Red dot on folded line", redDot) { redDot = it; s.redDotOnFold = it }
        Check("Tap folded line to peek", peek) { peek = it; s.tapFoldToPeek = it }
        Check("Fold in cursor mode", cursorFold) { cursorFold = it; s.foldInCursorMode = it }
        Check("Close peek when typing", closePeek) { closePeek = it; s.closePeekWhenTyping = it }
    }
}

@Composable
internal fun Header(title: String, onBack: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().height44().clickable(onClick = onBack).padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("‹  ", color = C.Text, fontSize = 24.sp)
        Text(title, color = C.Text, fontSize = 20.sp, fontWeight = FontWeight.Bold)
    }
}

private fun Modifier.height44() = this.then(Modifier.touchTarget())

@Composable
internal fun Section(t: String) =
    Text(t, color = C.Muted, fontSize = 13.sp, fontWeight = FontWeight.Bold,
        modifier = Modifier.padding(start = 16.dp, top = 20.dp, bottom = 8.dp))

@Composable
private fun Radio(title: String, sub: String, selected: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp).clip(RoundedCornerShape(14.dp))
            .background(C.Bar).clickable(onClick = onClick).padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, color = C.Text, fontSize = 17.sp)
            Text(sub, color = C.Muted, fontSize = 13.sp)
        }
        Box(
            Modifier.size(24.dp).clip(CircleShape).background(if (selected) C.Accent else C.Text),
            contentAlignment = Alignment.Center,
        ) { if (selected) Box(Modifier.size(10.dp).clip(CircleShape).background(C.Bg)) }
    }
}

@Composable
private fun Check(title: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp).clip(RoundedCornerShape(14.dp))
            .background(C.Bar).clickable { onChange(!checked) }.padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, color = C.Text, fontSize = 17.sp, modifier = Modifier.weight(1f))
        Box(
            Modifier.size(24.dp).clip(RoundedCornerShape(6.dp)).background(if (checked) C.Accent else C.Key),
            contentAlignment = Alignment.Center,
        ) { if (checked) Text("✓", color = C.Bg, fontWeight = FontWeight.Bold) }
    }
}
