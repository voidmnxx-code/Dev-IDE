package dev.ide.app.ui

import android.content.res.Configuration
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.ide.app.FoldMode
import dev.ide.app.IdeViewModel
import dev.ide.app.KeyboardMode
import dev.ide.app.OutputTab
import dev.ide.app.PanelSnap
import kotlin.math.abs

/**
 * One layout for phone and tablet (spec §1): behaviour is identical, only sizes
 * change. The editor never moves when panels fold or open: folding only changes
 * the space *above* it, and Peek is drawn on top of it.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun IdeScreen(vm: IdeViewModel) {
    val landscape = LocalConfiguration.current.orientation == Configuration.ORIENTATION_LANDSCAPE
    val imeVisible = WindowInsets.isImeVisible
    val cursorMode = vm.keyboardMode == KeyboardMode.Cursor
    val terminalActive = vm.outputTab == OutputTab.Terminal
    val keyboard = LocalSoftwareKeyboardController.current
    val editorFocus = remember { FocusRequester() }
    val context = androidx.compose.ui.platform.LocalContext.current

    // Isolated web projects (SharedArrayBuffer / wasm threads) can only run in Chrome.
    LaunchedEffect(vm.openExternal) {
        val url = vm.openExternal ?: return@LaunchedEffect
        vm.openExternal = null
        try {
            androidx.browser.customtabs.CustomTabsIntent.Builder().build()
                .launchUrl(context, android.net.Uri.parse(url))
        } catch (_: android.content.ActivityNotFoundException) {
            vm.message = "No browser available to open $url"
        }
    }

    // Back closes the topmost thing first: overlay, then peek, then cursor editor.
    androidx.activity.compose.BackHandler(enabled = vm.overlay != null || vm.peeking) {
        if (vm.overlay != null) vm.overlay = null else vm.peeking = false
    }

    // ---- state machine (spec §3) -------------------------------------
    val foldAllowed = when (vm.settings.foldMode) {
        FoldMode.Always -> true
        FoldMode.LandscapeOnly -> true // portrait keeps the one-line output (user decision)
        FoldMode.Off -> false
    }
    val terminalTyping = terminalActive && vm.terminalFocused && (imeVisible || (cursorMode && vm.cursorForTerminal))
    val autoFold = foldAllowed && !terminalTyping &&
        ((imeVisible) || (cursorMode && vm.settings.foldInCursorMode))
    val showFolded = autoFold || vm.panelSnap == PanelSnap.Folded
    val typing = imeVisible || cursorMode

    // Leaving the folded state closes any peek.
    LaunchedEffect(showFolded) { if (!showFolded) vm.peeking = false }

    // Cursor editor replaces the system keyboard, and vice versa.
    var first by remember { mutableStateOf(true) }
    LaunchedEffect(vm.keyboardMode) {
        if (first) { first = false; return@LaunchedEffect }
        if (vm.keyboardMode == KeyboardMode.Cursor) {
            vm.cursorForTerminal = terminalActive && vm.terminalFocused
            keyboard?.hide()
        } else if (vm.cursorForTerminal) {
            vm.cursorForTerminal = false
            vm.terminalRefocus++ // TerminalView takes focus and shows the keyboard
        } else { editorFocus.requestFocus(); keyboard?.show() }
    }

    Box(Modifier.fillMaxSize().background(C.Bg).systemBarsPadding().imePadding()) {
        Column(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f).fillMaxWidth()) {
                if (landscape) LandscapeBody(vm, showFolded, typing, editorFocus)
                else PortraitBody(vm, showFolded, typing, terminalTyping, editorFocus)
            }
            // Row above the keyboard / cursor editor.
            if (cursorMode) CursorEditor(vm, compact = landscape)
            else if (imeVisible) ExtraKeysRow(vm, terminal = terminalActive && vm.terminalFocused)
        }

        // Left-edge swipe opens the file drawer (spec §5).
        if (vm.overlay == null) Box(
            Modifier.align(Alignment.CenterStart).width(14.dp).fillMaxHeight(0.6f)
                .pointerInput(Unit) {
                    detectHorizontalDragGestures { change, dx ->
                        if (dx > 8f) { change.consume(); vm.overlay = IdeViewModel.Overlay.Files }
                    }
                },
        )

        when (vm.overlay) {
            IdeViewModel.Overlay.Files -> FilesDrawer(vm)
            IdeViewModel.Overlay.NewFile -> NewFileDialog(vm)
            IdeViewModel.Overlay.Customize -> CustomizeScreen(vm)
            IdeViewModel.Overlay.AutoFold -> AutoFoldScreen(vm)
            IdeViewModel.Overlay.Git -> GitScreen(vm)
            IdeViewModel.Overlay.Packages -> PackagesScreen(vm)
            IdeViewModel.Overlay.Preview -> PreviewScreen(vm)
            IdeViewModel.Overlay.NewProject -> NewProjectScreen(vm)
            else -> Unit
        }

        vm.message?.let { m ->
            Text(
                m, color = C.Text,
                modifier = Modifier.align(Alignment.TopCenter).background(C.Key).clickable { vm.message = null }
                    .padding(horizontal = 16.dp, vertical = 10.dp),
            )
        }
    }
}

@Composable
private fun PortraitBody(
    vm: IdeViewModel,
    showFolded: Boolean,
    typing: Boolean,
    terminalTyping: Boolean,
    focus: FocusRequester,
) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val total = maxHeight
        val totalPx = with(LocalDensity.current) { total.toPx() }
        // Free-drag height; null means "use the snap value".
        var dragFraction by remember { mutableFloatStateOf(-1f) }
        val target: Dp = when {
            showFolded -> 0.dp
            dragFraction >= 0f -> total * dragFraction
            terminalTyping -> total * 0.40f
            else -> total * vm.panelSnap.fraction
        }
        val panelHeight by animateDpAsState(target, tween(if (dragFraction >= 0f) 0 else 200), label = "panel")

        Column(Modifier.fillMaxSize()) {
            if (showFolded) FoldedLine(vm)
            else OutputPanel(
                vm,
                Modifier.fillMaxWidth().height(panelHeight),
                onDragDelta = { dy ->
                    val cur = if (dragFraction >= 0f) dragFraction else vm.panelSnap.fraction
                    dragFraction = (cur + dy / totalPx).coerceIn(0f, 0.9f)
                },
                onDragEnd = {
                    // Magnet to the nearest of folded / normal / expanded.
                    val f = dragFraction.coerceAtLeast(0f)
                    vm.panelSnap = PanelSnap.entries.minBy { abs(it.fraction - f) }
                    dragFraction = -1f
                },
            )
            if (!showFolded || vm.settings.keepTabsInPortrait) FileTabs(vm)
            EditorArea(vm, showFolded, typing, focus, Modifier.weight(1f))
        }
    }
}

/** Width fractions the divider magnets to while dragging (spec §4, rotated for landscape). */
private val LandscapeSnaps = floatArrayOf(0.33f, 0.6f)
private const val FoldBelow = 0.12f

