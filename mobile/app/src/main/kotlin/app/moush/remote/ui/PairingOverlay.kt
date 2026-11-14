package app.moush.remote.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.moush.remote.net.LinkState
import kotlinx.coroutines.delay

/**
 * Full-screen pairing flow: connecting → compare the code → success.
 * Phases cross-fade with a short upward drift; nothing bounces.
 */
@Composable
fun PairingOverlay(link: LinkState, onConfirm: (Boolean) -> Unit, onCancel: () -> Unit, onDone: () -> Unit) {
    val p = LocalPalette.current
    var success by remember { mutableStateOf(false) }
    var hostName by remember { mutableStateOf("") }
    val busy = link is LinkState.Connecting || link is LinkState.Verifying
    var wasBusy by remember { mutableStateOf(false) }
    if (busy) wasBusy = true
    when (link) {
        is LinkState.Verifying -> hostName = link.host
        is LinkState.Connecting -> hostName = link.host
        else -> {}
    }
    // When a pairing attempt ends in "connected", celebrate briefly before leaving.
    LaunchedEffect(link is LinkState.Connected, busy) {
        if (link is LinkState.Connected && wasBusy && !success) {
            success = true
            delay(1500)
            success = false
            wasBusy = false
            onDone()
        }
    }
    val visible = busy || success
    val bgAlpha by androidx.compose.animation.core.animateFloatAsState(if (visible) 1f else 0f, Motion.base(), label = "overlay")
    if (bgAlpha < 0.01f && !visible) return

    Box(
        Modifier
            .fillMaxSize()
            .graphicsLayer { alpha = bgAlpha }
            .background(p.bg)
            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { },
    ) {
        Column(Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding().padding(28.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                val phase = when {
                    success -> 2
                    link is LinkState.Verifying -> 1
                    else -> 0
                }
                AnimatedContent(
                    targetState = phase,
                    transitionSpec = {
                        (fadeIn(tween(Motion.Slow, 120, Motion.Smooth)) + slideInVertically(tween(Motion.Slow, 120, Motion.Smooth)) { it / 14 }) togetherWith
                            (fadeOut(tween(Motion.Fast, easing = Motion.Smooth)) + slideOutVertically(tween(Motion.Base, easing = Motion.Smooth)) { -it / 14 })
                    },
                    label = "phase",
                ) { ph ->
                    when (ph) {
                        0 -> ConnectingPhase(hostName)
                        1 -> VerifyPhase(link as? LinkState.Verifying, hostName, onConfirm)
                        else -> SuccessPhase(hostName)
                    }
                }
            }
            if (!success) {
                MButton("Cancel", onCancel, Modifier.fillMaxWidth(), kind = ButtonKind.Ghost)
            }
        }
    }
}

@Composable
private fun ConnectingPhase(host: String) {
    val p = LocalPalette.current
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        LinkAnimation()
        Box(Modifier.height(36.dp))
        Txt("Connecting", Type.Display)
        Box(Modifier.height(8.dp))
        Txt(if (host.isBlank()) "Looking for your computer…" else "Reaching $host…", Type.Body, p.textDim, Modifier.padding(horizontal = 12.dp))
    }
}

@Composable
private fun VerifyPhase(state: LinkState.Verifying?, host: String, onConfirm: (Boolean) -> Unit) {
    val p = LocalPalette.current
    val code = state?.code ?: "------"
    val confirmed = state?.confirmed == true
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Txt("Confirm it's you", Type.Display)
        Box(Modifier.height(10.dp))
        Txt(
            "Make sure $host shows the same code, then approve the request on the computer.",
            Type.Body, p.textDim, Modifier.padding(horizontal = 8.dp), maxLines = 4,
        )
        Box(Modifier.height(34.dp))
        // Digits rise into place one after another.
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            code.forEachIndexed { i, ch ->
                if (i == 3) Box(Modifier.width(10.dp))
                val a = remember(code) { Animatable(0f) }
                LaunchedEffect(code) {
                    a.snapTo(0f)
                    delay(120L + i * 70L)
                    a.animateTo(1f, tween(480, easing = Motion.Smooth))
                }
                Box(
                    Modifier
                        .size(width = 46.dp, height = 68.dp)
                        .graphicsLayer { alpha = a.value; translationY = (1f - a.value) * 26f }
                        .clip(RoundedCornerShape(16.dp))
                        .background(p.surfaceHigh),
                    contentAlignment = Alignment.Center,
                ) {
                    Txt(ch.toString(), Type.Code.copy(fontSize = androidx.compose.ui.unit.TextUnit(34f, androidx.compose.ui.unit.TextUnitType.Sp), letterSpacing = androidx.compose.ui.unit.TextUnit(0f, androidx.compose.ui.unit.TextUnitType.Sp)), p.text)
                }
            }
        }
        Box(Modifier.height(36.dp))
        if (!confirmed) {
            MButton("The codes match", { onConfirm(true) }, Modifier.fillMaxWidth(), icon = app.moush.remote.R.drawable.ic_check)
        } else {
            WaitingDots("Waiting for $host to approve")
        }
    }
}

@Composable
private fun WaitingDots(label: String) {
    val p = LocalPalette.current
    val inf = rememberInfiniteTransition(label = "dots")
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for (i in 0..2) {
                val a by inf.animateFloat(0.25f, 1f, infiniteRepeatable(tween(700, delayMillis = 0, easing = Motion.InOut), RepeatMode.Reverse, initialStartOffset = androidx.compose.animation.core.StartOffset(i * 180)), label = "d$i")
                Box(Modifier.size(9.dp).graphicsLayer { alpha = a }.clip(CircleShape).background(p.accent))
            }
        }
        Box(Modifier.height(14.dp))
        Txt(label, Type.Label, p.textDim, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp), maxLines = 2)
    }
}

@Composable
private fun SuccessPhase(host: String) {
    val p = LocalPalette.current
    val t = remember { Animatable(0f) }
    LaunchedEffect(Unit) { t.animateTo(1f, tween(900, easing = Motion.InOut)) }
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        SuccessMark(t.value)
        Box(Modifier.height(28.dp))
        Txt("You're connected", Type.Display)
        Box(Modifier.height(8.dp))
        Txt(host, Type.Body, p.textDim)
    }
}
