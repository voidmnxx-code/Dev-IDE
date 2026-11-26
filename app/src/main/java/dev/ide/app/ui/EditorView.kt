package dev.ide.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.runtime.produceState
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.buildAnnotatedString
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import uniffi.ide_core.TokenSpan
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import dev.ide.app.IdeViewModel
import dev.ide.app.KeyboardMode

private const val SENTINEL = "​​"

/** Code point offset -> UTF-16 index inside [s]. */
private fun String.cpToIndex(cp: Int): Int =
    offsetByCodePoints(0, cp.coerceIn(0, codePointCount(0, length)))

/**
 * The editor. Text lives in Rust; this composable only renders it and forwards
 * input. System-keyboard input arrives through an invisible text field whose
 * content is a fixed sentinel, so every change is a pure "typed X / backspace".
 */
@Composable
fun EditorView(vm: IdeViewModel, keyboardRequest: FocusRequester, modifier: Modifier = Modifier) {
    val text = vm.text()
    val lines = remember(text) { text.split("\n") }
    val cur = vm.cursor()
    val listState = rememberLazyListState()
    val keyboard = LocalSoftwareKeyboardController.current

    // Cumulative start offset (in code points) of each line.
    val lineStarts = remember(lines) {
        val out = IntArray(lines.size)
        var acc = 0
        lines.forEachIndexed { i, l -> out[i] = acc; acc += l.codePointCount(0, l.length) + 1 }
        out
    }

    // Syntax highlighting runs off the UI thread; stale spans are clamped per line.
    val spans by produceState(emptyList<TokenSpan>(), vm.version, vm.activeTab) {
        delay(40) // debounce while typing
        value = withContext(Dispatchers.Default) { vm.highlight() }
    }
    val styled = remember(lines, lineStarts, spans) { styleLines(lines, lineStarts, spans) }

    // Keep the caret line visible without moving anything else.
    LaunchedEffect(cur?.line) {
        val line = cur?.line?.toInt() ?: return@LaunchedEffect
        val visible = listState.layoutInfo.visibleItemsInfo
        if (visible.isNotEmpty() && (line < visible.first().index || line > visible.last().index)) {
            listState.animateScrollToItem(line)
        }
    }

    Box(
        modifier.fillMaxSize().background(C.Bg).pointerInput(Unit) {
            // Two-finger tap = undo (spec §5). Observes only; never consumes.
            androidx.compose.foundation.gestures.awaitEachGesture {
                val first = awaitFirstDown(requireUnconsumed = false, pass = androidx.compose.ui.input.pointer.PointerEventPass.Initial)
                val t0 = first.uptimeMillis
                var maxPointers = 1
                var moved = false
                while (true) {
                    val ev = awaitPointerEvent(androidx.compose.ui.input.pointer.PointerEventPass.Initial)
                    maxPointers = maxOf(maxPointers, ev.changes.count { it.pressed })
                    if (ev.changes.any { (it.position - it.previousPosition).getDistance() > 8f && it.pressed }) moved = true
                    if (ev.changes.none { it.pressed }) break
                    if (ev.changes.first().uptimeMillis - t0 > 400) return@awaitEachGesture
                }
                if (maxPointers == 2 && !moved) vm.undo()
            }
        },
    ) {
        LazyColumn(state = listState, modifier = Modifier.fillMaxSize()) {
            itemsIndexed(lines) { i, line ->
                EditorLine(
                    number = i + 1,
                    line = line,
                    styled = styled[i],
                    start = lineStarts[i],
                    isCurrent = cur?.line?.toInt() == i,
                    caretCol = if (cur?.line?.toInt() == i) cur.column.toInt() else -1,
                    selStart = cur?.selStart?.toInt() ?: 0,
                    selEnd = cur?.selEnd?.toInt() ?: 0,
                    onTapAt = { cpCol ->
                        vm.setCursor(lineStarts[i] + cpCol)
                        if (vm.keyboardMode == KeyboardMode.System) {
                            keyboardRequest.requestFocus()
                            keyboard?.show()
                        }
                    },
                )
            }
        }
        ImeBridge(vm, keyboardRequest)
    }
}

