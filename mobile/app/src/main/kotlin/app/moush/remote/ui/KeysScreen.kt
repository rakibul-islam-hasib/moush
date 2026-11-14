package app.moush.remote.ui

import android.content.ClipboardManager
import android.content.Context
import android.view.HapticFeedbackConstants
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import app.moush.remote.R
import app.moush.remote.data.Settings
import app.moush.remote.input.Key
import app.moush.remote.input.Keys
import app.moush.remote.input.press
import app.moush.remote.input.typeText
import app.moush.remote.net.LinkState
import app.moush.remote.net.Session

@Composable
fun KeysScreen(session: Session, link: LinkState, settings: Settings) {
    val p = LocalPalette.current
    val view = LocalView.current
    val ctx = LocalContext.current
    val connected = link is LinkState.Connected
    val focus = remember { FocusRequester() }
    val kb = LocalSoftwareKeyboardController.current
    var field by remember { mutableStateOf("") }
    var ctrl by remember { mutableStateOf(false) }
    var alt by remember { mutableStateOf(false) }
    var shift by remember { mutableStateOf(false) }
    var win by remember { mutableStateOf(false) }
    val tick = { if (settings.haptics) view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP) }

    /** Wraps [action] in the armed modifiers, then disarms them (one-shot, like a phone's shift). */
    fun withMods(action: () -> Unit) {
        val held = buildList {
            if (ctrl) add(Keys.Ctrl)
            if (alt) add(Keys.Alt)
            if (shift) add(Keys.Shift)
            if (win) add(Keys.Win)
        }
        held.forEach { session.key(it.vk, it.scan, it.extended, true) }
        action()
        held.reversed().forEach { session.key(it.vk, it.scan, it.extended, false) }
        ctrl = false; alt = false; shift = false; win = false
    }

    fun tap(k: Key) {
        if (!connected) return
        tick()
        withMods { session.press(k) }
    }

    Column(Modifier.fillMaxSize().statusBarsPadding().padding(horizontal = 18.dp).verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(top = 14.dp, bottom = 14.dp), verticalAlignment = Alignment.CenterVertically) {
            Txt("Keyboard", Type.Display, modifier = Modifier.weight(1f))
        }

        // Live typing field: every character you type goes straight to the computer.
        Tile(Modifier.fillMaxWidth(), level = 0, shape = RoundedCornerShape(28.dp)) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 6.dp).height(64.dp), verticalAlignment = Alignment.CenterVertically) {
                MIcon(R.drawable.ic_keyboard, tint = p.textDim, size = 24.dp)
                Box(Modifier.width(14.dp))
                Box(Modifier.weight(1f)) {
                    if (field.isEmpty()) Txt(if (connected) "Tap and type — it appears on your computer" else "Connect to a computer first", Type.Body, p.textFaint)
                    BasicTextField(
                        value = field,
                        onValueChange = { new ->
                            if (!connected) return@BasicTextField
                            var cp = 0
                            while (cp < field.length && cp < new.length && field[cp] == new[cp]) cp++
                            repeat(field.length - cp) { withMods { session.press(Keys.Backspace) } }
                            val add = new.substring(cp)
                            if (add.isNotEmpty()) withMods { session.typeText(add) }
                            field = if (new.length > 80) "" else new
                        },
                        singleLine = true,
                        textStyle = Type.Body.copy(color = p.text),
                        cursorBrush = SolidColor(p.accent),
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send, capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                        keyboardActions = KeyboardActions(onSend = { tap(Keys.Enter); field = "" }),
                        modifier = Modifier.fillMaxWidth().focusRequester(focus),
                    )
                }
            }
        }

        Box(Modifier.height(14.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            ModKey("Ctrl", ctrl, Modifier.weight(1f)) { ctrl = !ctrl; tick() }
            ModKey("Alt", alt, Modifier.weight(1f)) { alt = !alt; tick() }
            ModKey("Shift", shift, Modifier.weight(1f)) { shift = !shift; tick() }
            ModKey("Win", win, Modifier.weight(1f)) { win = !win; tick() }
        }

        Box(Modifier.height(10.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            CapKey("Esc", Modifier.weight(1f)) { tap(Keys.Esc) }
            CapKey("Tab", Modifier.weight(1f)) { tap(Keys.Tab) }
            CapKey("Del", Modifier.weight(1f)) { tap(Keys.Delete) }
            CapKey("⌫", Modifier.weight(1f), repeat = true) { tap(Keys.Backspace) }
            CapKey("Enter", Modifier.weight(1.5f), accent = true) { tap(Keys.Enter) }
        }

        Box(Modifier.height(18.dp))
        SectionLabel("Navigate")
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1.6f), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Box(Modifier.weight(1f)); CapKey("↑", Modifier.weight(1f), repeat = true) { tap(Keys.Up) }; Box(Modifier.weight(1f))
                }
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    CapKey("←", Modifier.weight(1f), repeat = true) { tap(Keys.Left) }
                    CapKey("↓", Modifier.weight(1f), repeat = true) { tap(Keys.Down) }
                    CapKey("→", Modifier.weight(1f), repeat = true) { tap(Keys.Right) }
                }
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    CapKey("Home", Modifier.weight(1f)) { tap(Keys.Home) }
                    CapKey("End", Modifier.weight(1f)) { tap(Keys.End) }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    CapKey("PgUp", Modifier.weight(1f)) { tap(Keys.PageUp) }
                    CapKey("PgDn", Modifier.weight(1f)) { tap(Keys.PageDown) }
                }
            }
        }

        Box(Modifier.height(18.dp))
        SectionLabel("Function keys")
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            for (n in 1..12) CapKey("F$n", Modifier.width(62.dp)) { tap(Keys.f(n)) }
        }

        Box(Modifier.height(18.dp))
        MButton(
            "Type my clipboard",
            onClick = {
                val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                val text = cm.primaryClip?.getItemAt(0)?.coerceToText(ctx)?.toString().orEmpty()
                if (text.isNotEmpty() && connected) { tick(); session.typeText(text.take(2000)) }
            },
            modifier = Modifier.fillMaxWidth(),
            kind = ButtonKind.Tonal,
            icon = R.drawable.ic_paste,
            enabled = connected,
        )
        Box(Modifier.height(118.dp))
    }
}

