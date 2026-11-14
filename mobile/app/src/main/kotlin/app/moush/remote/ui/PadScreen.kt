package app.moush.remote.ui

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.view.HapticFeedbackConstants
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.dp
import app.moush.remote.R
import app.moush.remote.data.Settings
import app.moush.remote.input.Keys
import app.moush.remote.input.chord
import app.moush.remote.net.LinkState
import app.moush.remote.net.Protocol
import app.moush.remote.net.Session
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

private enum class PadMode { Touch, Air }

@Composable
fun PadScreen(session: Session, link: LinkState, settings: Settings, onOpenLink: () -> Unit) {
    val p = LocalPalette.current
    val view = LocalView.current
    val connected = link is LinkState.Connected
    var mode by remember { mutableStateOf(PadMode.Touch) }
    var glow by remember { mutableStateOf<Offset?>(null) }
    var touching by remember { mutableStateOf(false) }
    var aiming by remember { mutableStateOf(false) }
    val cfg = rememberUpdatedState(settings)
    val tick = { if (cfg.value.haptics) view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP) }
    val hintAlpha by animateFloatAsState(if (touching || aiming || !connected) 0f else 1f, Motion.base(), label = "hint")
    val glowAlpha by animateFloatAsState(if (touching || aiming) 1f else 0f, Motion.slow(), label = "glow")
    val padEnabled = connected

    if (mode == PadMode.Air) AirMouse(active = aiming && connected, session = session, sensitivity = { cfg.value.sensitivity })

    Column(Modifier.fillMaxSize().statusBarsPadding().padding(horizontal = 18.dp)) {
        Row(Modifier.fillMaxWidth().padding(top = 14.dp, bottom = 14.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Txt("Touchpad", Type.Display)
            }
            StatusChip(link, onOpenLink)
        }

        Box(
            Modifier
                .weight(1f)
                .fillMaxWidth()
                .clip(RoundedCornerShape(34.dp))
                .background(p.surface)
                .drawBehind {
                    // Dot grid.
                    val step = 24.dp.toPx()
                    val r = 1.4.dp.toPx()
                    var y = step / 2
                    while (y < size.height) {
                        var x = step / 2
                        while (x < size.width) {
                            drawCircle(p.textFaint.copy(alpha = 0.22f), r, Offset(x, y))
                            x += step
                        }
                        y += step
                    }
                    // Light that follows the finger.
                    val g = glow
                    if (g != null && glowAlpha > 0.01f) {
                        drawCircle(
                            Brush.radialGradient(listOf(p.accent.copy(alpha = 0.30f * glowAlpha), p.accentAlt.copy(alpha = 0.10f * glowAlpha), androidx.compose.ui.graphics.Color.Transparent), center = g, radius = 190.dp.toPx()),
                            radius = 190.dp.toPx(),
                            center = g,
                        )
                    }
                }
                .then(
                    if (mode == PadMode.Touch && padEnabled) {
                        Modifier.pointerInput(Unit) {
                            touchpad(session, { cfg.value }, tick, { glow = it; touching = it != null })
                        }
                    } else if (mode == PadMode.Air && padEnabled) {
                        Modifier.pointerInput(Unit) {
                            awaitEachGesture {
                                val d = awaitFirstDown(requireUnconsumed = false)
                                aiming = true
                                glow = d.position
                                tick()
                                do {
                                    val e = awaitPointerEvent()
                                    e.changes.firstOrNull()?.let { glow = it.position }
                                    e.changes.forEach { it.consume() }
                                } while (e.changes.any { it.pressed })
                                aiming = false
                                glow = null
                            }
                        }
                    } else Modifier,
                ),
            contentAlignment = Alignment.Center,
        ) {
            Column(
                Modifier.graphicsLayer { alpha = hintAlpha },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                MIcon(if (mode == PadMode.Touch) R.drawable.ic_touchpad else R.drawable.ic_gyro, tint = p.textFaint, size = 44.dp)
                Box(Modifier.height(14.dp))
                Txt(if (mode == PadMode.Touch) "Slide to move" else "Hold and point to aim", Type.Body, p.textDim)
                Box(Modifier.height(4.dp))
                Txt(
                    if (mode == PadMode.Touch) "Tap to click · two fingers to scroll · three to switch" else "Tilt your phone like a laser pointer",
                    Type.Label, p.textFaint,
                    Modifier.padding(horizontal = 36.dp),
                )
            }
            if (!connected) {
                Box(Modifier.fillMaxSize().background(p.bg.copy(alpha = 0.78f)), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        MIcon(R.drawable.ic_laptop, tint = p.textDim, size = 40.dp)
                        Box(Modifier.height(14.dp))
                        Txt("Not connected", Type.Title)
                        Box(Modifier.height(18.dp))
                        MButton("Find a computer", onOpenLink, icon = R.drawable.ic_wifi)
                    }
                }
            }
        }

        Box(Modifier.height(14.dp))
        Segmented(listOf(PadMode.Touch to "Touch", PadMode.Air to "Air mouse"), mode) { mode = it }
        Box(Modifier.height(12.dp))
        ClickBar(session, enabled = connected, tick = tick)
        Box(Modifier.height(132.dp)) // room for the floating tab bar
    }
}

