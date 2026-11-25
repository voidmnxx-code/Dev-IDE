package dev.ide.app.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.ide.app.IdeViewModel
import dev.ide.app.KeyboardMode
import uniffi.ide_core.CursorMove
import uniffi.ide_core.SelectUnit

/** One key on the extra row. [alts] are shown on long-press. */
private data class Key(
    val label: String,
    val alts: List<String> = emptyList(),
    val onTap: (IdeViewModel) -> Unit,
)

private fun ch(c: String, alts: List<String> = emptyList()) = Key(c, alts) { it.typeChar(c) }
private fun pair(o: String, c: String, alts: List<String> = emptyList()) =
    Key("$o $c", alts) { it.insertPair(o, c) }

private val codeKeys = listOf(
    Key("Tab") { it.insert("  ") },
    pair("(", ")", listOf("()", "(\"\")")),
    pair("{", "}", listOf("{}", "{\n}")),
    pair("[", "]"),
    ch(";"), ch(":"), ch("\""), ch("="), ch("<"), ch(">"), ch("/"), ch("_"),
    Key("←") { it.moveCursor(CursorMove.LEFT) },
    Key("↑") { it.moveCursor(CursorMove.UP) },
    Key("↓") { it.moveCursor(CursorMove.DOWN) },
    Key("→") { it.moveCursor(CursorMove.RIGHT) },
)

private fun terminalKeys(ctrl: Boolean) = listOf(
    Key(if (ctrl) "Ctrl ●" else "Ctrl") { it.ctrlActive = !it.ctrlActive },
    Key("Esc") { it.termRaw("\u001b") },
    Key("Tab") { it.termRaw("\t") },
    Key("↑") { it.termRaw("\u001b[A") },
    Key("↓") { it.termRaw("\u001b[B") },
    Key("←") { it.termRaw("\u001b[D") },
    Key("→") { it.termRaw("\u001b[C") },
    Key("|") { it.termType("|") },
    Key("~") { it.termType("~") },
    Key("/") { it.termType("/") },
    Key("-") { it.termType("-") },
    Key("Ctrl+C") { it.termRaw("\u0003") },
)

