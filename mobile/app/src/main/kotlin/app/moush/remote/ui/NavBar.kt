package app.moush.remote.ui

import androidx.annotation.DrawableRes
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Lets a screen slide the tab bar away (bottom sheets). */
class NavControl {
    var hidden by androidx.compose.runtime.mutableStateOf(false)
}

val LocalNavControl = androidx.compose.runtime.staticCompositionLocalOf { NavControl() }

data class NavItem(val label: String, @DrawableRes val icon: Int, val badge: BadgeKind = BadgeKind.None)

enum class BadgeKind { None, Online, Busy }

/**
 * Floating tab bar. The selected tab grows to reveal its label while a tinted pill
 * glides behind it; every other tab shrinks to a bare icon. Everything is driven by
 * the same eased tween, so the whole bar moves as one piece.
 */
@Composable
fun NavBar(items: List<NavItem>, selected: Int, onSelect: (Int) -> Unit, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    val weights = items.indices.map { i ->
        val w by animateFloatAsState(if (i == selected) 2.5f else 1f, Motion.slow(), label = "w$i")
        w
    }
    val total = weights.sum()
    // A soft fade keeps scrolling content from colliding with the floating bar.
    Box(
        modifier
            .fillMaxWidth()
            .background(Brush.verticalGradient(listOf(Color.Transparent, p.bg.copy(alpha = 0.92f), p.bg)))
            .padding(top = 28.dp)
            .navigationBarsPadding()
            .padding(start = 18.dp, end = 18.dp, bottom = 10.dp),
    ) {
        BoxWithConstraints(
            Modifier
                .fillMaxWidth()
                .height(66.dp)
                .clip(RoundedCornerShape(26.dp))
                .background(if (p.dark) p.surfaceTop else p.surface)
                .padding(6.dp),
        ) {
            val innerW = maxWidth
            val before = weights.take(selected).sum()
            val x = innerW * (before / total)
            val w = innerW * (weights[selected] / total)
            // Gliding pill.
            Box(
                Modifier
                    .offset(x = x)
                    .width(w)
                    .fillMaxHeight()
                    .clip(RoundedCornerShape(21.dp))
                    .background(Brush.horizontalGradient(listOf(p.accent.copy(alpha = 0.26f), p.accentAlt.copy(alpha = 0.16f)))),
            )
            Row(Modifier.fillMaxSize()) {
                items.forEachIndexed { i, item ->
                    val reveal = ((weights[i] - 1f) / 1.5f).coerceIn(0f, 1f)
                    val tint = lerpColor(p.textDim, p.text, reveal)
                    Box(
                        Modifier
                            .weight(weights[i])
                            .fillMaxHeight()
                            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { onSelect(i) },
                        contentAlignment = Alignment.Center,
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box {
                                MIcon(item.icon, tint = lerpColor(p.textDim, p.accent, reveal), size = 24.dp)
                                if (item.badge != BadgeKind.None) {
                                    Box(
                                        Modifier
                                            .align(Alignment.TopEnd)
                                            .offset(x = 3.dp, y = (-2).dp)
                                            .size(8.dp)
                                            .clip(CircleShape)
                                            .background(if (item.badge == BadgeKind.Online) p.ok else p.accentAlt),
                                    )
                                }
                            }
                            if (reveal > 0.02f) {
                                Box(Modifier.width((8 * reveal).dp))
                                Box(Modifier.graphicsLayer { alpha = reveal }) {
                                    Txt(item.label, Type.Label.copy(fontSize = 14.sp), tint, maxLines = 1)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fun lerpColor(a: androidx.compose.ui.graphics.Color, b: androidx.compose.ui.graphics.Color, t: Float) =
    androidx.compose.ui.graphics.lerp(a, b, t.coerceIn(0f, 1f))
