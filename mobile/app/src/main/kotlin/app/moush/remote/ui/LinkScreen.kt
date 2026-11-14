package app.moush.remote.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import app.moush.remote.AppViewModel
import app.moush.remote.R
import app.moush.remote.data.ThemeMode
import app.moush.remote.net.FoundHost
import app.moush.remote.net.LinkState

@Composable
fun LinkScreen(vm: AppViewModel) {
    val p = LocalPalette.current
    val link by vm.link.collectAsState()
    val found by vm.found.collectAsState()
    val scanning by vm.scanning.collectAsState()
    val paired by vm.paired.collectAsState()
    val s by vm.settings.collectAsState()
    var manual by remember { mutableStateOf(false) }
    var address by remember { mutableStateOf("") }
    val connected = link as? LinkState.Connected

    LaunchedEffect(Unit) { vm.refreshPaired(); if (vm.link.value !is LinkState.Connected) vm.scan() }

    Column(Modifier.fillMaxSize().statusBarsPadding().padding(horizontal = 18.dp).verticalScroll(rememberScrollState())) {
        Txt(if (connected != null) "Connected" else "Find a computer", Type.Display, modifier = Modifier.padding(top = 14.dp, bottom = 6.dp))

        if (connected != null) {
            Box(Modifier.height(10.dp))
            Tile(Modifier.fillMaxWidth(), level = 1, shape = RoundedCornerShape(30.dp)) {
                Column(Modifier.padding(22.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(54.dp).clip(CircleShape).background(p.ok.copy(alpha = 0.14f)), contentAlignment = Alignment.Center) {
                            MIcon(R.drawable.ic_laptop, tint = p.ok, size = 28.dp)
                        }
                        Box(Modifier.width(16.dp))
                        Column(Modifier.weight(1f)) {
                            Txt(connected.host, Type.Title, maxLines = 1)
                            Txt(connected.latencyMs?.let { "Live · $it ms" } ?: "Live", Type.Label, p.ok)
                        }
                    }
                    Box(Modifier.height(18.dp))
                    MButton("Disconnect", { vm.session.disconnect() }, Modifier.fillMaxWidth(), kind = ButtonKind.Danger)
                }
            }
        } else {
            val failed = link as? LinkState.Failed
            Radar(blips = found.map { it.id }, scanning = scanning)
            AnimatedVisibility(failed != null, enter = fadeIn(tween(Motion.Base)) + expandVertically(tween(Motion.Base, easing = Motion.Smooth)), exit = fadeOut(tween(Motion.Fast)) + shrinkVertically(tween(Motion.Base))) {
                Tile(Modifier.fillMaxWidth().padding(bottom = 14.dp), level = 1, shape = RoundedCornerShape(22.dp)) {
                    Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(8.dp).clip(CircleShape).background(p.danger))
                        Box(Modifier.width(12.dp))
                        Txt(failed?.reason ?: "", Type.Label, p.text, Modifier.weight(1f))
                    }
                }
            }

            SectionLabel(if (found.isEmpty()) (if (scanning) "Searching your network…" else "Nothing found nearby") else "Nearby")
            found.forEach { h -> HostRow(h.name, h.address, paired.any { it.id == h.id }) { vm.connect(h) }; Box(Modifier.height(10.dp)) }

            val offline = paired.filter { pr -> found.none { it.id == pr.id } }
            if (offline.isNotEmpty()) {
                SectionLabel("Paired")
                offline.forEach { h ->
                    HostRow(h.name, h.lastAddress ?: "Not seen recently", true, dim = true, onForget = { vm.forget(h.id) }) { vm.connect(h) }
                    Box(Modifier.height(10.dp))
                }
            }

            Box(Modifier.height(6.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                MButton(if (scanning) "Scanning…" else "Scan again", { vm.scan() }, Modifier.weight(1f), kind = ButtonKind.Tonal, enabled = !scanning, icon = R.drawable.ic_wifi)
                MButton("Enter address", { manual = !manual }, Modifier.weight(1f), kind = ButtonKind.Tonal, icon = R.drawable.ic_laptop)
            }
            AnimatedVisibility(manual, enter = fadeIn(tween(Motion.Base)) + expandVertically(tween(Motion.Base, easing = Motion.Smooth)), exit = fadeOut(tween(Motion.Fast)) + shrinkVertically(tween(Motion.Base))) {
                Column(Modifier.padding(top = 12.dp)) {
                    Tile(Modifier.fillMaxWidth(), level = 1, shape = RoundedCornerShape(20.dp)) {
                        Box(Modifier.padding(horizontal = 20.dp).height(56.dp), contentAlignment = Alignment.CenterStart) {
                            if (address.isEmpty()) Txt("192.168.1.20", Type.Body, p.textFaint)
                            BasicTextField(
                                address, { address = it.trim() }, singleLine = true,
                                textStyle = Type.Body.copy(color = p.text), cursorBrush = SolidColor(p.accent),
                                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                                modifier = Modifier.fillMaxWidth(),
                            )
                        }
                    }
                    Box(Modifier.height(10.dp))
                    MButton("Connect", { vm.connect(address) }, Modifier.fillMaxWidth(), enabled = address.isNotBlank())
                }
            }
        }

        Box(Modifier.height(26.dp))
        SectionLabel("Feel")
        Tile(Modifier.fillMaxWidth(), level = 1, shape = RoundedCornerShape(28.dp)) {
            Column(Modifier.padding(vertical = 8.dp)) {
                Column(Modifier.padding(horizontal = 20.dp, vertical = 10.dp)) {
                    Txt("Pointer speed", Type.Body)
                    MSlider(s.sensitivity, { v -> vm.update { it.copy(sensitivity = v) } }, 0.4f..2.5f)
                }
                Column(Modifier.padding(horizontal = 20.dp, vertical = 10.dp)) {
                    Txt("Scroll speed", Type.Body)
                    MSlider(s.scrollSpeed, { v -> vm.update { it.copy(scrollSpeed = v) } }, 0.4f..2.5f)
                }
                SettingRow("Natural scrolling", "Content follows your fingers") { MSwitch(s.naturalScroll) { v -> vm.update { it.copy(naturalScroll = v) } } }
                SettingRow("Tap to click") { MSwitch(s.tapToClick) { v -> vm.update { it.copy(tapToClick = v) } } }
                SettingRow("Haptics") { MSwitch(s.haptics) { v -> vm.update { it.copy(haptics = v) } } }
            }
        }

        Box(Modifier.height(18.dp))
        SectionLabel("App")
        Tile(Modifier.fillMaxWidth(), level = 1, shape = RoundedCornerShape(28.dp)) {
            Column(Modifier.padding(vertical = 8.dp)) {
                SettingRow("Keep screen on", "While connected") { MSwitch(s.keepAwake) { v -> vm.update { it.copy(keepAwake = v) } } }
                SettingRow("Reconnect automatically") { MSwitch(s.autoReconnect) { v -> vm.update { it.copy(autoReconnect = v) } } }
                Column(Modifier.padding(horizontal = 20.dp, vertical = 12.dp)) {
                    Txt("Appearance", Type.Body)
                    Box(Modifier.height(10.dp))
                    Segmented(listOf(ThemeMode.System to "Auto", ThemeMode.Light to "Light", ThemeMode.Dark to "Dark"), s.theme) { m -> vm.update { it.copy(theme = m) } }
                }
            }
        }
        Box(Modifier.height(124.dp))
    }
}

@Composable
private fun HostRow(name: String, sub: String, paired: Boolean, dim: Boolean = false, onForget: (() -> Unit)? = null, onClick: () -> Unit) {
    val p = LocalPalette.current
    Tile(Modifier.fillMaxWidth(), level = 1, shape = RoundedCornerShape(24.dp), onClick = onClick) {
        Row(Modifier.padding(horizontal = 18.dp, vertical = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(46.dp).clip(RoundedCornerShape(16.dp)).background(p.surfaceTop), contentAlignment = Alignment.Center) {
                MIcon(R.drawable.ic_laptop, tint = if (dim) p.textDim else p.accent, size = 24.dp)
            }
            Box(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Txt(name, Type.Body, if (dim) p.textDim else p.text, maxLines = 1)
                Txt(if (paired) "$sub · paired" else sub, Type.Label, p.textFaint, maxLines = 1)
            }
            if (onForget != null) {
                Box(Modifier.clip(CircleShape).pressable(onClick = onForget).padding(horizontal = 12.dp, vertical = 8.dp)) { Txt("Forget", Type.Label, p.danger) }
            }
        }
    }
}
