package app.moush.remote.ui

import android.view.HapticFeedbackConstants
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import app.moush.remote.AppViewModel
import app.moush.remote.R
import app.moush.remote.data.RemoteDisplay
import app.moush.remote.data.ScreenFrame
import app.moush.remote.data.ViewRequest
import app.moush.remote.net.LinkState
import app.moush.remote.net.Protocol
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.debounce
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.roundToInt

private enum class Drive { Touch, Pad }
private enum class ClickType { Left, Right, Drag }
private enum class Quality(val label: String, val jpeg: Int, val fps: Int, val edge: Int) {
    Saver("Saver", 40, 10, 1000),
    Balanced("Balanced", 60, 15, 1400),
    Sharp("Sharp", 82, 20, 1900),
}

/** Geometry shared by the gestures and the drawing: which part of a display is on screen. */
private class Viewport {
    var box by mutableStateOf(IntSize(1, 1))
    var display by mutableStateOf<RemoteDisplay?>(null)
    var zoom by mutableFloatStateOf(1f)
    var cx by mutableFloatStateOf(0f) // centre of the view, display pixels
    var cy by mutableFloatStateOf(0f)

    val baseScale: Float
        get() = display?.let { min(box.width / it.width.toFloat(), box.height / it.height.toFloat()) } ?: 1f
    val scale: Float get() = baseScale * zoom

    fun reset() {
        val d = display ?: return
        zoom = 1f
        cx = d.width / 2f
        cy = d.height / 2f
    }

    fun clamp() {
        val d = display ?: return
        zoom = zoom.coerceIn(1f, 8f)
        val hw = box.width / (2 * scale)
        val hh = box.height / (2 * scale)
        cx = if (hw * 2 >= d.width) d.width / 2f else cx.coerceIn(hw, d.width - hw)
        cy = if (hh * 2 >= d.height) d.height / 2f else cy.coerceIn(hh, d.height - hh)
    }

    /** Container pixel → display-local pixel. */
    fun toDisplay(p: Offset) = Offset((p.x - box.width / 2f) / scale + cx, (p.y - box.height / 2f) / scale + cy)

    fun toContainer(x: Float, y: Float) = Offset((x - cx) * scale + box.width / 2f, (y - cy) * scale + box.height / 2f)

    /** The visible rectangle in display pixels: (x, y, w, h). */
    fun visible(): IntArray {
        val d = display ?: return intArrayOf(0, 0, 0, 0)
        if (zoom <= 1.02f) return intArrayOf(0, 0, 0, 0)
        val w = min(d.width.toFloat(), box.width / scale)
        val h = min(d.height.toFloat(), box.height / scale)
        return intArrayOf((cx - w / 2).roundToInt(), (cy - h / 2).roundToInt(), w.roundToInt(), h.roundToInt())
    }
}

