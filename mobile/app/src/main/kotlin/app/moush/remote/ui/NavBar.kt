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
    val barColor = if (p.dark) p.surfaceHigh else p.surface
    // A flat strip in the page colour: content ends cleanly above the bar.
    Box(
        modifier
            .fillMaxWidth()
            .background(p.bg)
            .padding(top = 10.dp)
            .navigationBarsPadding()
            .padding(start = 16.dp, end = 16.dp, bottom = 10.dp),
    ) {
        BoxWithConstraints(
            Modifier
                .fillMaxWidth()
                .height(64.dp)
                .clip(CircleShape)
                .background(barColor)
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
                    .clip(CircleShape)
                    .background(p.accent),
            )
            Row(Modifier.fillMaxSize()) {
                items.forEachIndexed { i, item ->
                    val reveal = ((weights[i] - 1f) / 1.5f).coerceIn(0f, 1f)
                    val tint = lerpColor(p.textDim, p.onAccent, reveal)
                    Box(
                        Modifier
                            .weight(weights[i])
                            .fillMaxHeight()
                            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { onSelect(i) },
                        contentAlignment = Alignment.Center,
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box {
                                MIcon(item.icon, tint = tint, size = 24.dp)
                                if (item.badge != BadgeKind.None) {
                                    // A dot with a ring in the colour behind it, so it reads as a cut-out.
                                    Box(
                                        Modifier
                                            .align(Alignment.TopEnd)
                                            .offset(x = 5.dp, y = (-4).dp)
                                            .size(12.dp)
                                            .clip(CircleShape)
                                            .background(lerpColor(barColor, p.accent, reveal)),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        Box(
                                            Modifier.size(7.dp).clip(CircleShape)
                                                .background(if (item.badge == BadgeKind.Online) p.ok else p.accentAlt),
                                        )
                                    }
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
