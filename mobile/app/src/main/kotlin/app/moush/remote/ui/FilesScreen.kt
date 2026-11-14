package app.moush.remote.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.DrawableRes
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Canvas
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
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import app.moush.remote.AppViewModel
import app.moush.remote.R
import app.moush.remote.data.TState
import app.moush.remote.data.TransferItem
import app.moush.remote.net.LinkState
import app.moush.remote.net.Protocol

@Composable
fun FilesScreen(vm: AppViewModel, onOpenLink: () -> Unit) {
    val p = LocalPalette.current
    val link by vm.link.collectAsState()
    val caps by vm.session.peerCaps.collectAsState()
    val items by vm.transfers.items.collectAsState()
    val connected = link as? LinkState.Connected
    val allowed = caps and Protocol.CAP_FILES != 0
    val ctx = LocalContext.current

    val pickFiles = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) vm.transfers.sendFiles(uris)
    }
    val pickFolder = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri != null) {
            runCatching { ctx.contentResolver.takePersistableUriPermission(uri, android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION) }
            vm.transfers.sendFolder(uri)
        }
    }

    Column(Modifier.fillMaxSize().statusBarsPadding().padding(horizontal = 18.dp).verticalScroll(rememberScrollState())) {
        Txt("Files", Type.Display, modifier = Modifier.padding(top = 14.dp, bottom = 14.dp))

        // Hero: a glowing drop target.
        Tile(Modifier.fillMaxWidth(), level = 1, shape = RoundedCornerShape(32.dp)) {
            Column(Modifier.fillMaxWidth().padding(22.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                UploadOrb(active = items.any { it.active })
                Box(Modifier.height(16.dp))
                Txt(
                    if (connected != null) "Send to ${connected.host}" else "Send to your computer",
                    Type.Title,
                )
                Txt(
                    when {
                        connected == null -> "Connect to a computer first."
                        !allowed -> "File transfer is off for this phone. Turn it on in Moush › Devices on the computer."
                        else -> "Photos, documents, whole folders. They land in the computer's Downloads folder."
                    },
                    Type.Label, p.textDim, Modifier.padding(top = 6.dp, start = 8.dp, end = 8.dp), maxLines = 3,
                )
                Box(Modifier.height(20.dp))
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    MButton("Files", { pickFiles.launch(arrayOf("*/*")) }, Modifier.weight(1f), icon = R.drawable.ic_file, enabled = connected != null && allowed)
                    MButton("Folder", { pickFolder.launch(null) }, Modifier.weight(1f), kind = ButtonKind.Tonal, icon = R.drawable.ic_folder, enabled = connected != null && allowed)
                }
                if (connected == null) {
                    Box(Modifier.height(10.dp))
                    MButton("Find a computer", onOpenLink, Modifier.fillMaxWidth(), kind = ButtonKind.Ghost, icon = R.drawable.ic_wifi)
                }
            }
        }

        Box(Modifier.height(12.dp))
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(22.dp)).background(p.surface).padding(horizontal = 16.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(36.dp).clip(CircleShape).background(p.accentAlt.copy(alpha = 0.16f)), contentAlignment = Alignment.Center) {
                MIcon(R.drawable.ic_upload, tint = p.accentAlt, size = 18.dp)
            }
            Box(Modifier.width(12.dp))
            Txt("Tip: in Gallery, Files or any app, tap Share and choose Moush.", Type.Label, p.textDim, Modifier.weight(1f), maxLines = 2)
        }

        Box(Modifier.height(22.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            SectionLabel("Transfers", Modifier.weight(1f))
            AnimatedVisibility(items.any { !it.active }, enter = fadeIn(tween(Motion.Base)), exit = fadeOut(tween(Motion.Fast))) {
                Box(Modifier.clip(CircleShape).pressable { vm.transfers.clearFinished() }.padding(horizontal = 12.dp, vertical = 6.dp)) {
                    Txt("Clear", Type.Label, p.accent)
                }
            }
        }
        if (items.isEmpty()) {
            Txt("Nothing sent yet.", Type.Body, p.textFaint, Modifier.padding(start = 6.dp, top = 4.dp))
        }
        items.forEach { it ->
            TransferCard(it, onCancel = { vm.transfers.cancel(it.id) })
            Box(Modifier.height(10.dp))
        }
        Box(Modifier.height(124.dp))
    }
}

/** A soft orb that breathes while something is being sent. */
@Composable
private fun UploadOrb(active: Boolean) {
    val p = LocalPalette.current
    val inf = rememberInfiniteTransition(label = "orb")
    val t by inf.animateFloat(0f, 1f, infiniteRepeatable(tween(2600, easing = LinearEasing)), label = "ring")
    val lift by inf.animateFloat(0f, 1f, infiniteRepeatable(tween(1400, easing = Motion.InOut), RepeatMode.Reverse), label = "lift")
    val run by animateFloatAsState(if (active) 1f else 0.25f, Motion.slow(), label = "run")
    Box(Modifier.size(150.dp), contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            for (k in 0..1) {
                val ph = (t + k * 0.5f) % 1f
                drawCircle(p.accent.copy(alpha = (1f - ph) * 0.3f * run), 38.dp.toPx() + 34.dp.toPx() * ph, center, style = androidx.compose.ui.graphics.drawscope.Stroke(2.dp.toPx()))
            }
            drawCircle(
                Brush.radialGradient(listOf(p.accent.copy(alpha = 0.34f), p.accentAlt.copy(alpha = 0.10f), Color.Transparent), center = center, radius = 60.dp.toPx()),
                radius = 60.dp.toPx(), center = center,
            )
        }
        Box(Modifier.size(70.dp).clip(CircleShape).background(p.surfaceTop), contentAlignment = Alignment.Center) {
            Box(Modifier.graphicsLayer { translationY = -4.dp.toPx() * lift * run }) {
                MIcon(R.drawable.ic_upload, tint = p.accent, size = 32.dp)
            }
        }
    }
}