@OptIn(FlowPreview::class)
@Composable
fun ScreenScreen(vm: AppViewModel) {
    val p = LocalPalette.current
    val view = LocalView.current
    val link by vm.link.collectAsState()
    val caps by vm.session.peerCaps.collectAsState()
    val settings by vm.settings.collectAsState()
    val displays by vm.screen.displays.collectAsState()
    val frame by vm.screen.frame.collectAsState()
    val cursor by vm.screen.cursor.collectAsState()
    val fps by vm.screen.fps.collectAsState()
    val error by vm.screen.error.collectAsState()
    val connected = link is LinkState.Connected
    val allowed = caps and Protocol.CAP_SCREEN != 0

    val vp = remember { Viewport() }
    var selected by remember { mutableIntStateOf(-1) }
    var sheet by remember { mutableStateOf(false) }
    var drive by remember { mutableStateOf(Drive.Touch) }
    var clickType by remember { mutableStateOf(ClickType.Left) }
    var quality by remember { mutableStateOf(Quality.Balanced) }
    var aim by remember { mutableStateOf<Offset?>(null) }
    val nav = LocalNavControl.current
    LaunchedEffect(sheet) { nav.hidden = sheet }
    DisposableEffect(Unit) { onDispose { nav.hidden = false } }
    val tick = { if (settings.haptics) view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP) }

    // Ask for the display list when the tab opens; stop streaming when it closes.
    LaunchedEffect(connected, allowed) {
        if (connected && allowed) vm.screen.requestDisplays() else vm.screen.reset()
    }
    DisposableEffect(Unit) { onDispose { vm.screen.stop() } }

    // First list: pick the primary display.
    LaunchedEffect(displays) {
        if (displays.isNotEmpty() && (selected !in displays.indices)) {
            selected = displays.indexOfFirst { it.primary }.takeIf { it >= 0 } ?: 0
        }
    }
    LaunchedEffect(selected, displays) {
        vp.display = displays.getOrNull(selected)
        vp.reset()
    }

    // Tell the computer what is visible (debounced), so zoomed views stay sharp.
    LaunchedEffect(Unit) {
        snapshotFlow {
            val d = vp.display
            val v = vp.visible()
            listOf(d?.index ?: -1, v[0], v[1], v[2], v[3], vp.box.width, vp.box.height, quality.ordinal)
        }.debounce(140).collect { s ->
            val d = vp.display ?: return@collect
            if (!connected || !allowed) return@collect
            val long = max(vp.box.width, vp.box.height).toFloat()
            val k = min(1f, quality.edge / long)
            vm.screen.setView(
                ViewRequest(
                    display = d.index, cropX = s[1], cropY = s[2], cropW = s[3], cropH = s[4],
                    outW = (vp.box.width * k).roundToInt().coerceAtLeast(64), outH = (vp.box.height * k).roundToInt().coerceAtLeast(64),
                    quality = quality.jpeg, fps = quality.fps,
                ),
            )
        }
    }

    Box(Modifier.fillMaxSize()) {
        // The viewer fills everything below the header; chrome floats above it.
        Column(Modifier.fillMaxSize().statusBarsPadding().padding(horizontal = 12.dp)) {
            Box(Modifier.height(66.dp))
            Box(
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(30.dp))
                    .background(p.surface)
                    .onSizeChanged { vp.box = IntSize(it.width.coerceAtLeast(1), it.height.coerceAtLeast(1)); vp.clamp() },
            ) {
                Viewer(vp, frame, cursor?.takeIf { it.display == vp.display?.index }, aim)
                if (connected && allowed && vp.display != null) {
                    Box(
                        Modifier.fillMaxSize().pointerInput(vp.display, drive, clickType) {
                            gestures(vp, drive, clickType, { settings }, vm, tick) { aim = it }
                        },
                    )
                    MiniMap(vp, Modifier.align(Alignment.BottomStart).padding(14.dp))
                    Loupe(vp, frame, aim)
                }
                if (frame == null && connected && allowed && error == null) Waiting(displays.isEmpty())
                StatusOverlay(connected, allowed, error, onOpenLink = {})
            }
            Box(Modifier.height(124.dp))
        }

        // Header: display picker + live stats.
        Row(
            Modifier.fillMaxWidth().statusBarsPadding().padding(horizontal = 18.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val d = displays.getOrNull(selected)
            Glass(onClick = { if (displays.isNotEmpty()) sheet = true }) {
                Row(Modifier.padding(start = 12.dp, end = 16.dp, top = 10.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(30.dp).clip(CircleShape).background(p.accent.copy(alpha = 0.18f)), contentAlignment = Alignment.Center) {
                        MIcon(R.drawable.ic_monitor, tint = p.accent, size = 18.dp)
                    }
                    Box(Modifier.width(10.dp))
                    Column {
                        Txt(d?.let { "Display ${it.index + 1}" } ?: "Screen", Type.Label.copy(fontSize = androidx.compose.ui.unit.TextUnit(13f, androidx.compose.ui.unit.TextUnitType.Sp)), p.text, maxLines = 1)
                        Txt(d?.let { "${it.width}×${it.height}${if (it.primary) " · main" else ""}" } ?: "Pick a display", Type.Caption.copy(letterSpacing = androidx.compose.ui.unit.TextUnit(0f, androidx.compose.ui.unit.TextUnitType.Sp)), p.textDim, maxLines = 1)
                    }
                    Box(Modifier.width(10.dp))
                    Chevron(p.textDim)
                }
            }
            Box(Modifier.weight(1f))
            if (fps > 0 && connected) {
                Glass { Txt("$fps fps", Type.Label, p.textDim, Modifier.padding(horizontal = 12.dp, vertical = 10.dp)) }
                Box(Modifier.width(8.dp))
            }
            if (vp.zoom > 1.02f) {
                Glass(onClick = { vp.reset() }) {
                    Txt("${"%.1f".format(vp.zoom)}×", Type.Label, p.accent, Modifier.padding(horizontal = 14.dp, vertical = 10.dp))
                }
            }
        }

        // Tool strip above the tab bar.
        if (connected && allowed && vp.display != null) {
            ToolStrip(
                drive, { drive = it }, clickType, { clickType = it },
                onFit = { vp.reset(); tick() },
                onZoom = { f ->
                    val c = Offset(vp.box.width / 2f, vp.box.height / 2f)
                    zoomAround(vp, c, vp.zoom * f)
                    tick()
                },
                modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 108.dp),
            )
        }

        DisplaySheet(
            visible = sheet, displays = displays, selected = selected, frame = frame, quality = quality,
            onSelect = { selected = it; sheet = false; tick() },
            onQuality = { quality = it },
            onClose = { sheet = false },
        )
    }
}

