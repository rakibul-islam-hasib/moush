package app.moush.remote.ui

import androidx.annotation.DrawableRes
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.roundToInt

/** Press feedback: a subtle scale-down with a smooth ease. No ripple, no bounce. */
@Composable
fun Modifier.pressable(enabled: Boolean = true, onClick: () -> Unit): Modifier {
    val src = remember { MutableInteractionSource() }
    val pressed by src.collectIsPressedAsState()
    val s by animateFloatAsState(if (pressed && enabled) 0.965f else 1f, Motion.fast(), label = "press")
    return this
        .scale(s)
        .clickable(interactionSource = src, indication = null, enabled = enabled, onClick = onClick)
}

@Composable
fun MIcon(@DrawableRes id: Int, modifier: Modifier = Modifier, tint: Color = LocalPalette.current.text, size: Dp = 24.dp, mirror: Boolean = false) {
    Image(
        painter = painterResource(id),
        contentDescription = null,
        colorFilter = ColorFilter.tint(tint),
        modifier = modifier.size(size).graphicsLayer { if (mirror) scaleX = -1f },
    )
}

@Composable
fun Txt(text: String, style: TextStyle = Type.Body, color: Color = LocalPalette.current.text, modifier: Modifier = Modifier, maxLines: Int = Int.MAX_VALUE) {
    BasicText(text = text, style = style.copy(color = color), modifier = modifier, maxLines = maxLines)
}

/** Tonal container. [level] 0..2 steps up the surface ladder. */
@Composable
fun Tile(
    modifier: Modifier = Modifier,
    level: Int = 1,
    shape: Shape = RoundedCornerShape(24.dp),
    onClick: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    val p = LocalPalette.current
    val bg = when (level) {
        0 -> p.surface
        1 -> p.surfaceHigh
        else -> p.surfaceTop
    }
    val m = modifier.clip(shape).background(bg, shape).let { if (onClick != null) it.pressable(onClick = onClick) else it }
    Box(m) { content() }
}

enum class ButtonKind { Primary, Tonal, Ghost, Danger }

@Composable
fun MButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    kind: ButtonKind = ButtonKind.Primary,
    @DrawableRes icon: Int? = null,
    enabled: Boolean = true,
) {
    val p = LocalPalette.current
    val bgBrush: Brush = when (kind) {
        ButtonKind.Primary -> Brush.horizontalGradient(listOf(p.accent, p.accent))
        ButtonKind.Tonal -> Brush.horizontalGradient(listOf(p.surfaceTop, p.surfaceTop))
        ButtonKind.Ghost -> Brush.horizontalGradient(listOf(Color.Transparent, Color.Transparent))
        ButtonKind.Danger -> Brush.horizontalGradient(listOf(p.danger.copy(alpha = 0.16f), p.danger.copy(alpha = 0.16f)))
    }
    val fg = when (kind) {
        ButtonKind.Primary -> p.onAccent
        ButtonKind.Tonal -> p.text
        ButtonKind.Ghost -> p.textDim
        ButtonKind.Danger -> p.danger
    }
    val alpha by animateFloatAsState(if (enabled) 1f else 0.4f, Motion.fast(), label = "btn-alpha")
    Row(
        modifier
            .graphicsLayer { this.alpha = alpha }
            .height(52.dp)
            .clip(RoundedCornerShape(18.dp))
            .background(bgBrush)
            .pressable(enabled = enabled, onClick = onClick)
            .padding(horizontal = 22.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            MIcon(icon, tint = fg, size = 20.dp)
            Box(Modifier.width(10.dp))
        }
        Txt(text, Type.Label.copy(fontSize = 15.sp), fg)
    }
}

@Composable
fun SectionLabel(text: String, modifier: Modifier = Modifier) {
    Txt(text.uppercase(), Type.Caption, LocalPalette.current.textFaint, modifier.padding(start = 6.dp, bottom = 10.dp, top = 6.dp))
}