@Composable
private fun EditorLine(
    number: Int,
    line: String,
    styled: AnnotatedString,
    start: Int,
    isCurrent: Boolean,
    caretCol: Int,
    selStart: Int,
    selEnd: Int,
    onTapAt: (Int) -> Unit,
) {
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
    Row(
        Modifier.fillMaxWidth().then(if (isCurrent) Modifier.background(C.CurrentLine) else Modifier)
            .padding(vertical = 5.dp),
    ) {
        Text(
            "$number",
            style = Mono.copy(color = C.Muted),
            modifier = Modifier.width(44.dp).padding(end = 12.dp),
            textAlign = androidx.compose.ui.text.style.TextAlign.End,
        )
        Box(
            Modifier.weight(1f)
                .pointerInput(line) {
                    detectTapGestures { pos ->
                        val l = layout ?: return@detectTapGestures
                        val idx = l.getOffsetForPosition(pos).coerceIn(0, line.length)
                        onTapAt(line.codePointCount(0, idx))
                    }
                }
                .drawBehind {
                    val l = layout ?: return@drawBehind
                    val lineEnd = start + line.codePointCount(0, line.length)
                    if (selEnd > selStart && selEnd > start && selStart <= lineEnd) {
                        val a = line.cpToIndex((selStart - start).coerceAtLeast(0))
                        val b = if (selEnd > lineEnd) line.length else line.cpToIndex(selEnd - start)
                        if (b > a) {
                            drawPath(l.getPathForRange(a, b), C.Selection)
                        } else if (selEnd > lineEnd && line.isEmpty()) {
                            drawRect(C.Selection, size = Size(8.dp.toPx(), l.size.height.toFloat()))
                        }
                        // Selection spanning the line break: extend a little past the last glyph.
                        if (selEnd > lineEnd && selStart <= lineEnd && b >= a) {
                            val end = l.getCursorRect(line.length)
                            drawRect(C.Selection, Offset(end.left, end.top), Size(8.dp.toPx(), end.height))
                        }
                    }
                    if (caretCol >= 0) {
                        val r = l.getCursorRect(line.cpToIndex(caretCol))
                        drawRect(C.Accent, Offset(r.left, r.top), Size(2.dp.toPx(), r.height))
                    }
                },
        ) {
            Text(
                if (line.isEmpty()) AnnotatedString(" ") else styled,
                style = Mono,
                softWrap = false,
                onTextLayout = { layout = it },
            )
        }
    }
}

private fun styleLines(lines: List<String>, lineStarts: IntArray, spans: List<TokenSpan>): List<AnnotatedString> {
    val per = Array(lines.size) { mutableListOf<Triple<Int, Int, androidx.compose.ui.graphics.Color>>() }
    for (sp in spans) {
        val color = tokenColor(sp.kind) ?: continue
        var s = sp.start.toInt()
        val e = sp.end.toInt()
        var li = java.util.Arrays.binarySearch(lineStarts, s).let { if (it >= 0) it else -it - 2 }.coerceAtLeast(0)
        while (li < lines.size && s < e) {
            val ls = lineStarts[li]
            val l = lines[li]
            val len = l.codePointCount(0, l.length)
            val a = (s - ls).coerceAtLeast(0)
            val b = (e - ls).coerceAtMost(len)
            if (b > a) per[li].add(Triple(l.cpToIndex(a), l.cpToIndex(b), color))
            s = ls + len + 1 // start of the next line
            li++
        }
    }
    return lines.mapIndexed { i, l ->
        buildAnnotatedString {
            append(l)
            per[i].forEach { (a, b, c) -> addStyle(SpanStyle(color = c), a, b) }
        }
    }
}

/** Invisible field that turns IME input into editor commands. */
@Composable
private fun ImeBridge(vm: IdeViewModel, focus: FocusRequester) {
    var value by remember { mutableStateOf(TextFieldValue(SENTINEL, TextRange(1))) }
    BasicTextField(
        value = value,
        onValueChange = { new ->
            val t = new.text
            when {
                t.length < SENTINEL.length -> vm.backspace()
                else -> {
                    val typed = t.replace("​", "")
                    typed.codePoints().forEach { cp ->
                        val s = String(Character.toChars(cp))
                        if (s == "\n") vm.newline() else vm.typeChar(s)
                    }
                }
            }
            value = TextFieldValue(SENTINEL, TextRange(1)) // reset to sentinel
        },
        keyboardOptions = KeyboardOptions(
            capitalization = KeyboardCapitalization.None,
            autoCorrectEnabled = false,
            keyboardType = KeyboardType.Password, // no suggestions/composition: raw keystrokes
        ),
        modifier = Modifier
            .size(1.dp)
            .focusRequester(focus)
            .onPreviewKeyEvent { e ->
                if (e.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                when (e.key) {
                    Key.Enter -> { vm.newline(); true }
                    Key.Tab -> { vm.insert("  "); true }
                    Key.Backspace -> { vm.backspace(); true }
                    else -> false
                }
            },
    )
}
