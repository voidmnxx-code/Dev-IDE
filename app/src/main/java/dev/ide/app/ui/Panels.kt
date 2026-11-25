package dev.ide.app.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.ide.app.IdeViewModel
import dev.ide.app.LineKind
import dev.ide.app.OutputTab
import dev.ide.app.PanelSnap

/** Output / Terminal / Problems / Debug / Logs with a drag handle (spec §4). */
@Composable
fun OutputPanel(
    vm: IdeViewModel,
    modifier: Modifier = Modifier,
    onDragDelta: (Float) -> Unit = {},
    onDragEnd: () -> Unit = {},
    vertical: Boolean = true,
) {
    Column(modifier.background(C.Panel)) {
        PanelTabs(vm)
        Box(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 12.dp)) {
            when (vm.outputTab) {
                OutputTab.Output -> LinesView(vm.output.toList())
                OutputTab.Terminal -> TerminalView(vm)
                OutputTab.Problems -> LinesView(vm.output.filter { it.kind == LineKind.Error })
                OutputTab.Debug -> Placeholder("Debugger (DAP) arrives in a later phase")
                OutputTab.Logs -> Placeholder("logcat viewer arrives in a later phase")
            }
        }
        if (vertical) DragHandle(onDragDelta, onDragEnd)
    }
}

@Composable
private fun PanelTabs(vm: IdeViewModel) {
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        OutputTab.entries.forEach { t ->
            val label = if (t == OutputTab.Problems && vm.problemCount > 0) "Problems ${vm.problemCount}" else t.label
            val selected = vm.outputTab == t
            Column(
                Modifier.touchTarget().clickable { vm.outputTab = t }.padding(horizontal = 8.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Text(
                    label,
                    color = if (selected) C.Text else C.Muted,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 15.sp,
                )
                Box(Modifier.height(2.dp).fillMaxWidth().background(if (selected) C.Accent else Color.Transparent))
            }
        }
    }
}

@Composable
private fun LinesView(lines: List<dev.ide.app.OutLine>) {
    val state = rememberLazyListState()
    LaunchedEffect(lines.size) { if (lines.isNotEmpty()) state.scrollToItem(lines.lastIndex) }
    LazyColumn(state = state, modifier = Modifier.fillMaxSize()) {
        items(lines) { l ->
            // Errors carry text AND colour (spec §13: never colour alone).
            Text(
                l.text,
                style = Mono.copy(
                    color = when (l.kind) {
                        LineKind.Command, LineKind.Ok -> C.Ok
                        LineKind.Error -> C.Error
                        LineKind.Info -> C.Muted
                    },
                ),
            )
        }
    }
}

@Composable
private fun Placeholder(msg: String) =
    Text(msg, color = C.Muted, modifier = Modifier.padding(top = 12.dp))

private const val TERM_SENTINEL = "\u200B\u200B"

/** Real shell session (PTY). Keystrokes go straight to the process; output is rendered as text. */
@Composable
private fun TerminalView(vm: IdeViewModel) {
    val measurer = androidx.compose.ui.text.rememberTextMeasurer()
    val density = androidx.compose.ui.platform.LocalDensity.current
    val cell = remember { measurer.measure("M", Mono).size }
    val focus = remember { androidx.compose.ui.focus.FocusRequester() }
    val keyboard = androidx.compose.ui.platform.LocalSoftwareKeyboardController.current
    val listState = rememberLazyListState()
    var input by remember {
        androidx.compose.runtime.mutableStateOf(
            androidx.compose.ui.text.input.TextFieldValue(TERM_SENTINEL, androidx.compose.ui.text.TextRange(1)),
        )
    }

    androidx.compose.foundation.layout.BoxWithConstraints(
        Modifier.fillMaxSize().clickable { focus.requestFocus(); keyboard?.show() },
    ) {
        val cols = (with(density) { maxWidth.toPx() } / cell.width).toInt()
        val rows = (with(density) { maxHeight.toPx() } / cell.height).toInt()
        LaunchedEffect(cols, rows) {
            vm.ensureTerminal(cols, rows)
            vm.terminalResize(cols, rows)
        }
        LaunchedEffect(vm.termLines.size) {
            if (vm.termLines.isNotEmpty()) listState.scrollToItem(vm.termLines.lastIndex)
        }

        val cur = vm.termCursor
        Column(Modifier.fillMaxSize()) {
            LazyColumn(state = listState, modifier = Modifier.weight(1f).fillMaxWidth()) {
                itemsIndexed(vm.termLines) { i, line ->
                    val text = if (cur != null && cur.line.toInt() == i && vm.termAlive) {
                        val col = cur.col.toInt()
                        val padded = line.padEnd(col + 1)
                        androidx.compose.ui.text.buildAnnotatedString {
                            append(padded)
                            addStyle(
                                androidx.compose.ui.text.SpanStyle(color = C.Bg, background = C.Accent),
                                col, col + 1,
                            )
                        }
                    } else {
                        androidx.compose.ui.text.AnnotatedString(line.ifEmpty { " " })
                    }
                    Text(text, style = Mono.copy(fontSize = 15.sp), softWrap = false)
                }
            }
            if (!vm.termAlive) {
                Box(
                    Modifier.fillMaxWidth().height(MinTouch).background(C.Bar)
                        .clickable { vm.restartTerminal(cols, rows) },
                    contentAlignment = Alignment.Center,
                ) { Text("Session ended · tap to restart", color = C.Accent, fontWeight = FontWeight.SemiBold) }
            }
        }

        // Invisible field that turns keyboard input into raw terminal input.
        androidx.compose.foundation.text.BasicTextField(
            value = input,
            onValueChange = { new ->
                val t = new.text
                if (t.length < TERM_SENTINEL.length) vm.termRaw("\u007f")
                else t.replace("\u200B", "").let { typed ->
                    if (typed.isNotEmpty()) vm.termType(typed.replace("\n", "\r"))
                }
                input = androidx.compose.ui.text.input.TextFieldValue(
                    TERM_SENTINEL, androidx.compose.ui.text.TextRange(1),
                )
            },
            keyboardOptions = KeyboardOptions(
                autoCorrectEnabled = false,
                keyboardType = androidx.compose.ui.text.input.KeyboardType.Password,
            ),
            modifier = Modifier.size(1.dp).focusRequester(focus)
                .onFocusChanged { vm.terminalFocused = it.isFocused }
                .onPreviewKeyEvent { e ->
                    if (e.type != androidx.compose.ui.input.key.KeyEventType.KeyDown) return@onPreviewKeyEvent false
                    val raw = when (e.key) {
                        androidx.compose.ui.input.key.Key.Enter -> "\r"
                        androidx.compose.ui.input.key.Key.Backspace -> "\u007f"
                        androidx.compose.ui.input.key.Key.Tab -> "\t"
                        androidx.compose.ui.input.key.Key.DirectionUp -> "\u001b[A"
                        androidx.compose.ui.input.key.Key.DirectionDown -> "\u001b[B"
                        androidx.compose.ui.input.key.Key.DirectionRight -> "\u001b[C"
                        androidx.compose.ui.input.key.Key.DirectionLeft -> "\u001b[D"
                        else -> null
                    }
                    if (raw != null) { vm.termRaw(raw); true } else false
                },
        )
    }
}