@Composable
private fun StatusChip(link: LinkState, onClick: () -> Unit) {
    val p = LocalPalette.current
    val (label, color) = when (link) {
        is LinkState.Connected -> (link.host + (link.latencyMs?.let { " · ${it} ms" } ?: "")) to p.ok
        is LinkState.Connecting, is LinkState.Verifying -> "Connecting…" to p.accentAlt
        else -> "Offline" to p.textFaint
    }
    Row(
        Modifier.clip(CircleShape).background(p.surfaceHigh).pressable(onClick = onClick).padding(horizontal = 14.dp, vertical = 9.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(8.dp).clip(CircleShape).background(color))
        Box(Modifier.size(8.dp))
        Txt(label, Type.Label, p.text, maxLines = 1)
    }
}

@Composable
private fun ClickBar(session: Session, enabled: Boolean, tick: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(62.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        ClickKey("Left", Modifier.weight(2f), enabled, tick, onDown = { session.button(Protocol.BTN_LEFT, true) }, onUp = { session.button(Protocol.BTN_LEFT, false) })
        ClickKey("Mid", Modifier.weight(0.9f), enabled, tick, onDown = { session.button(Protocol.BTN_MIDDLE, true) }, onUp = { session.button(Protocol.BTN_MIDDLE, false) })
        ClickKey("Right", Modifier.weight(2f), enabled, tick, onDown = { session.button(Protocol.BTN_RIGHT, true) }, onUp = { session.button(Protocol.BTN_RIGHT, false) })
    }
}

@Composable
private fun ClickKey(label: String, modifier: Modifier, enabled: Boolean, tick: () -> Unit, onDown: () -> Unit, onUp: () -> Unit) {
    val p = LocalPalette.current
    var pressed by remember { mutableStateOf(false) }
    val fill by animateFloatAsState(if (pressed) 1f else 0f, Motion.fast(), label = "click")
    val s by animateFloatAsState(if (pressed) 0.97f else 1f, Motion.fast(), label = "click-s")
    Box(
        modifier
            .fillMaxSize()
            .graphicsLayer { scaleX = s; scaleY = s; alpha = if (enabled) 1f else 0.45f }
            .clip(RoundedCornerShape(22.dp))
            .background(lerpColor(p.surfaceHigh, p.surfaceTop, fill))
            .pointerInput(enabled) {
                if (enabled) detectTapGestures(onPress = {
                    pressed = true; tick(); onDown()
                    try { tryAwaitRelease() } finally { pressed = false; onUp() }
                })
            },
        contentAlignment = Alignment.Center,
    ) {
        Txt(label, Type.Label, lerpColor(p.textDim, p.text, fill))
    }
}

// --------------------------------------------------------------------------------------------
// Gesture engine
// --------------------------------------------------------------------------------------------