private fun size(bytes: Long): String = when {
    bytes >= 1L shl 30 -> "%.1f GB".format(bytes / (1L shl 30).toDouble())
    bytes >= 1L shl 20 -> "%.1f MB".format(bytes / (1L shl 20).toDouble())
    bytes >= 1L shl 10 -> "%.0f KB".format(bytes / (1L shl 10).toDouble())
    else -> "$bytes B"
}

@Composable
private fun TransferCard(t: TransferItem, onCancel: () -> Unit) {
    val p = LocalPalette.current
    val frac by animateFloatAsState(t.fraction, tween(380, easing = Motion.Smooth), label = "progress")
    val tint by animateColorAsState(
        when (t.state) {
            TState.Done -> p.ok
            TState.Failed, TState.Rejected -> p.danger
            TState.Cancelled -> p.textDim
            else -> p.accent
        },
        Motion.base(), label = "tint",
    )
    @DrawableRes val icon = if (t.folder) R.drawable.ic_folder else R.drawable.ic_file
    Tile(Modifier.fillMaxWidth(), level = 1, shape = RoundedCornerShape(26.dp)) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(46.dp).clip(RoundedCornerShape(16.dp)).background(tint.copy(alpha = 0.14f)), contentAlignment = Alignment.Center) {
                    if (t.state == TState.Done) MIcon(R.drawable.ic_check, tint = tint, size = 24.dp) else MIcon(icon, tint = tint, size = 24.dp)
                }
                Box(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Txt(t.title, Type.Body, p.text, maxLines = 1)
                    val sub = buildString {
                        if (t.files > 0) append(if (t.files == 1) "1 file" else "${t.files} files")
                        if (t.totalBytes > 0) { if (isNotEmpty()) append(" · "); append(size(t.totalBytes)) }
                        if (t.state == TState.Sending && t.bytesPerSec > 0) append(" · ${size(t.bytesPerSec.toLong())}/s")
                    }
                    Txt(sub.ifEmpty { " " }, Type.Label, p.textDim, maxLines = 1)
                }
                if (t.active) {
                    Box(Modifier.size(38.dp).clip(CircleShape).pressable(onClick = onCancel), contentAlignment = Alignment.Center) {
                        MIcon(R.drawable.ic_close, tint = p.textDim, size = 20.dp)
                    }
                } else {
                    Txt(
                        when (t.state) {
                            TState.Done -> "Sent"
                            TState.Rejected -> "Declined"
                            TState.Cancelled -> "Cancelled"
                            else -> "Failed"
                        },
                        Type.Label, tint,
                    )
                }
            }
            AnimatedVisibility(
                t.state == TState.Sending || t.state == TState.Waiting || t.state == TState.Preparing,
                enter = fadeIn(tween(Motion.Base)) + expandVertically(tween(Motion.Base, easing = Motion.Smooth)),
                exit = fadeOut(tween(Motion.Fast)) + shrinkVertically(tween(Motion.Base)),
            ) {
                Column {
                    Box(Modifier.height(14.dp))
                    Box(Modifier.fillMaxWidth().height(6.dp).clip(CircleShape).background(p.surfaceTop)) {
                        if (t.state == TState.Sending) {
                            Box(Modifier.fillMaxWidth(frac.coerceAtLeast(0.02f)).height(6.dp).clip(CircleShape).background(Brush.horizontalGradient(listOf(p.accent, p.accentAlt))))
                        } else {
                            Indeterminate()
                        }
                    }
                    if (t.state != TState.Sending) {
                        Txt(t.message ?: "Working…", Type.Label, p.textDim, Modifier.padding(top = 8.dp), maxLines = 2)
                    }
                }
            }
            AnimatedVisibility(
                !t.active && t.message != null,
                enter = fadeIn(tween(Motion.Base)) + expandVertically(tween(Motion.Base, easing = Motion.Smooth)),
                exit = fadeOut(tween(Motion.Fast)),
            ) {
                Txt(t.message ?: "", Type.Label, p.textDim, Modifier.padding(top = 10.dp), maxLines = 3)
            }
        }
    }
}

/** A slow light that sweeps along the track while we wait for an answer. */
@Composable
private fun Indeterminate() {
    val p = LocalPalette.current
    val inf = rememberInfiniteTransition(label = "wait")
    val t by inf.animateFloat(0f, 1f, infiniteRepeatable(tween(1500, easing = Motion.InOut)), label = "sweep")
    Canvas(Modifier.fillMaxWidth().height(6.dp)) {
        val w = size.width * 0.35f
        val x = (size.width + w) * t - w
        drawRect(Brush.horizontalGradient(listOf(Color.Transparent, p.accent, Color.Transparent), startX = x, endX = x + w), topLeft = Offset(x.coerceAtLeast(0f), 0f), size = androidx.compose.ui.geometry.Size(w.coerceAtMost(size.width - x.coerceAtLeast(0f)), size.height))
    }
}
