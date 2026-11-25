package dev.ide.app.ui

import android.content.res.Configuration
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
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

    // ---- state machine (spec §3) -------------------------------------
    val foldAllowed = when (vm.settings.foldMode) {
        FoldMode.Always -> true
        FoldMode.LandscapeOnly -> landscape
        FoldMode.Off -> false
    }
    val terminalTyping = terminalActive && imeVisible && vm.terminalFocused
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
        if (vm.keyboardMode == KeyboardMode.Cursor) keyboard?.hide()
        else { editorFocus.requestFocus(); keyboard?.show() }
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

        when (vm.overlay) {
            IdeViewModel.Overlay.Files -> FilesDrawer(vm)
            IdeViewModel.Overlay.NewFile -> NewFileDialog(vm)
            IdeViewModel.Overlay.Customize -> CustomizeScreen(vm)
            IdeViewModel.Overlay.AutoFold -> AutoFoldScreen(vm)
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

@Composable
private fun LandscapeBody(vm: IdeViewModel, showFolded: Boolean, typing: Boolean, focus: FocusRequester) {
    Row(Modifier.fillMaxSize()) {
        if (!showFolded) {
            OutputPanel(vm, Modifier.fillMaxWidth(0.33f).fillMaxHeight(), vertical = false)
            Box(Modifier.width(1.dp).fillMaxHeight().background(C.Key))
        }
        Column(Modifier.weight(1f).fillMaxHeight()) {
            FileTabs(vm) {
                // Folded: latest error sits in the tab row (spec §10).
                if (showFolded) vm.lastError?.let {
                    Text(
                        it, color = C.Error, fontSize = 13.sp,
                        modifier = Modifier.clickable(enabled = vm.settings.tapFoldToPeek) { vm.peeking = !vm.peeking }
                            .padding(horizontal = 10.dp),
                        maxLines = 1,
                    )
                }
            }
            EditorArea(vm, showFolded, typing, focus, Modifier.weight(1f))
        }
    }
}

@Composable
private fun EditorArea(
    vm: IdeViewModel,
    showFolded: Boolean,
    typing: Boolean,
    focus: FocusRequester,
    modifier: Modifier,
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
            OutputPanel(
                vm,
                Modifier.align(Alignment.TopCenter).fillMaxWidth().fillMaxHeight(0.45f),
                vertical = false,
            )
        }
        FloatingActions(vm, typing, widthPx, with(LocalDensity.current) { maxHeight.toPx() })
    }
}