// ------------------------------------------------------------------------------------------
// Drawing
// ------------------------------------------------------------------------------------------

@Composable
private fun Viewer(vp: Viewport, frame: ScreenFrame?, cursor: app.moush.remote.data.RemoteCursor?, aim: Offset?) {
    val p = LocalPalette.current
    val image = remember(frame?.seq) { frame?.bitmap?.asImageBitmap() }
    val pulse by animateFloatAsState(if (aim != null) 1f else 0f, Motion.fast(), label = "aim")
    Canvas(Modifier.fillMaxSize()) {
        val d = vp.display
        if (d == null || frame == null || image == null || frame.display != d.index) return@Canvas
        // The frame shows crop (cropX, cropY, cropW, cropH) of the display: place it accordingly.
        val cw = if (frame.cropW > 0) frame.cropW else d.width
        val ch = if (frame.cropH > 0) frame.cropH else d.height
        val tl = vp.toContainer(frame.cropX.toFloat(), frame.cropY.toFloat())
        val s = vp.scale
        drawImage(
            image,
            srcSize = androidx.compose.ui.unit.IntSize(image.width, image.height),
            dstOffset = IntOffset(tl.x.roundToInt(), tl.y.roundToInt()),
            dstSize = IntSize((cw * s).roundToInt().coerceAtLeast(1), (ch * s).roundToInt().coerceAtLeast(1)),
            filterQuality = FilterQuality.Medium,
        )
        // The computer's own cursor, drawn on top (the capture does not include it).
        cursor?.let {
            val c = vp.toContainer(it.x.toFloat(), it.y.toFloat())
            val u = 1.dp.toPx()
            val arrow = Path().apply {
                moveTo(c.x, c.y)
                lineTo(c.x, c.y + 17 * u)
                lineTo(c.x + 4.5f * u, c.y + 13 * u)
                lineTo(c.x + 8 * u, c.y + 20 * u)
                lineTo(c.x + 10.5f * u, c.y + 19 * u)
                lineTo(c.x + 7 * u, c.y + 12 * u)
                lineTo(c.x + 12.5f * u, c.y + 12 * u)
                close()
            }
            if (pulse > 0.01f) drawCircle(p.accent.copy(alpha = 0.22f * pulse), 26.dp.toPx(), c)
            drawPath(arrow, Color.Black.copy(alpha = 0.55f), style = Stroke(3.2f * u))
            drawPath(arrow, Color.White)
        }
    }
}