@Composable
private fun LandscapeBody(vm: IdeViewModel, showFolded: Boolean, typing: Boolean, focus: FocusRequester) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val totalWidth = maxWidth
        val totalPx = with(LocalDensity.current) { totalWidth.toPx() }
        var dragging by remember { mutableStateOf(false) }
        var live by remember { mutableFloatStateOf(vm.panelWidthFraction) }
        LaunchedEffect(vm.panelWidthFraction) { if (!dragging) live = vm.panelWidthFraction }

        if (showFolded) {
            // Page 11: output + file bar collapse into ONE thin line above the editor.
            Column(Modifier.fillMaxSize()) {
                FoldedLine(vm, compact = true, onExpand = { vm.panelSnap = PanelSnap.Normal })
                EditorArea(vm, true, typing, focus, Modifier.weight(1f), peekTabs = true)
            }
            return@BoxWithConstraints
        }
        Row(Modifier.fillMaxSize()) {
            if (!showFolded) {
                OutputPanel(vm, Modifier.width(totalWidth * live).fillMaxHeight(), vertical = false)
                // Vertical drag handle: moves the output/editor boundary left and right.
                Box(
                    Modifier.width(MinTouch).fillMaxHeight().background(C.Panel)
                        .semantics { contentDescription = "Resize output panel" }
                        .pointerInput(totalPx) {
                            detectHorizontalDragGestures(
                                onDragStart = { dragging = true },
                                onDragEnd = {
                                    dragging = false
                                    settleLandscape(vm, live) { live = it }
                                },
                                onDragCancel = {
                                    dragging = false
                                    settleLandscape(vm, live) { live = it }
                                },
                            ) { change, dx ->
                                change.consume()
                                live = (live + dx / totalPx).coerceIn(0.05f, 0.8f)
                            }
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Box(Modifier.width(1.dp).fillMaxHeight().background(C.Key))
                    Box(Modifier.size(width = 5.dp, height = 56.dp).clip(CircleShape).background(C.Muted))
                }
            }
            Column(Modifier.weight(1f).fillMaxHeight()) {
                FileTabs(vm)
                EditorArea(vm, showFolded, typing, focus, Modifier.weight(1f))
            }
        }
    }
}

/** Called when the drag ends: fold if dragged to the edge, magnet to a snap point if close. */
private fun settleLandscape(vm: IdeViewModel, value: Float, setLive: (Float) -> Unit) {
    if (value < FoldBelow) {
        vm.panelSnap = PanelSnap.Folded
        setLive(vm.panelWidthFraction) // keep the previous width for when it is reopened
        return
    }
    val near = LandscapeSnaps.minBy { abs(it - value) }
    val final = if (abs(near - value) < 0.05f) near else value
    vm.panelWidthFraction = final
    vm.panelSnap = PanelSnap.Normal
    setLive(final)
}

@Composable
private fun EditorArea(
    vm: IdeViewModel,
    showFolded: Boolean,
    typing: Boolean,
    focus: FocusRequester,
    modifier: Modifier,
    peekTabs: Boolean = false,
) {
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val widthPx = with(LocalDensity.current) { maxWidth.toPx() }
        if (vm.hasTab) EditorView(vm, focus)
        else Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text("Tap + to create a file", color = C.Muted)
        }

        if (vm.overlay == IdeViewModel.Overlay.Find) FindBar(vm, Modifier.align(Alignment.TopCenter))

        // Peek: the panel opens ABOVE the editor and never pushes the code down.
        if (showFolded && vm.peeking) {
            Column(
                Modifier.align(Alignment.TopCenter).fillMaxWidth()
                    .fillMaxHeight(if (peekTabs) 0.8f else 0.45f),
            ) {
                if (peekTabs) FileTabs(vm)
                OutputPanel(vm, Modifier.weight(1f).fillMaxWidth(), vertical = false)
            }
        }
        FloatingActions(vm, typing, widthPx, with(LocalDensity.current) { maxHeight.toPx() })
    }
}
