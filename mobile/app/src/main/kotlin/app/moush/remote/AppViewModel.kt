package app.moush.remote

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import app.moush.remote.data.PairedHost
import app.moush.remote.data.Settings
import app.moush.remote.data.ScreenClient
import app.moush.remote.data.Store
import app.moush.remote.data.Transfers
import app.moush.remote.net.Discovery
import app.moush.remote.net.FoundHost
import app.moush.remote.net.LinkState
import app.moush.remote.net.Protocol
import app.moush.remote.net.Session
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

class AppViewModel(app: Application) : AndroidViewModel(app) {
    val store = Store(app)
    val session = Session(store)
    val link: StateFlow<LinkState> = session.state
    val transfers = Transfers(app, session, viewModelScope)
    val screen = ScreenClient(session, viewModelScope)

    /** Files shared into the app while no computer was connected; sent as soon as one is. */
    /** Bumps whenever something was shared in, so the UI can jump to the Files tab. */
    val shareTick = MutableStateFlow(0)

    private var pendingShare: List<android.net.Uri> = emptyList()

    private val _settings = MutableStateFlow(store.settings())
    val settings: StateFlow<Settings> = _settings.asStateFlow()

    private val _found = MutableStateFlow<List<FoundHost>>(emptyList())
    val found: StateFlow<List<FoundHost>> = _found.asStateFlow()

    private val _scanning = MutableStateFlow(false)
    val scanning: StateFlow<Boolean> = _scanning.asStateFlow()

    private val _paired = MutableStateFlow(store.pairedHosts())
    val paired: StateFlow<List<PairedHost>> = _paired.asStateFlow()

    private var scanJob: Job? = null

    fun update(change: (Settings) -> Settings) {
        _settings.update(change)
        store.save(_settings.value)
    }

    fun scan() {
        if (scanJob?.isActive == true) return
        _scanning.value = true
        scanJob = viewModelScope.launch {
            try {
                _found.value = emptyList()
                Discovery.scan(getApplication(), 2400) { h -> _found.update { (it.filter { x -> x.id != h.id } + h) } }
            } finally {
                _scanning.value = false
            }
        }
    }

    fun connect(address: String, port: Int = Protocol.DEFAULT_PORT) {
        session.connect(address, port, _settings.value.deviceName)
    }

    fun connect(h: FoundHost) = connect(h.address, h.port)

    fun connect(h: PairedHost) {
        // The computer may have a new address: look it up by id first, fall back to the last one.
        viewModelScope.launch {
            var hit: FoundHost? = null
            Discovery.scan(getApplication(), 1200) { if (it.id == h.id) hit = it }
            val f = hit
            when {
                f != null -> connect(f.address, f.port)
                h.lastAddress != null -> connect(h.lastAddress, h.lastPort)
                else -> connect("", h.lastPort)
            }
        }
    }

    fun reconnectLast() {
        if (link.value !is LinkState.Idle && link.value !is LinkState.Failed) return
        val id = store.lastHostId ?: return
        _paired.value.firstOrNull { it.id == id }?.let { connect(it) }
    }

    /** Files arriving from the Android share sheet. Returns true if they were sent right away. */
    fun share(uris: List<android.net.Uri>): Boolean {
        if (uris.isEmpty()) return false
        shareTick.value += 1
        if (link.value is LinkState.Connected) {
            transfers.sendFiles(uris)
            return true
        }
        pendingShare = uris
        reconnectLast()
        return false
    }

    fun flushPendingShare() {
        val u = pendingShare
        if (u.isNotEmpty() && link.value is LinkState.Connected) {
            pendingShare = emptyList()
            transfers.sendFiles(u)
        }
    }

    fun forget(id: String) {
        store.forget(id)
        _paired.value = store.pairedHosts()
    }

    fun refreshPaired() {
        _paired.value = store.pairedHosts()
    }

    override fun onCleared() {
        session.disconnect()
    }
}