/** A magnifier above the finger while aiming, so a thumb can hit a 10-pixel target. */
@Composable
private fun Loupe(vp: Viewport, frame: ScreenFrame?, aim: Offset?) {
    val p = LocalPalette.current
    val a by animateFloatAsState(if (aim != null) 1f else 0f, Motion.fast(), label = "loupe")
    val last = remember { mutableStateOf(Offset.Zero) }
    if (aim != null) last.value = aim
    if (a < 0.01f || frame == null) return
    val image = remember(frame.seq) { frame.bitmap.asImageBitmap() }
    Canvas(Modifier.fillMaxSize().graphicsLayer { alpha = a }) {
        val pos = last.value
        val r = 58.dp.toPx()
        val above = pos.y > r * 2.8f
        val center = Offset(pos.x.coerceIn(r + 8.dp.toPx(), size.width - r - 8.dp.toPx()), if (above) pos.y - r * 1.9f else pos.y + r * 1.9f)
        val d = vp.display ?: return@Canvas
        val local = vp.toDisplay(pos)
        val cw = (if (frame.cropW > 0) frame.cropW else d.width).toFloat()
        val ch = (if (frame.cropH > 0) frame.cropH else d.height).toFloat()
        val kx = image.width / cw
        val ky = image.height / ch
        // Source: the area of the picture around the finger, magnified 3×.
        val srcW = (2 * r / (vp.scale * 3f))
        val sx = ((local.x - frame.cropX) - srcW / 2) * kx
        val sy = ((local.y - frame.cropY) - srcW / 2) * ky
        val circle = Path().apply { addOval(androidx.compose.ui.geometry.Rect(center, r)) }
        clipPath(circle) {
            drawRect(Color.Black, topLeft = center - Offset(r, r), size = Size(2 * r, 2 * r))
            drawImage(
                image,
                srcOffset = androidx.compose.ui.unit.IntOffset(sx.roundToInt(), sy.roundToInt()),
                srcSize = IntSize((srcW * kx).roundToInt().coerceAtLeast(1), (srcW * ky).roundToInt().coerceAtLeast(1)),
                dstOffset = IntOffset((center.x - r).roundToInt(), (center.y - r).roundToInt()),
                dstSize = IntSize((2 * r).roundToInt(), (2 * r).roundToInt()),
                filterQuality = FilterQuality.Low,
            )
        }
        drawCircle(p.surface.copy(alpha = 0.9f), r, center, style = Stroke(4.dp.toPx()))
        drawCircle(p.accent, r, center, style = Stroke(1.5.dp.toPx()))
        val arm = 9.dp.toPx()
        drawLine(Color.White, center - Offset(arm, 0f), center + Offset(arm, 0f), 1.6.dp.toPx())
        drawLine(Color.White, center - Offset(0f, arm), center + Offset(0f, arm), 1.6.dp.toPx())
        drawCircle(p.accentAlt, 2.5.dp.toPx(), center)
    }
}

/** Where you are when zoomed in. */
@Composable
private fun MiniMap(vp: Viewport, modifier: Modifier) {
    val p = LocalPalette.current
    val a by animateFloatAsState(if (vp.zoom > 1.05f) 1f else 0f, Motion.base(), label = "map")
    val d = vp.display ?: return
    if (a < 0.01f) return
    val w = 92.dp
    val h = w * (d.height.toFloat() / d.width)
    Box(modifier.graphicsLayer { alpha = a }) {
        Canvas(Modifier.size(w, h)) {
            val r = 8.dp.toPx()
            drawRoundRect(p.surface.copy(alpha = 0.82f), cornerRadius = androidx.compose.ui.geometry.CornerRadius(r))
            val k = size.width / d.width
            val vw = min(d.width.toFloat(), vp.box.width / vp.scale)
            val vh = min(d.height.toFloat(), vp.box.height / vp.scale)
            val tl = Offset((vp.cx - vw / 2) * k, (vp.cy - vh / 2) * k)
            drawRoundRect(p.accent.copy(alpha = 0.28f), tl, Size(vw * k, vh * k), androidx.compose.ui.geometry.CornerRadius(3.dp.toPx()))
            drawRoundRect(p.accent, tl, Size(vw * k, vh * k), androidx.compose.ui.geometry.CornerRadius(3.dp.toPx()), Stroke(1.5.dp.toPx()))
        }
    }
}

@Composable
private fun Waiting(listing: Boolean) {
    val p = LocalPalette.current
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            LinkAnimationSmall()
            Box(Modifier.height(16.dp))
            Txt(if (listing) "Looking for displays…" else "Opening the screen…", Type.Body, p.textDim)
        }
    }
}

