package app.moush.remote.ui

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import app.moush.remote.R

/** Tonal palette: depth comes from surface steps, never from shadows. */
@Immutable
class Palette(
    val dark: Boolean,
    val bg: Color,
    val surface: Color,
    val surfaceHigh: Color,
    val surfaceTop: Color,
    val hairline: Color,
    val text: Color,
    val textDim: Color,
    val textFaint: Color,
    val accent: Color,
    val accentAlt: Color,
    val onAccent: Color,
    val ok: Color,
    val danger: Color,
)

val DarkPalette = Palette(
    dark = true,
    bg = Color(0xFF0B0C10),
    surface = Color(0xFF13151B),
    surfaceHigh = Color(0xFF1A1D25),
    surfaceTop = Color(0xFF232731),
    hairline = Color(0x14FFFFFF),
    text = Color(0xFFECEEF4),
    textDim = Color(0xFF8E94A5),
    textFaint = Color(0xFF565C6C),
    accent = Color(0xFF8B7BFF),
    accentAlt = Color(0xFF3DDCC2),
    onAccent = Color(0xFF0B0A18),
    ok = Color(0xFF4ADE9A),
    danger = Color(0xFFFF6B7A),
)

val LightPalette = Palette(
    dark = false,
    bg = Color(0xFFF3F4F8),
    surface = Color(0xFFFFFFFF),
    surfaceHigh = Color(0xFFEBEDF3),
    surfaceTop = Color(0xFFE0E3EC),
    hairline = Color(0x14000000),
    text = Color(0xFF12141A),
    textDim = Color(0xFF5E6474),
    textFaint = Color(0xFF9AA0AF),
    accent = Color(0xFF5B49F5),
    accentAlt = Color(0xFF0FB9A0),
    onAccent = Color(0xFFFFFFFF),
    ok = Color(0xFF12A065),
    danger = Color(0xFFD8364A),
)

val LocalPalette = staticCompositionLocalOf { DarkPalette }

/** Motion: one calm deceleration curve everywhere. No springs, no overshoot. */
object Motion {
    val Smooth = CubicBezierEasing(0.22f, 0.61f, 0.36f, 1f)
    val InOut = CubicBezierEasing(0.65f, 0f, 0.35f, 1f)
    const val Fast = 140
    const val Base = 260
    const val Slow = 420

    fun <T> fast() = tween<T>(Fast, easing = Smooth)
    fun <T> base() = tween<T>(Base, easing = Smooth)
    fun <T> slow() = tween<T>(Slow, easing = Smooth)
}

object Type {
    val Poppins = FontFamily(
        Font(R.font.poppins_regular, FontWeight.Normal),
        Font(R.font.poppins_medium, FontWeight.Medium),
        Font(R.font.poppins_semibold, FontWeight.SemiBold),
        Font(R.font.poppins_bold, FontWeight.Bold),
    )
    private val family = Poppins
    val Display = TextStyle(fontFamily = family, fontWeight = FontWeight.SemiBold, fontSize = 27.sp, letterSpacing = (-0.5).sp)
    val Title = TextStyle(fontFamily = family, fontWeight = FontWeight.SemiBold, fontSize = 18.sp, letterSpacing = (-0.2).sp)
    val Body = TextStyle(fontFamily = family, fontWeight = FontWeight.Normal, fontSize = 14.sp, lineHeight = 21.sp)
    val Label = TextStyle(fontFamily = family, fontWeight = FontWeight.Medium, fontSize = 12.sp, letterSpacing = 0.1.sp)
    val Caption = TextStyle(fontFamily = family, fontWeight = FontWeight.Medium, fontSize = 11.sp, letterSpacing = 0.6.sp)
    val Code = TextStyle(fontFamily = family, fontWeight = FontWeight.SemiBold, fontSize = 44.sp, letterSpacing = 2.sp)
}

@Composable
fun MoushTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val p = if (dark) DarkPalette else LightPalette
    val scheme = if (dark) {
        darkColorScheme(primary = p.accent, onPrimary = p.onAccent, background = p.bg, surface = p.surface, onSurface = p.text, onBackground = p.text)
    } else {
        lightColorScheme(primary = p.accent, onPrimary = p.onAccent, background = p.bg, surface = p.surface, onSurface = p.text, onBackground = p.text)
    }
    CompositionLocalProvider(LocalPalette provides p) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
