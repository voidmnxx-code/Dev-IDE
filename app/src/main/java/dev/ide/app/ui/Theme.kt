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

/** Syntax colours (readable on the dark editor background, contrast >= 4.5:1). */
fun tokenColor(kind: uniffi.ide_core.TokenKind): Color? = when (kind) {
    uniffi.ide_core.TokenKind.KEYWORD -> Color(0xFFC792EA)
    uniffi.ide_core.TokenKind.STRING -> Color(0xFFA5D6A7)
    uniffi.ide_core.TokenKind.COMMENT -> Color(0xFF8B93A5)
    uniffi.ide_core.TokenKind.NUMBER -> Color(0xFFF9A86C)
    uniffi.ide_core.TokenKind.FUNCTION -> Color(0xFF82AAFF)
    uniffi.ide_core.TokenKind.TYPE -> Color(0xFFFFCB6B)
    uniffi.ide_core.TokenKind.CONSTANT -> Color(0xFFF9A86C)
    uniffi.ide_core.TokenKind.BUILTIN -> Color(0xFFFF8A80)
    uniffi.ide_core.TokenKind.OPERATOR -> Color(0xFF89DDFF)
    uniffi.ide_core.TokenKind.PUNCTUATION -> Color(0xFFB0B7C6)
    uniffi.ide_core.TokenKind.PROPERTY -> Color(0xFF9ED0FF)
    uniffi.ide_core.TokenKind.ATTRIBUTE -> Color(0xFFFFCB6B)
    uniffi.ide_core.TokenKind.TAG -> Color(0xFFFF8A80)
    uniffi.ide_core.TokenKind.ESCAPE -> Color(0xFF89DDFF)
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