private suspend fun PointerInputScope.touchpad(
    session: Session,
    cfg: () -> Settings,
    tick: () -> Unit,
    onGlow: (Offset?) -> Unit,
) {
    var lastTapUp = 0L
    val dp = density
    val slop = 9f * dp
    awaitEachGesture {
        val first = awaitFirstDown(requireUnconsumed = false)
        val startT = first.uptimeMillis
        var lastT = startT
        val s = cfg()
        val dragLatched = s.tapToClick && lastTapUp != 0L && startT - lastTapUp < 300
        var dragging = false
        if (dragLatched) {
            session.button(Protocol.BTN_LEFT, true)
            dragging = true
        }
        var maxPointers = 1
        var travelled = 0f
        var remX = 0f
        var remY = 0f
        var scrollY = 0f
        var scrollX = 0f
        var zoomAcc = 0f
        var ctrlHeld = false
        var swipeX = 0f
        var swipeY = 0f
        var swiped = false
        onGlow(first.position)

        while (true) {
            val ev = awaitPointerEvent()
            val down = ev.changes.filter { it.pressed }
            lastT = ev.changes.maxOf { it.uptimeMillis }
            if (down.isEmpty()) break
            maxPointers = max(maxPointers, down.size)
            val cx = down.map { it.position.x }.average().toFloat()
            val cy = down.map { it.position.y }.average().toFloat()
            val pcx = down.map { it.previousPosition.x }.average().toFloat()
            val pcy = down.map { it.previousPosition.y }.average().toFloat()
            val dx = cx - pcx
            val dy = cy - pcy
            val dt = max(1L, ev.changes.maxOf { it.uptimeMillis - it.previousUptimeMillis }).toFloat()
            travelled += hypot(dx, dy)
            onGlow(Offset(cx, cy))

            when (down.size) {
                1 -> if (travelled > slop || dragging || maxPointers > 1) {
                    val speed = hypot(dx, dy) / dp / dt // dp per ms
                    val gain = s.sensitivity * (1.0f + 2.2f * min(speed / 3.2f, 1f).pow(1.35f))
                    remX += dx / dp * 2.1f * gain
                    remY += dy / dp * 2.1f * gain
                    val ix = remX.toInt()
                    val iy = remY.toInt()
                    if (ix != 0 || iy != 0) {
                        session.move(ix, iy)
                        remX -= ix
                        remY -= iy
                    }
                }
                2 -> {
                    val a = down[0].position
                    val b = down[1].position
                    val dist = hypot(a.x - b.x, a.y - b.y)
                    val pa = down[0].previousPosition
                    val pb = down[1].previousPosition
                    val pd = hypot(pa.x - pb.x, pa.y - pb.y)
                    val dd = dist - pd
                    if (travelled > slop) {
                        val pinching = abs(dd) > hypot(dx, dy) * 1.6f && abs(dd) > 0.6f
                        if (pinching) {
                            if (!ctrlHeld) {
                                session.key(Keys.Ctrl.vk, Keys.Ctrl.scan, false, true)
                                ctrlHeld = true
                            }
                            zoomAcc += dd / dp * 5f
                            val z = zoomAcc.toInt()
                            if (abs(z) >= 6) {
                                session.wheel(false, z * 4)
                                zoomAcc = 0f
                            }
                        } else {
                            val k = s.scrollSpeed * 5.2f
                            val sign = if (s.naturalScroll) 1f else -1f
                            scrollY += dy / dp * k * sign
                            scrollX += -dx / dp * k * sign
                            if (abs(scrollY) >= 8f) { session.wheel(false, scrollY.toInt()); scrollY -= scrollY.toInt() }
                            if (abs(scrollX) >= 8f) { session.wheel(true, scrollX.toInt()); scrollX -= scrollX.toInt() }
                        }
                    }
                }
                else -> if (!swiped) {
                    swipeX += dx
                    swipeY += dy
                    val th = 70f * dp
                    if (abs(swipeX) > th || abs(swipeY) > th) {
                        swiped = true
                        tick()
                        if (abs(swipeX) > abs(swipeY)) {
                            // Previous / next virtual desktop.
                            val arrow = if (swipeX > 0) Keys.Left else Keys.Right
                            session.chord(Keys.Ctrl, Keys.Win, arrow)
                        } else if (swipeY < 0) session.chord(Keys.Win, Keys.Tab)
                        else session.chord(Keys.Win, Keys.letter('d'))
                    }
                }
            }
            ev.changes.forEach { it.consume() }
        }

        if (ctrlHeld) session.key(Keys.Ctrl.vk, Keys.Ctrl.scan, false, false)
        if (dragging) {
            session.button(Protocol.BTN_LEFT, false)
        } else if (travelled <= slop && lastT - startT < 260 && !swiped) {
            when (maxPointers) {
                1 -> if (s.tapToClick) {
                    tick()
                    session.click(Protocol.BTN_LEFT)
                    lastTapUp = lastT
                }
                2 -> { tick(); session.click(Protocol.BTN_RIGHT) }
                else -> { tick(); session.click(Protocol.BTN_MIDDLE) }
            }
        }
        if (dragLatched) lastTapUp = 0L
        onGlow(null)
    }
}

// --------------------------------------------------------------------------------------------
// Air mouse (gyroscope)
// --------------------------------------------------------------------------------------------

@Composable
private fun AirMouse(active: Boolean, session: Session, sensitivity: () -> Float) {
    val ctx = LocalContext.current
    DisposableEffect(active) {
        if (!active) return@DisposableEffect onDispose { }
        val sm = ctx.getSystemService(Context.SENSOR_SERVICE) as SensorManager
        val gyro = sm.getDefaultSensor(Sensor.TYPE_GYROSCOPE)
        var last = 0L
        var rx = 0f
        var ry = 0f
        val l = object : SensorEventListener {
            override fun onSensorChanged(e: SensorEvent) {
                if (last != 0L) {
                    val dt = (e.timestamp - last) / 1e9f
                    val k = 1100f * sensitivity()
                    // Held like a remote (screen up, top edge toward the screen): yaw = z, pitch = x.
                    rx += -e.values[2] * dt * k
                    ry += -e.values[0] * dt * k
                    val ix = rx.toInt()
                    val iy = ry.toInt()
                    if (ix != 0 || iy != 0) {
                        session.move(ix, iy)
                        rx -= ix
                        ry -= iy
                    }
                }
                last = e.timestamp
            }

            override fun onAccuracyChanged(s: Sensor?, a: Int) {}
        }
        if (gyro != null) sm.registerListener(l, gyro, SensorManager.SENSOR_DELAY_GAME)
        onDispose { sm.unregisterListener(l) }
    }
}