@Composable
private fun DragHandle(onDelta: (Float) -> Unit, onEnd: () -> Unit) {
    Box(
        Modifier.fillMaxWidth().height(MinTouch)
            .semantics { contentDescription = "Resize output panel" }
            .pointerInput(Unit) {
                detectVerticalDragGestures(onDragEnd = onEnd, onDragCancel = onEnd) { c, dy ->
                    c.consume(); onDelta(dy)
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(width = 56.dp, height = 5.dp).clip(CircleShape).background(C.Key))
    }
}

/** Folded output: a 20dp strip, with a 44dp tap area (spec §9). */
@Composable
fun FoldedLine(vm: IdeViewModel, modifier: Modifier = Modifier) {
    val tab = vm.tabs.getOrNull(vm.activeTab)
    val err = vm.lastError
    Row(
        modifier.fillMaxWidth().height(MinTouch).background(C.Bar)
            .clickable(enabled = vm.settings.tapFoldToPeek) { vm.peeking = !vm.peeking }
            .padding(horizontal = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (vm.settings.redDotOnFold && err != null) {
            Box(Modifier.size(10.dp).clip(CircleShape).background(C.Error))
            Box(Modifier.size(8.dp))
        }
        Text(tab?.name ?: "", color = C.Text, fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
        Box(Modifier.weight(1f))
        Text(
            if (vm.peeking) "Tap to close" else if (err != null) "Error · tap to peek" else "Tap to peek",
            color = C.Muted, fontSize = 13.sp,
        )
    }
}

/** File tabs row: horizontal scroll, long-press closes, + creates a file (spec §5). */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun FileTabs(vm: IdeViewModel, trailing: @Composable () -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().height(MinTouch).background(C.Bar),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier.weight(1f).horizontalScroll(rememberScrollState()).padding(horizontal = 6.dp),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            vm.tabs.forEachIndexed { i, t ->
                val sel = i == vm.activeTab
                Row(
                    Modifier.touchTarget().clip(RoundedCornerShape(8.dp))
                        .background(if (sel) C.Panel else Color.Transparent)
                        .combinedClickable(onClick = { vm.selectTab(i) }, onLongClick = { vm.closeTab(i) })
                        .padding(horizontal = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(t.name, color = if (sel) C.Text else C.Muted, fontSize = 15.sp)
                    if (t.dirty) { // unsaved indicator (autosave clears it)
                        Box(Modifier.size(8.dp))
                        Box(Modifier.size(6.dp).clip(CircleShape).background(C.Accent))
                    }
                }
            }
            Box(
                Modifier.touchTarget().clickable { vm.overlay = IdeViewModel.Overlay.NewFile }
                    .semantics { contentDescription = "New file" },
                contentAlignment = Alignment.Center,
            ) { Text("+", color = C.Muted, fontSize = 20.sp) }
        }
        trailing()
    }
}
