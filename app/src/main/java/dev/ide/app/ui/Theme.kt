package dev.ide.app.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

object C {
    val Bg = Color(0xFF14161B)
    val Panel = Color(0xFF0E1014)
    val Bar = Color(0xFF1A1D24)
    val Key = Color(0xFF2A2D35)
    val KeyboardBg = Color(0xFF2A2D35)
    val CursorKey = Color(0xFF35527F)
    val CurrentLine = Color(0xFF1F2330)
    val Selection = Color(0xFF2F5CA8)
    val Accent = Color(0xFF5AA9FF)
    val Error = Color(0xFFF28B82)
    val Ok = Color(0xFF8FD694)
    val Text = Color(0xFFE6E8EE)
    val Muted = Color(0xFF9AA0AC)
    val Scrim = Color(0xE6000000)
}

val Mono = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 15.sp, color = C.Text)

/** Spec §13: every tappable element is at least 44dp. */
val MinTouch = 44.dp
fun Modifier.touchTarget(): Modifier = this.heightIn(min = MinTouch).widthIn(min = MinTouch)

@Composable
fun IdeTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            background = C.Bg, surface = C.Bar, primary = C.Accent, onSurface = C.Text,
        ),
    ) { Box(Modifier.fillMaxSize()) { content() } }
}
