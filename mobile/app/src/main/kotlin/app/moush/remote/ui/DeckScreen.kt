package app.moush.remote.ui

import android.view.HapticFeedbackConstants
import androidx.annotation.DrawableRes
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.dp
import app.moush.remote.R
import app.moush.remote.data.Settings
import app.moush.remote.input.Key
import app.moush.remote.input.Keys
import app.moush.remote.input.chord
import app.moush.remote.input.press
import app.moush.remote.net.LinkState
import app.moush.remote.net.Session
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private class Shortcut(val label: String, @DrawableRes val icon: Int, val keys: List<Key>)

private val shortcuts = listOf(
    Shortcut("Copy", R.drawable.ic_copy, listOf(Keys.Ctrl, Keys.letter('c'))),
    Shortcut("Paste", R.drawable.ic_paste, listOf(Keys.Ctrl, Keys.letter('v'))),
    Shortcut("Undo", R.drawable.ic_undo, listOf(Keys.Ctrl, Keys.letter('z'))),
    Shortcut("Snip", R.drawable.ic_screenshot, listOf(Keys.Win, Keys.Shift, Keys.letter('s'))),
    Shortcut("Desktop", R.drawable.ic_desktop, listOf(Keys.Win, Keys.letter('d'))),
    Shortcut("Lock", R.drawable.ic_lock, listOf(Keys.Win, Keys.letter('l'))),
)

@Composable
fun DeckScreen(session: Session, link: LinkState, settings: Settings) {
    val p = LocalPalette.current
    val view = LocalView.current
    val connected = link is LinkState.Connected
    val tick = { if (settings.haptics) view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP) }
    var playing by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize().statusBarsPadding().padding(horizontal = 18.dp).verticalScroll(rememberScrollState())) {
        Txt("Deck", Type.Display, modifier = Modifier.padding(top = 14.dp, bottom = 14.dp))

        // Media transport
        Tile(Modifier.fillMaxWidth(), level = 1, shape = RoundedCornerShape(30.dp)) {
            Column(Modifier.padding(20.dp)) {
                Txt("MEDIA", Type.Caption, p.textFaint)
                Box(Modifier.height(14.dp))
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
                    RoundKey(R.drawable.ic_prev, 56, connected) { tick(); session.press(Keys.PrevTrack) }
                    RoundKey(if (playing) R.drawable.ic_pause else R.drawable.ic_play, 76, connected, primary = true) {
                        tick(); playing = !playing; session.press(Keys.PlayPause)
                    }
                    RoundKey(R.drawable.ic_prev, 56, connected, mirror = true) { tick(); session.press(Keys.NextTrack) }
                }
                Box(Modifier.height(16.dp))
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    WideKey(R.drawable.ic_vol_down, "Quieter", Modifier.weight(1f), connected, repeat = true) { session.press(Keys.VolDown) }
                    WideKey(R.drawable.ic_mute, "Mute", Modifier.weight(1f), connected) { tick(); session.press(Keys.Mute) }
                    WideKey(R.drawable.ic_vol_up, "Louder", Modifier.weight(1f), connected, repeat = true) { session.press(Keys.VolUp) }
                }
            }
        }

        Box(Modifier.height(18.dp))
        SectionLabel("Shortcuts")
        shortcuts.chunked(3).forEachIndexed { row, items ->
            if (row > 0) Box(Modifier.height(10.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                items.forEach { s ->
                    Tile(Modifier.weight(1f).height(92.dp), level = 1, shape = RoundedCornerShape(24.dp), onClick = if (connected) ({ tick(); session.chord(*s.keys.toTypedArray()) }) else null) {
                        Column(Modifier.fillMaxSize().graphicsLayer { alpha = if (connected) 1f else 0.4f }, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
                            MIcon(s.icon, tint = p.accent, size = 26.dp)
                            Box(Modifier.height(8.dp))
                            Txt(s.label, Type.Label, p.text)
                        }
                    }
                }
            }
        }

        Box(Modifier.height(18.dp))
        SectionLabel("Presenter")
        Tile(Modifier.fillMaxWidth(), level = 1, shape = RoundedCornerShape(30.dp)) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    BigKey("Back", Modifier.weight(1f), connected) { tick(); session.press(Keys.Left) }
                    BigKey("Next", Modifier.weight(1f), connected, accent = true) { tick(); session.press(Keys.Right) }
                }
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    WideKey(R.drawable.ic_present, "Start", Modifier.weight(1f), connected) { tick(); session.press(Keys.F5) }
                    WideKey(R.drawable.ic_desktop, "Black", Modifier.weight(1f), connected) { tick(); session.press(Keys.letter('b')) }
                    WideKey(R.drawable.ic_lock, "End", Modifier.weight(1f), connected) { tick(); session.press(Keys.Esc) }
                }
            }
        }
        Box(Modifier.height(118.dp))
    }
}

@Composable
private fun RoundKey(@DrawableRes icon: Int, size: Int, enabled: Boolean, primary: Boolean = false, mirror: Boolean = false, onClick: () -> Unit) {
    val p = LocalPalette.current
    Box(
        Modifier
            .size(size.dp)
            .graphicsLayer { alpha = if (enabled) 1f else 0.4f }
            .clip(CircleShape)
            .background(if (primary) p.accent else p.surfaceTop)
            .pressable(enabled = enabled, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        MIcon(icon, tint = if (primary) p.onAccent else p.text, size = 26.dp, mirror = mirror)
    }
}

@Composable
private fun BigKey(label: String, modifier: Modifier, enabled: Boolean, accent: Boolean = false, onClick: () -> Unit) {
    val p = LocalPalette.current
    Box(
        modifier.height(64.dp).clip(RoundedCornerShape(20.dp))
            .background(if (accent) p.accent else p.surfaceTop)
            .pressable(enabled = enabled, onClick = onClick)
            .graphicsLayer { alpha = if (enabled) 1f else 0.4f },
        contentAlignment = Alignment.Center,
    ) {
        Txt(label, Type.Title, if (accent) p.onAccent else p.text)
    }
}

/** Icon + caption key; with [repeat] it keeps firing while held (volume). */
@Composable
private fun WideKey(@DrawableRes icon: Int, label: String, modifier: Modifier, enabled: Boolean, repeat: Boolean = false, onClick: () -> Unit) {
    val p = LocalPalette.current
    var held by remember { mutableStateOf(false) }
    val s by animateFloatAsState(if (held) 0.96f else 1f, Motion.fast(), label = "wide")
    val click = rememberUpdatedState(onClick)
    Column(
        modifier
            .height(64.dp)
            .graphicsLayer { scaleX = s; scaleY = s; alpha = if (enabled) 1f else 0.4f }
            .clip(RoundedCornerShape(20.dp))
            .background(if (held) p.surfaceTop.copy(alpha = 0.7f) else p.surfaceTop)
            .pointerInput(enabled, repeat) {
                if (enabled) detectTapGestures(onPress = {
                    held = true
                    click.value()
                    coroutineScope {
                        val job = if (repeat) launch { delay(380); while (true) { click.value(); delay(70) } } else null
                        tryAwaitRelease()
                        job?.cancel()
                    }
                    held = false
                })
            },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        MIcon(icon, tint = p.text, size = 22.dp)
        Box(Modifier.height(4.dp))
        Txt(label, Type.Caption.copy(letterSpacing = androidx.compose.ui.unit.TextUnit(0.2f, androidx.compose.ui.unit.TextUnitType.Sp)), p.textDim)
    }
}
