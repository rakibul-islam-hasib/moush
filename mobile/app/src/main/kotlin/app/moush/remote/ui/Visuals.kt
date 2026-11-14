package app.moush.remote.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathMeasure
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.moush.remote.R
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sin

/**
 * Discovery radar: soft rings expand from the centre and a faint sweep rotates around it.
 * Computers that answer appear as blips on the rings.
 */
@Composable
fun Radar(blips: List<String>, scanning: Boolean, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    val inf = rememberInfiniteTransition(label = "radar")
    val t by inf.animateFloat(0f, 1f, infiniteRepeatable(tween(3600, easing = LinearEasing)), label = "rings")
    val sweep by inf.animateFloat(0f, 360f, infiniteRepeatable(tween(5200, easing = LinearEasing)), label = "sweep")
    val run = if (scanning) 1f else 0.35f
    Box(modifier.fillMaxWidth().height(290.dp), contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            val c = center
            val maxR = size.minDimension / 2f
            // Static guide rings.
            for (i in 1..3) drawCircle(p.hairline, radius = maxR * i / 3f, center = c, style = Stroke(1.dp.toPx()))
            // Expanding pulses.
            for (k in 0..2) {
                val ph = (t + k / 3f) % 1f
                val eased = 1f - (1f - ph) * (1f - ph)
                drawCircle(p.accent.copy(alpha = (1f - ph) * 0.34f * run), radius = 44.dp.toPx() + (maxR - 44.dp.toPx()) * eased, center = c, style = Stroke(2.dp.toPx()))
            }
            // Sweep.
            if (scanning) {
                val brush = Brush.sweepGradient(
                    0f to Color.Transparent, 0.82f to Color.Transparent, 1f to p.accentAlt.copy(alpha = 0.28f), center = c,
                )
                rotate(sweep, c) {
                    drawCircle(brush, radius = maxR, center = c)
                }
            }
            // Blips.
            blips.forEachIndexed { i, id ->
                val h = id.hashCode()
                val ang = ((h and 0xFFFF) / 65535f) * 2f * PI.toFloat()
                val r = maxR * (0.45f + 0.4f * (((h ushr 16) and 0xFF) / 255f))
                val pos = Offset(c.x + cos(ang) * r, c.y + sin(ang) * r)
                drawCircle(p.accentAlt.copy(alpha = 0.18f), 14.dp.toPx(), pos)
                drawCircle(p.accentAlt, 5.dp.toPx(), pos)
            }
        }
        Box(Modifier.size(84.dp).clip(CircleShape).background(p.surfaceHigh), contentAlignment = Alignment.Center) {
            MIcon(R.drawable.ic_wifi, tint = p.accent, size = 34.dp)
        }
    }
}

/** Phone ↔ computer link: little packets travel along a dashed line while connecting. */
@Composable
fun LinkAnimation(modifier: Modifier = Modifier, size: Dp = 280.dp) {
    val p = LocalPalette.current
    val inf = rememberInfiniteTransition(label = "link")
    val t by inf.animateFloat(0f, 1f, infiniteRepeatable(tween(1500, easing = LinearEasing)), label = "packets")
    val breath by inf.animateFloat(0f, 1f, infiniteRepeatable(tween(1800, easing = Motion.InOut), RepeatMode.Reverse), label = "breath")
    Box(modifier.size(width = size, height = 120.dp), contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            val y = this.size.height / 2f
            val x0 = 52.dp.toPx()
            val x1 = this.size.width - 52.dp.toPx()
            drawLine(p.textFaint.copy(alpha = 0.35f), Offset(x0, y), Offset(x1, y), 2.dp.toPx(), cap = StrokeCap.Round,
                pathEffect = androidx.compose.ui.graphics.PathEffect.dashPathEffect(floatArrayOf(6.dp.toPx(), 9.dp.toPx())))
            for (k in 0..2) {
                val ph = (t + k / 3f) % 1f
                val x = x0 + (x1 - x0) * ph
                val a = 1f - kotlin.math.abs(ph - 0.5f) * 2f
                drawCircle(p.accent.copy(alpha = 0.25f + 0.75f * a), 4.5.dp.toPx(), Offset(x, y))
            }
            val r = 30.dp.toPx() + 3.dp.toPx() * breath
            drawCircle(p.accent.copy(alpha = 0.10f), r + 8.dp.toPx(), Offset(x0, y))
            drawCircle(p.accentAlt.copy(alpha = 0.10f), r + 8.dp.toPx(), Offset(x1, y))
        }
        Box(Modifier.fillMaxSize()) {
            Box(Modifier.align(Alignment.CenterStart).size(104.dp, 104.dp), contentAlignment = Alignment.Center) {
                Box(Modifier.size(60.dp).clip(CircleShape).background(p.surfaceTop), contentAlignment = Alignment.Center) {
                    MIcon(R.drawable.ic_touchpad, tint = p.accent, size = 28.dp)
                }
            }
            Box(Modifier.align(Alignment.CenterEnd).size(104.dp, 104.dp), contentAlignment = Alignment.Center) {
                Box(Modifier.size(60.dp).clip(CircleShape).background(p.surfaceTop), contentAlignment = Alignment.Center) {
                    MIcon(R.drawable.ic_laptop, tint = p.accentAlt, size = 28.dp)
                }
            }
        }
    }
}

/** A ring that closes, then a check that draws itself. [progress] 0..1. */
@Composable
fun SuccessMark(progress: Float, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    Canvas(modifier.size(112.dp)) {
        val s = size.minDimension
        val stroke = 5.dp.toPx()
        val ringP = (progress / 0.55f).coerceIn(0f, 1f)
        val checkP = ((progress - 0.45f) / 0.55f).coerceIn(0f, 1f)
        drawCircle(p.ok.copy(alpha = 0.12f * ringP), radius = s / 2f - stroke)
        drawArc(
            color = p.ok, startAngle = -90f, sweepAngle = 360f * ringP, useCenter = false,
            topLeft = Offset(stroke, stroke), size = Size(s - stroke * 2, s - stroke * 2),
            style = Stroke(stroke, cap = StrokeCap.Round),
        )
        val path = Path().apply {
            moveTo(s * 0.30f, s * 0.53f)
            lineTo(s * 0.45f, s * 0.67f)
            lineTo(s * 0.72f, s * 0.37f)
        }
        val seg = Path()
        val pm = PathMeasure().apply { setPath(path, false) }
        pm.getSegment(0f, pm.length * checkP, seg, true)
        drawPath(seg, p.ok, style = Stroke(stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}
