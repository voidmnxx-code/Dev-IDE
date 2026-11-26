package dev.ide.app.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.ide.app.Action
import dev.ide.app.IdeViewModel
import kotlin.math.roundToInt

private val FabSize = 48.dp

/**
 * Floating ^ button (spec §6): draggable with edge snapping, tap opens layer 1,
 * long-press runs the assigned command, "More" opens layer 2.
 * [containerWidthPx] is the width of the editor area, used for snapping.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun BoxScope.FloatingActions(vm: IdeViewModel, typing: Boolean, containerWidthPx: Float, containerHeightPx: Float) {
    // 0 = closed, 1 = layer 1, 2 = layer 2
    var layer by remember { mutableIntStateOf(0) }
    // Offset relative to the bottom-right anchor (<= 0 means moved left/up).
    var dx by remember { mutableFloatStateOf(0f) }
    var dy by remember { mutableFloatStateOf(0f) }
    val sizePx = with(androidx.compose.ui.platform.LocalDensity.current) { (FabSize + 16.dp).toPx() }
    val maxUp = (containerHeightPx - sizePx).coerceAtLeast(0f)

    // When the keyboard / cursor editor appears the editor shrinks: bring the
    // button back to its default spot (bottom edge) so it never leaves the screen.
    androidx.compose.runtime.LaunchedEffect(typing) { if (typing) dy = 0f }
    // Also keep it inside the area if the container shrinks for any other reason.
    androidx.compose.runtime.LaunchedEffect(maxUp) { if (-dy > maxUp) dy = -maxUp }

    val onLeft = dx < -(containerWidthPx - sizePx) / 2f

    if (layer != 0) {
        Box(Modifier.fillMaxSize().background(C.Scrim).clickable { layer = 0 })
    }

    if (layer == 2) {
        Layer2Card(vm, onBack = { layer = 1 }, onClose = { layer = 0 })
    }

    Column(
        Modifier.align(Alignment.BottomEnd)
            .offset { IntOffset(dx.roundToInt(), dy.roundToInt()) }
            .padding(16.dp),
        horizontalAlignment = if (onLeft) Alignment.Start else Alignment.End,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (layer == 1) {
            // Most used nearest the thumb: list is rendered bottom-up.
            vm.settings.layer1.reversed().forEach { a ->
                Pill(a.label, primary = a == Action.Run) {
                    if (a == Action.More) layer = 2 else { vm.perform(a); layer = 0 }
                }
            }
        }
        Box(
            Modifier.size(FabSize).alpha(if (layer != 0) 1f else if (onLeft) 0.55f else if (typing) 0.7f else 1f)
                .clip(CircleShape).background(C.Accent)
                .semantics { contentDescription = "Quick actions" }
                .pointerInput(Unit) {
                    detectDragGestures(
                        onDragEnd = {
                            // Snap to the nearest horizontal edge on release.
                            dx = if (dx < -(containerWidthPx - sizePx) / 2f) -(containerWidthPx - sizePx) else 0f
                        },
                    ) { change, drag ->
                        change.consume()
                        dx = (dx + drag.x).coerceIn(-(containerWidthPx - sizePx), 0f)
                        dy = (dy + drag.y).coerceIn(-maxUp, 0f)
                    }
                }
                .combinedClickable(
                    onClick = { layer = if (layer == 0) 1 else 0 },
                    onLongClick = { vm.perform(vm.settings.longPressAction) },
                ),
            contentAlignment = Alignment.Center,
        ) { Text(if (layer == 0) "⌃" else "⌄", color = C.Bg, fontSize = 22.sp, fontWeight = FontWeight.Bold) }
    }
}

@Composable
private fun Pill(label: String, primary: Boolean = false, onClick: () -> Unit) {
    Box(
        Modifier.height(MinTouch).clip(CircleShape)
            .background(if (primary) C.Accent else C.Key)
            .border(1.dp, C.Muted.copy(alpha = 0.3f), CircleShape)
            .clickable(onClick = onClick)
            .padding(horizontal = 26.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = if (primary) C.Bg else C.Text, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
    }
}

@Composable
private fun BoxScope.Layer2Card(vm: IdeViewModel, onBack: () -> Unit, onClose: () -> Unit) {
    Column(
        Modifier.align(Alignment.BottomCenter).padding(start = 16.dp, end = 16.dp, bottom = 80.dp)
            .fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(C.Bar)
            .border(1.dp, C.Muted.copy(alpha = 0.3f), RoundedCornerShape(24.dp))
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text("‹ Back", color = C.Text, fontWeight = FontWeight.Bold, fontSize = 18.sp,
                modifier = Modifier.touchTarget().clickable(onClick = onBack).padding(4.dp))
            Text("Customize", color = C.Accent, fontWeight = FontWeight.SemiBold, fontSize = 16.sp,
                modifier = Modifier.touchTarget().clickable {
                    vm.overlay = IdeViewModel.Overlay.Customize; onClose()
                }.padding(4.dp))
        }
        vm.settings.layer2.chunked(2).forEach { row ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                row.forEach { a ->
                    Box(
                        Modifier.weight(1f).height(MinTouch + 8.dp).clip(RoundedCornerShape(14.dp))
                            .background(C.Key).clickable { vm.perform(a); onClose() }.padding(horizontal = 14.dp),
                        contentAlignment = Alignment.CenterStart,
                    ) { Text(a.label, color = C.Text, fontWeight = FontWeight.SemiBold, fontSize = 16.sp) }
                }
                if (row.size == 1) Box(Modifier.weight(1f))
            }
        }
    }
}