@Composable
private fun LinkAnimationSmall() {
    val p = LocalPalette.current
    val inf = androidx.compose.animation.core.rememberInfiniteTransition(label = "wait")
    val t by inf.animateFloat(0f, 1f, androidx.compose.animation.core.infiniteRepeatable(androidx.compose.animation.core.tween(1600, easing = androidx.compose.animation.core.LinearEasing)), label = "t")
    Canvas(Modifier.size(70.dp)) {
        val c = center
        for (k in 0..1) {
            val ph = (t + k * 0.5f) % 1f
            drawCircle(p.accent.copy(alpha = (1 - ph) * 0.4f), 14.dp.toPx() + 20.dp.toPx() * ph, c, style = Stroke(2.dp.toPx()))
        }
        drawCircle(p.accent, 6.dp.toPx(), c)
    }
}

@Composable
private fun StatusOverlay(connected: Boolean, allowed: Boolean, error: String?, onOpenLink: () -> Unit) {
    val p = LocalPalette.current
    val message: Pair<String, String>? = when {
        !connected -> "Not connected" to "Connect to a computer to see its screen."
        !allowed -> "Screen viewing is off" to "Turn on “Screen viewing” for this phone in Moush › Devices on the computer."
        error != null -> "Screen unavailable" to error
        else -> null
    }
    val a by animateFloatAsState(if (message != null) 1f else 0f, Motion.base(), label = "status")
    if (a < 0.01f) return
    Box(Modifier.fillMaxSize().graphicsLayer { alpha = a }.background(p.bg.copy(alpha = 0.86f)), contentAlignment = Alignment.Center) {
        Column(Modifier.padding(36.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            MIcon(R.drawable.ic_monitor, tint = p.textDim, size = 44.dp)
            Box(Modifier.height(16.dp))
            Txt(message?.first ?: "", Type.Title)
            Box(Modifier.height(8.dp))
            Txt(message?.second ?: "", Type.Body, p.textDim, maxLines = 4)
        }
    }
}

// ------------------------------------------------------------------------------------------
// Chrome
// ------------------------------------------------------------------------------------------

@Composable
private fun Glass(modifier: Modifier = Modifier, onClick: (() -> Unit)? = null, content: @Composable () -> Unit) {
    val p = LocalPalette.current
    val shape = CircleShape
    val m = modifier.clip(shape).background(p.surfaceHigh.copy(alpha = 0.92f), shape).let { if (onClick != null) it.pressable(onClick = onClick) else it }
    Box(m) { content() }
}

@Composable
private fun Chevron(color: Color) {
    Canvas(Modifier.size(14.dp)) {
        val s = 1.8.dp.toPx()
        val w = size.width
        val h = size.height
        drawLine(color, Offset(w * 0.15f, h * 0.38f), Offset(w * 0.5f, h * 0.68f), s, androidx.compose.ui.graphics.StrokeCap.Round)
        drawLine(color, Offset(w * 0.85f, h * 0.38f), Offset(w * 0.5f, h * 0.68f), s, androidx.compose.ui.graphics.StrokeCap.Round)
    }
}

@Composable
private fun ToolStrip(
    drive: Drive, onDrive: (Drive) -> Unit,
    click: ClickType, onClick: (ClickType) -> Unit,
    onFit: () -> Unit, onZoom: (Float) -> Unit,
    modifier: Modifier,
) {
    val p = LocalPalette.current
    Row(
        modifier.clip(RoundedCornerShape(24.dp)).background(p.surfaceHigh.copy(alpha = 0.94f)).padding(5.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Pill("Touch", drive == Drive.Touch) { onDrive(Drive.Touch) }
        Pill("Pad", drive == Drive.Pad) { onDrive(Drive.Pad) }
        Box(Modifier.width(1.dp).height(20.dp).background(p.hairline))
        Pill("Click", click == ClickType.Left) { onClick(ClickType.Left) }
        Pill("Right", click == ClickType.Right) { onClick(ClickType.Right) }
        Pill("Drag", click == ClickType.Drag) { onClick(ClickType.Drag) }
        Box(Modifier.width(1.dp).height(20.dp).background(p.hairline))
        IconPill(R.drawable.ic_fit, onFit)
        IconPill(R.drawable.ic_zoom, { onZoom(1.6f) })
    }
}

@Composable
private fun Pill(label: String, on: Boolean, onClick: () -> Unit) {
    val p = LocalPalette.current
    val bg by androidx.compose.animation.animateColorAsState(if (on) p.accent else Color.Transparent, Motion.base(), label = "pill-bg")
    val fg by androidx.compose.animation.animateColorAsState(if (on) p.onAccent else p.textDim, Motion.base(), label = "pill-fg")
    Box(Modifier.clip(RoundedCornerShape(18.dp)).background(bg).pressable(onClick = onClick).padding(horizontal = 12.dp, vertical = 9.dp), contentAlignment = Alignment.Center) {
        Txt(label, Type.Label, fg)
    }
}

@Composable
private fun IconPill(icon: Int, onClick: () -> Unit) {
    val p = LocalPalette.current
    Box(Modifier.size(38.dp).clip(CircleShape).pressable(onClick = onClick), contentAlignment = Alignment.Center) {
        MIcon(icon, tint = p.text, size = 20.dp)
    }
}

// ------------------------------------------------------------------------------------------
// Display picker sheet
// ------------------------------------------------------------------------------------------

@Composable
private fun DisplaySheet(
    visible: Boolean,
    displays: List<RemoteDisplay>,
    selected: Int,
    frame: ScreenFrame?,
    quality: Quality,
    onSelect: (Int) -> Unit,
    onQuality: (Quality) -> Unit,
    onClose: () -> Unit,
) {
    val p = LocalPalette.current
    val scrim by animateFloatAsState(if (visible) 0.55f else 0f, Motion.base(), label = "scrim")
    if (scrim < 0.005f && !visible) return
    Box(Modifier.fillMaxSize()) {
        Box(
            Modifier.fillMaxSize().background(Color.Black.copy(alpha = scrim))
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = onClose),
        )
        AnimatedVisibility(
            visible,
            modifier = Modifier.align(Alignment.BottomCenter),
            enter = slideInVertically(androidx.compose.animation.core.tween(Motion.Slow, easing = Motion.Smooth)) { it } + fadeIn(androidx.compose.animation.core.tween(Motion.Base)),
            exit = slideOutVertically(androidx.compose.animation.core.tween(Motion.Base, easing = Motion.Smooth)) { it } + fadeOut(androidx.compose.animation.core.tween(Motion.Fast)),
        ) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(topStart = 34.dp, topEnd = 34.dp))
                    .background(p.surface)
                    .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { }
                    .navigationBarsPadding()
                    .padding(horizontal = 22.dp, vertical = 14.dp),
            ) {
                Box(Modifier.align(Alignment.CenterHorizontally).size(40.dp, 4.dp).clip(CircleShape).background(p.surfaceTop))
                Box(Modifier.height(18.dp))
                Txt("Displays", Type.Title)
                Txt("Tap a monitor to look at it. The map matches how they sit on your desk.", Type.Label, p.textDim, Modifier.padding(top = 4.dp, bottom = 16.dp), maxLines = 2)
                MonitorMap(displays, selected, frame, onSelect)
                Box(Modifier.height(22.dp))
                Txt("QUALITY", Type.Caption, p.textFaint, Modifier.padding(bottom = 10.dp))
                Segmented(Quality.entries.map { it to it.label }, quality, onQuality)
                Box(Modifier.height(10.dp))
                Txt(
                    when (quality) {
                        Quality.Saver -> "Lowest data use. Good on a weak Wi-Fi signal."
                        Quality.Balanced -> "A smooth picture for everyday use."
                        Quality.Sharp -> "Crisp text and detail. Uses more data."
                    },
                    Type.Label, p.textDim,
                )
                Box(Modifier.height(10.dp))
            }
        }
    }
}