/** Spec §7: 44dp row above the system keyboard; first key opens the cursor editor. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ExtraKeysRow(vm: IdeViewModel, terminal: Boolean) {
    val keys = if (terminal) terminalKeys(vm.ctrlActive) else codeKeys
    LazyRow(
        Modifier.fillMaxWidth().height(MinTouch).background(C.Bar),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 6.dp, vertical = 3.dp),
    ) {
        item {
            Box(
                Modifier.width(MinTouch).height(38.dp).clip(RoundedCornerShape(8.dp)).background(C.CursorKey)
                    .semantics { contentDescription = "Cursor editor" }
                    .combinedClickable { vm.keyboardMode = KeyboardMode.Cursor },
                contentAlignment = Alignment.Center,
            ) { Text("⌶", color = C.Text, fontSize = 18.sp) }
        }
        items(keys) { k ->
            var menu by remember { mutableStateOf(false) }
            Box {
                Box(
                    Modifier.widthIn(min = MinTouch).height(38.dp).clip(RoundedCornerShape(8.dp)).background(C.Key)
                        .combinedClickable(
                            onClick = { k.onTap(vm) },
                            onLongClick = { if (k.alts.isNotEmpty()) menu = true },
                        )
                        .padding(horizontal = 12.dp),
                    contentAlignment = Alignment.Center,
                ) { Text(k.label, style = Mono.copy(fontSize = 16.sp, fontWeight = FontWeight.Bold)) }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    k.alts.forEach { alt ->
                        DropdownMenuItem(
                            text = { Text(alt.replace("\n", "⏎"), style = Mono) },
                            onClick = { vm.insert(alt); menu = false },
                        )
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// Cursor editor (spec §8): replaces the system keyboard, ~334dp tall.
// ---------------------------------------------------------------------

@Composable
fun CursorEditor(vm: IdeViewModel, compact: Boolean = false) {
    val rowH = if (compact) 36.dp else 48.dp
    val clipboard = LocalClipboardManager.current
    val density = LocalDensity.current
    val stepX = with(density) { 12.dp.toPx() }
    val stepY = with(density) { 26.dp.toPx() }

    Column(
        Modifier.fillMaxWidth().height(if (compact) 220.dp else 334.dp).background(C.KeyboardBg).padding(if (compact) 4.dp else 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(Modifier.fillMaxWidth().height(rowH), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PadButton(
                if (vm.selectMode) "Select ON" else "Select",
                Modifier.weight(1f),
                active = vm.selectMode,
            ) { vm.selectMode = !vm.selectMode; if (!vm.selectMode) vm.clearSelection() }
            PadButton("Word", Modifier.weight(1f)) { vm.select(SelectUnit.WORD) }
            PadButton("Line", Modifier.weight(1f)) { vm.select(SelectUnit.LINE) }
            PadButton("Doc", Modifier.weight(1f)) { vm.select(SelectUnit.DOCUMENT) }
        }
        Row(Modifier.fillMaxWidth().weight(1f), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            // Trackpad: drag moves the caret; with Select ON it extends the selection.
            var accX by remember { mutableStateOf(0f) }
            var accY by remember { mutableStateOf(0f) }
            Box(
                Modifier.weight(1.3f).fillMaxSize().clip(RoundedCornerShape(12.dp)).background(C.Panel)
                    .semantics { contentDescription = "Trackpad: drag to move the cursor" }
                    .pointerInput(vm.selectMode) {
                        detectDragGestures { change, drag ->
                            change.consume()
                            accX += drag.x; accY += drag.y
                            val dx = (accX / stepX).toInt()
                            val dy = (accY / stepY).toInt()
                            if (dx != 0 || dy != 0) {
                                vm.nudge(dx, dy)
                                accX -= dx * stepX; accY -= dy * stepY
                            }
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    "Drag here to move the cursor\nWith Select ON: drag to select",
                    color = C.Muted, fontSize = 13.sp, textAlign = TextAlign.Center,
                    modifier = Modifier.padding(12.dp),
                )
            }
            Column(Modifier.weight(1f).fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Row(Modifier.weight(1f).fillMaxWidth(), horizontalArrangement = Arrangement.Center) {
                    PadButton("↑", Modifier.width(60.dp).fillMaxSize()) { vm.moveCursor(CursorMove.UP) }
                }
                Row(Modifier.weight(1f).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    PadButton("←", Modifier.weight(1f).fillMaxSize()) { vm.moveCursor(CursorMove.LEFT) }
                    PadButton("Esc", Modifier.weight(1f).fillMaxSize()) {
                        vm.selectMode = false; vm.clearSelection()
                    }
                    PadButton("→", Modifier.weight(1f).fillMaxSize()) { vm.moveCursor(CursorMove.RIGHT) }
                }
                Row(Modifier.weight(1f).fillMaxWidth(), horizontalArrangement = Arrangement.Center) {
                    PadButton("↓", Modifier.width(60.dp).fillMaxSize()) { vm.moveCursor(CursorMove.DOWN) }
                }
            }
        }
        Row(Modifier.fillMaxWidth().height(rowH), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PadButton("Cut", Modifier.weight(1f)) { clipboard.setText(AnnotatedString(vm.cut())) }
            PadButton("Copy", Modifier.weight(1f)) { clipboard.setText(AnnotatedString(vm.selectedText())) }
            PadButton("Paste", Modifier.weight(1f)) { clipboard.getText()?.text?.let { vm.insert(it) } }
            PadButton("All", Modifier.weight(1f)) { vm.select(SelectUnit.DOCUMENT) }
        }
        PadButton("ABC · Back to keys", Modifier.fillMaxWidth().height(rowH + 4.dp), primary = true) {
            vm.keyboardMode = KeyboardMode.System
            vm.selectMode = false
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PadButton(
    label: String,
    modifier: Modifier,
    active: Boolean = false,
    primary: Boolean = false,
    onClick: () -> Unit,
) {
    val bg = when {
        active -> C.Accent
        primary -> C.CursorKey
        else -> C.Key
    }
    Box(
        modifier.clip(RoundedCornerShape(12.dp)).background(bg)
            .combinedClickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            color = if (active) C.Bg else C.Text,
            fontWeight = FontWeight.SemiBold,
            fontSize = 16.sp,
        )
    }
}
