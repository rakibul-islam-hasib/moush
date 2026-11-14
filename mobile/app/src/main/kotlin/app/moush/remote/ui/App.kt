package app.moush.remote.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import app.moush.remote.AppViewModel
import app.moush.remote.R
import app.moush.remote.net.LinkState

@Composable
fun App(vm: AppViewModel) {
    val p = LocalPalette.current
    val link by vm.link.collectAsState()
    val settings by vm.settings.collectAsState()
    var tab by remember { mutableIntStateOf(if (vm.store.pairedHosts().isEmpty()) 5 else 0) }
    var last by remember { mutableIntStateOf(tab) }

    val navControl = remember { NavControl() }
    val navHide by androidx.compose.animation.core.animateFloatAsState(if (navControl.hidden) 1f else 0f, Motion.base(), label = "nav-hide")

    val items = listOf(
        NavItem("Pad", R.drawable.ic_touchpad),
        NavItem("Keys", R.drawable.ic_keyboard),
        NavItem("Screen", R.drawable.ic_monitor),
        NavItem("Deck", R.drawable.ic_apps),
        NavItem("Files", R.drawable.ic_upload),
        NavItem(
            "Link", R.drawable.ic_laptop,
            when (link) {
                is LinkState.Connected -> BadgeKind.Online
                is LinkState.Connecting, is LinkState.Verifying -> BadgeKind.Busy
                else -> BadgeKind.None
            },
        ),
    )

    // Files shared from another app wait for a connection, then go out and show the Files tab.
    LaunchedEffect(link is LinkState.Connected) {
        if (link is LinkState.Connected) vm.flushPendingShare()
    }
    LaunchedEffect(vm.shareTick.collectAsState().value) { if (vm.shareTick.value > 0) tab = 4 }

    Box(Modifier.fillMaxSize().background(p.bg)) {
        androidx.compose.runtime.CompositionLocalProvider(LocalNavControl provides navControl) {
        AnimatedContent(
            targetState = tab,
            transitionSpec = {
                val dir = if (targetState > initialState) 1 else -1
                (fadeIn(tween(Motion.Slow, 60, Motion.Smooth)) + slideInHorizontally(tween(Motion.Slow, 60, Motion.Smooth)) { dir * it / 12 }) togetherWith
                    (fadeOut(tween(Motion.Fast, easing = Motion.Smooth)) + slideOutHorizontally(tween(Motion.Base, easing = Motion.Smooth)) { -dir * it / 12 })
            },
            label = "tabs",
        ) { t ->
            when (t) {
                0 -> PadScreen(vm.session, link, settings) { tab = 5 }
                1 -> KeysScreen(vm.session, link, settings)
                2 -> ScreenScreen(vm)
                3 -> DeckScreen(vm.session, link, settings)
                4 -> FilesScreen(vm) { tab = 5 }
                else -> LinkScreen(vm)
            }
        }

        }

        NavBar(
            items, tab, { last = tab; tab = it },
            Modifier.align(Alignment.BottomCenter).graphicsLayer { translationY = navHide * 240.dp.toPx(); alpha = 1f - navHide },
        )

        PairingOverlay(
            link = link,
            onConfirm = { vm.session.confirmCode(it) },
            onCancel = { vm.session.disconnect() },
            onDone = { tab = 0 },
        )
    }
}