/** Toggle with a gliding knob: tween only. */
@Composable
fun MSwitch(checked: Boolean, onChange: (Boolean) -> Unit) {
    val p = LocalPalette.current
    val track by animateColorAsState(if (checked) p.accent else p.surfaceTop, Motion.base(), label = "track")
    val knob by animateFloatAsState(if (checked) 1f else 0f, Motion.base(), label = "knob")
    Box(
        Modifier
            .width(50.dp).height(30.dp)
            .clip(CircleShape).background(track)
            .pressable { onChange(!checked) },
    ) {
        Box(
            Modifier
                .offset(x = (4f + 20f * knob).dp, y = 4.dp)
                .size(22.dp)
                .clip(CircleShape)
                .background(if (checked) p.onAccent else p.textDim),
        )
    }
}

@Composable
fun SettingRow(title: String, subtitle: String? = null, trailing: @Composable () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Txt(title, Type.Body)
            if (subtitle != null) Txt(subtitle, Type.Label, LocalPalette.current.textDim, Modifier.padding(top = 2.dp))
        }
        trailing()
    }
}

/** Flat slider: filled track and a small knob, no shadow. */
@Composable
fun MSlider(value: Float, onChange: (Float) -> Unit, range: ClosedFloatingPointRange<Float> = 0f..1f, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    var widthPx by remember { androidx.compose.runtime.mutableIntStateOf(1) }
    val frac = ((value - range.start) / (range.endInclusive - range.start)).coerceIn(0f, 1f)
    val density = LocalDensity.current
    Box(
        modifier
            .fillMaxWidth()
            .height(36.dp)
            .onSizeChanged { widthPx = it.width.coerceAtLeast(1) }
            .pointerInput(range) {
                detectTapGestures { o -> onChange(range.start + (o.x / widthPx).coerceIn(0f, 1f) * (range.endInclusive - range.start)) }
            }
            .pointerInput(range) {
                detectHorizontalDragGestures { change, _ ->
                    change.consume()
                    onChange(range.start + (change.position.x / widthPx).coerceIn(0f, 1f) * (range.endInclusive - range.start))
                }
            },
        contentAlignment = Alignment.CenterStart,
    ) {
        Box(Modifier.fillMaxWidth().height(6.dp).clip(CircleShape).background(p.surfaceTop))
        Box(Modifier.fillMaxWidth(frac).height(6.dp).clip(CircleShape).background(Brush.horizontalGradient(listOf(p.accent, p.accentAlt))))
        val knobX = with(density) { (widthPx * frac).toDp() } - 9.dp
        Box(Modifier.offset(x = knobX.coerceAtLeast(0.dp)).size(18.dp).clip(CircleShape).background(p.text))
    }
}

@Composable
fun <T> Segmented(options: List<Pair<T, String>>, selected: T, onSelect: (T) -> Unit) {
    val p = LocalPalette.current
    val idx = options.indexOfFirst { it.first == selected }.coerceAtLeast(0)
    val pos by animateFloatAsState(idx.toFloat(), Motion.base(), label = "seg")
    var w by remember { androidx.compose.runtime.mutableIntStateOf(1) }
    Box(
        Modifier.fillMaxWidth().height(44.dp).clip(RoundedCornerShape(16.dp)).background(p.surfaceTop).onSizeChanged { w = it.width },
    ) {
        val segW = with(LocalDensity.current) { (w / options.size).toDp() }
        Box(
            Modifier
                .offset { androidx.compose.ui.unit.IntOffset((pos * w / options.size).roundToInt() + 3, 0) }
                .padding(vertical = 3.dp)
                .width(segW - 6.dp)
                .fillMaxWidth()
                .height(38.dp)
                .clip(RoundedCornerShape(13.dp))
                .background(p.surface),
        )
        Row(Modifier.fillMaxWidth().height(44.dp)) {
            options.forEach { (v, label) ->
                Box(Modifier.weight(1f).height(44.dp).clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { onSelect(v) }, contentAlignment = Alignment.Center) {
                    Txt(label, Type.Label, if (v == selected) p.text else p.textDim)
                }
            }
        }
    }
}

fun Modifier.hairline(color: Color, shape: Shape): Modifier = this.border(1.dp, color, shape)