@Composable
private fun ModKey(label: String, on: Boolean, modifier: Modifier, onClick: () -> Unit) {
    val p = LocalPalette.current
    val bg by animateColorAsState(if (on) p.accent else p.surfaceHigh, Motion.base(), label = "mod-bg")
    val fg by animateColorAsState(if (on) p.onAccent else p.textDim, Motion.base(), label = "mod-fg")
    Box(modifier.height(46.dp).clip(RoundedCornerShape(16.dp)).background(bg).pressable(onClick = onClick), contentAlignment = Alignment.Center) {
        Txt(label, Type.Label, fg)
    }
}

@Composable
private fun CapKey(label: String, modifier: Modifier, accent: Boolean = false, repeat: Boolean = false, onClick: () -> Unit) {
    val p = LocalPalette.current
    val bg = if (accent) p.accent.copy(alpha = 0.18f) else p.surfaceHigh
    val fg = if (accent) p.accent else p.text
    var held by remember { mutableStateOf(false) }
    val s by animateFloatAsState(if (held) 0.96f else 1f, Motion.fast(), label = "cap")
    val click = rememberUpdatedState(onClick)
    Box(
        modifier
            .height(52.dp)
            .graphicsLayer { scaleX = s; scaleY = s }
            .clip(RoundedCornerShape(16.dp))
            .background(if (held) lerpColor(bg, p.surfaceTop, 0.6f) else bg)
            .pointerInput(repeat) {
                detectTapGestures(onPress = {
                    held = true
                    click.value()
                    coroutineScope {
                        val job = if (repeat) launch {
                            delay(380)
                            while (true) { click.value(); delay(55) }
                        } else null
                        tryAwaitRelease()
                        job?.cancel()
                    }
                    held = false
                })
            },
        contentAlignment = Alignment.Center,
    ) {
        Txt(label, Type.Label.copy(fontSize = 15.sp), fg)
    }
}