/** The monitors laid out as they are on the desk; the chosen one shows its live picture. */
@Composable
private fun MonitorMap(displays: List<RemoteDisplay>, selected: Int, frame: ScreenFrame?, onSelect: (Int) -> Unit) {
    val p = LocalPalette.current
    if (displays.isEmpty()) {
        Box(Modifier.fillMaxWidth().height(120.dp), contentAlignment = Alignment.Center) { Txt("No displays found.", Type.Body, p.textDim) }
        return
    }
    val minX = displays.minOf { it.x }
    val minY = displays.minOf { it.y }
    val maxX = displays.maxOf { it.x + it.width }
    val maxY = displays.maxOf { it.y + it.height }
    val bw = (maxX - minX).toFloat()
    val bh = (maxY - minY).toFloat()
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val gap = 6.dp
        val k = min(maxWidth.value / bw, 190f / bh)
        val mapH = (bh * k).dp
        val density = LocalDensity.current
        Box(Modifier.fillMaxWidth().height(mapH), contentAlignment = Alignment.Center) {
            Box(Modifier.size((bw * k).dp, mapH)) {
                displays.forEach { d ->
                    val on = d.index == selected
                    val a by animateFloatAsState(if (on) 1f else 0f, Motion.base(), label = "mon-${d.index}")
                    val img = remember(frame?.seq, on) { if (on && frame != null && frame.display == d.index) frame.bitmap.asImageBitmap() else null }
                    Box(
                        Modifier
                            .offset(((d.x - minX) * k).dp + gap / 2, ((d.y - minY) * k).dp + gap / 2)
                            .size((d.width * k).dp - gap, (d.height * k).dp - gap)
                            .clip(RoundedCornerShape(14.dp))
                            .background(lerpColor(p.surfaceHigh, p.accent, a * 0.35f))
                            .pressable { onSelect(d.index) },
                    ) {
                        if (img != null) {
                            Canvas(Modifier.fillMaxSize().graphicsLayer { alpha = 0.55f * a }) {
                                drawImage(img, dstSize = IntSize(size.width.roundToInt(), size.height.roundToInt()), filterQuality = FilterQuality.Low)
                            }
                        }
                        Column(Modifier.align(Alignment.Center).padding(8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                            Box(Modifier.size(30.dp).clip(CircleShape).background(if (on) p.accent else p.surfaceTop), contentAlignment = Alignment.Center) {
                                Txt("${d.index + 1}", Type.Label, if (on) p.onAccent else p.text)
                            }
                            Box(Modifier.height(6.dp))
                            Txt("${d.width}×${d.height}", Type.Caption.copy(letterSpacing = androidx.compose.ui.unit.TextUnit(0f, androidx.compose.ui.unit.TextUnitType.Sp)), if (on) p.text else p.textDim, maxLines = 1)
                            if (d.primary) Txt("MAIN", Type.Caption, p.accentAlt)
                        }
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------------------------------
// Gestures
// ------------------------------------------------------------------------------------------

private fun zoomAround(vp: Viewport, focus: Offset, newZoom: Float) {
    val before = vp.toDisplay(focus)
    vp.zoom = newZoom.coerceIn(1f, 8f)
    // Keep the point under the fingers where it was.
    vp.cx = before.x - (focus.x - vp.box.width / 2f) / vp.scale
    vp.cy = before.y - (focus.y - vp.box.height / 2f) / vp.scale
    vp.clamp()
}

private const val HOLD_MS = 230L

private suspend fun PointerInputScope.gestures(
    vp: Viewport,
    drive: Drive,
    clickType: ClickType,
    settings: () -> app.moush.remote.data.Settings,
    vm: AppViewModel,
    tick: () -> Unit,
    onAim: (Offset?) -> Unit,
) {
    val dp = density
    val slop = 9f * dp
    val session = vm.session

    fun absAt(p: Offset) {
        val d = vp.display ?: return
        val l = vp.toDisplay(p)
        session.moveAbs(d.x + l.x.roundToInt().coerceIn(0, d.width - 1), d.y + l.y.roundToInt().coerceIn(0, d.height - 1))
    }

    awaitEachGesture {
        val first = awaitFirstDown(requireUnconsumed = false)
        val t0 = first.uptimeMillis
        val wallStart = System.currentTimeMillis()
        var lastT = t0
        var travelled = 0f
        var maxPtrs = 1
        var panning = false
        var aiming = false
        var dragging = false
        var remX = 0f
        var remY = 0f
        var scrollY = 0f
        var scrollX = 0f
        var pinchDone = false
        val s = settings()
        var lastPos = first.position

        while (true) {
            val undecided = !panning && !aiming && maxPtrs == 1 && travelled <= slop
            val ev = if (undecided) {
                val left = HOLD_MS - (System.currentTimeMillis() - wallStart)
                if (left > 0) withTimeoutOrNull(left) { awaitPointerEvent() } else null
            } else awaitPointerEvent()

            if (ev == null) {
                // Held still: aim mode. The cursor follows the finger, the loupe helps hit small targets.
                if (drive == Drive.Touch) {
                    aiming = true
                    tick()
                    absAt(lastPos)
                    onAim(lastPos)
                    if (clickType == ClickType.Drag) {
                        session.button(Protocol.BTN_LEFT, true)
                        dragging = true
                    }
                } else {
                    // Pad mode: hold = press and drag, like a physical button.
                    tick()
                    session.button(Protocol.BTN_LEFT, true)
                    dragging = true
                    panning = true
                }
                continue
            }

            val down = ev.changes.filter { it.pressed }
            lastT = ev.changes.maxOf { it.uptimeMillis }
            if (down.isEmpty()) break
            maxPtrs = max(maxPtrs, down.size)
            val cxm = down.map { it.position.x }.average().toFloat()
            val cym = down.map { it.position.y }.average().toFloat()
            val pcx = down.map { it.previousPosition.x }.average().toFloat()
            val pcy = down.map { it.previousPosition.y }.average().toFloat()
            val dx = cxm - pcx
            val dy = cym - pcy
            travelled += hypot(dx, dy)
            lastPos = down[0].position

            if (down.size >= 2) {
                if (aiming) { onAim(null); aiming = false; if (dragging) { session.button(Protocol.BTN_LEFT, false); dragging = false } }
                val a = down[0].position
                val b = down[1].position
                val dist = hypot(a.x - b.x, a.y - b.y)
                val pdist = hypot(down[0].previousPosition.x - down[1].previousPosition.x, down[0].previousPosition.y - down[1].previousPosition.y)
                val dd = dist - pdist
                val pinch = abs(dd) > hypot(dx, dy) * 1.3f && pdist > 0f
                if (pinch || pinchDone) {
                    if (pinch) {
                        pinchDone = true
                        zoomAround(vp, Offset(cxm, cym), vp.zoom * (dist / pdist).coerceIn(0.8f, 1.25f))
                    }
                } else if (travelled > slop) {
                    // Two fingers sliding together scroll the computer.
                    val k = s.scrollSpeed * 5.2f
                    val sign = if (s.naturalScroll) 1f else -1f
                    scrollY += dy / dp * k * sign
                    scrollX += -dx / dp * k * sign
                    if (abs(scrollY) >= 8f) { session.wheel(false, scrollY.toInt()); scrollY -= scrollY.toInt() }
                    if (abs(scrollX) >= 8f) { session.wheel(true, scrollX.toInt()); scrollX -= scrollX.toInt() }
                }
            } else if (aiming) {
                absAt(lastPos)
                onAim(lastPos)
            } else if (travelled > slop || panning) {
                if (drive == Drive.Touch && !dragging) {
                    // One finger drags the picture around.
                    panning = true
                    vp.cx -= dx / vp.scale
                    vp.cy -= dy / vp.scale
                    vp.clamp()
                } else {
                    panning = true
                    val gain = s.sensitivity * 1.6f
                    remX += dx / dp * 2.1f * gain
                    remY += dy / dp * 2.1f * gain
                    val ix = remX.toInt()
                    val iy = remY.toInt()
                    if (ix != 0 || iy != 0) { session.move(ix, iy); remX -= ix; remY -= iy }
                }
            }
            ev.changes.forEach { it.consume() }
        }

        onAim(null)
        when {
            aiming && dragging -> session.button(Protocol.BTN_LEFT, false)
            aiming -> {
                tick()
                when (clickType) {
                    ClickType.Right -> session.click(Protocol.BTN_RIGHT)
                    else -> session.click(Protocol.BTN_LEFT)
                }
            }
            dragging -> session.button(Protocol.BTN_LEFT, false)
            maxPtrs == 1 && !panning && travelled <= slop && lastT - t0 < 260 -> {
                tick()
                if (drive == Drive.Touch) absAt(first.position)
                session.click(if (clickType == ClickType.Right) Protocol.BTN_RIGHT else Protocol.BTN_LEFT)
            }
            maxPtrs == 2 && !pinchDone && travelled <= slop && lastT - t0 < 260 -> { tick(); session.click(Protocol.BTN_RIGHT) }
        }
    }
}
