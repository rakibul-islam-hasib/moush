package app.moush.remote.net

import app.moush.remote.data.PairedHost
import app.moush.remote.data.Store
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.io.ByteArrayOutputStream
import java.net.InetSocketAddress
import java.net.Socket

sealed interface LinkState {
    data object Idle : LinkState
    data class Connecting(val host: String) : LinkState

    /** Both screens show [code]; the user confirms it matches. [hostApproved] flips when the PC accepted. */
    data class Verifying(val host: String, val code: String, val hostApproved: Boolean, val confirmed: Boolean) : LinkState

    data class Connected(val host: String, val hostId: String, val latencyMs: Int?) : LinkState
    data class Failed(val reason: String, val canRetry: Boolean = true) : LinkState
}

/**
 * One connection to a computer: Noise handshake, pairing, keepalive and a coalescing
 * input writer. All input calls are cheap and thread-safe; they only enqueue.
 */
class Session(private val store: Store) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _state = MutableStateFlow<LinkState>(LinkState.Idle)
    val state: StateFlow<LinkState> = _state.asStateFlow()

    /** Feature traffic from the computer (files, screen). */
    val features = MutableSharedFlow<Msg.Feature>(extraBufferCapacity = 1024)

    /** What the computer lets us do (its capability announcement). */
    private val _peerCaps = MutableStateFlow(0)
    val peerCaps: StateFlow<Int> = _peerCaps.asStateFlow()

    private var job: Job? = null
    private var socket: Socket? = null
    private var userDecision: CompletableDeferred<Boolean>? = null
    private val outbox = Channel<Any>(Channel.UNLIMITED)
    private var deviceName = "Phone"

    private object Flush

    private class Move(var dx: Int, var dy: Int)

    fun connect(address: String, port: Int, name: String) {
        disconnect()
        deviceName = name
        val myJob = scope.launch { run(address, port, name) }
        job = myJob
    }

    fun disconnect() {
        job?.cancel()
        job = null
        runCatching { socket?.close() }
        socket = null
        userDecision?.cancel()
        userDecision = null
        while (outbox.tryReceive().isSuccess) { /* drop stale input */ }
        _state.value = LinkState.Idle
        _peerCaps.value = 0
    }

    /** Called by the pairing screen when the user confirms (or rejects) the code. */
    fun confirmCode(match: Boolean) {
        val cur = _state.value
        if (cur is LinkState.Verifying) _state.value = cur.copy(confirmed = match)
        userDecision?.complete(match)
    }

    // ---- input API -------------------------------------------------------------------------

    fun move(dx: Int, dy: Int) {
        if (dx == 0 && dy == 0) return
        outbox.trySend(Move(dx, dy))
    }

    fun button(button: Int, down: Boolean) {
        outbox.trySend(Protocol.encode(Msg.Button(button, down)))
    }

    fun click(button: Int = Protocol.BTN_LEFT) {
        outbox.trySend(Protocol.encode(Msg.Button(button, true), Msg.Button(button, false)))
    }

    fun wheel(horizontal: Boolean, delta: Int) {
        var left = delta
        while (left != 0) {
            val step = left.coerceIn(-32000, 32000)
            outbox.trySend(Protocol.encode(Msg.Wheel(horizontal, step)))
            left -= step
        }
    }

    fun key(vk: Int, scan: Int, extended: Boolean, down: Boolean) {
        val flags = (if (down) Protocol.KEY_DOWN else 0) or (if (extended) Protocol.KEY_EXTENDED else 0)
        outbox.trySend(Protocol.encode(Msg.Key(vk, scan, flags)))
    }

    /** Absolute pointer position in virtual-desktop pixels. */
    fun moveAbs(x: Int, y: Int) {
        outbox.trySend(Protocol.encode(Msg.MouseAbs(x, y)))
    }

    fun sendFeature(channel: Int, body: ByteArray) {
        outbox.trySend(Protocol.encode(Msg.Feature(channel, body)))
    }

    fun releaseAll() {
        outbox.trySend(Protocol.encode(Msg.ReleaseAll))
    }

    // ---- connection ------------------------------------------------------------------------

    private suspend fun run(address: String, port: Int, name: String) {
        _state.value = LinkState.Connecting(address)
        var hostName = address
        try {
            val sock = withContext(Dispatchers.IO) {
                Socket().apply {
                    tcpNoDelay = true
                    keepAlive = true
                    connect(InetSocketAddress(address, port), 5000)
                    soTimeout = 0
                }
            }
            socket = sock
            val ch = withContext(Dispatchers.IO) {
                sock.soTimeout = 8000
                Noise.handshake(sock.getInputStream(), sock.getOutputStream(), store.identity)
            }
            sock.soTimeout = 0

            // Reader: decodes frames into a channel so pairing and keepalive can both consume it.
            val inbox = Channel<Msg>(Channel.UNLIMITED)
            val reader = scope.launch {
                try {
                    while (isActive) for (m in Protocol.decodeAll(ch.read())) inbox.send(m)
                    inbox.close()
                } catch (e: Throwable) {
                    inbox.close(e)
                }
            }

            val iTrust = store.isTrusted(ch.remoteStatic)
            ch.write(Protocol.encode(Msg.Hello(Protocol.VERSION, Protocol.ROLE_REMOTE, name, iTrust)))
            val hello = nextMsg<Msg.Hello>(inbox, 8000)
            hostName = hello.name
            if (hello.role != Protocol.ROLE_HOST) fail("That device is not a Moush host.")
            if (hello.version != Protocol.VERSION) fail("Version mismatch. Update Moush on both devices.")

            if (iTrust && hello.trustsPeer) {
                ch.write(Protocol.encode(Msg.Authorize(true)))
            } else {
                // The PC shows its prompt now; we show the code and wait for the user.
                _state.value = LinkState.Verifying(hostName, ch.verificationCode, hostApproved = hello.trustsPeer, confirmed = false)
                if (!iTrust) {
                    val decision = CompletableDeferred<Boolean>().also { userDecision = it }
                    val ok = withTimeoutOrNull(90_000) { decision.await() } ?: false
                    if (!ok) {
                        runCatching { ch.write(Protocol.encode(Msg.Authorize(false))) }
                        fail("Pairing cancelled.", retry = true)
                    }
                }
                ch.write(Protocol.encode(Msg.Authorize(true)))
            }
            // The host's decision (it may take the user a while to approve on the PC).
            while (true) {
                val m = nextMsg<Msg>(inbox, 100_000)
                when (m) {
                    is Msg.Authorize -> {
                        if (!m.accept) fail("$hostName declined the pairing.")
                        break
                    }
                    is Msg.Bye -> fail(m.reason.ifBlank { "$hostName closed the connection." })
                    else -> {}
                }
            }
            store.saveHost(PairedHost(Noise.deviceId(ch.remoteStatic), hostName, ch.remoteStatic, address, port))
            val hostId = Noise.deviceId(ch.remoteStatic)
            store.lastHostId = hostId
            _state.value = LinkState.Connected(hostName, hostId, null)
            _peerCaps.value = 0
            // Files: we may send, and the computer may offer (we politely decline).
            ch.write(Protocol.encode(Msg.Capabilities(Protocol.CAP_FILES)))

            coroutineScopeRun(ch, inbox, hostName, hostId)
            reader.cancel()
        } catch (e: CancellationException) {
            throw e
        } catch (e: SessionFailure) {
            _state.value = LinkState.Failed(e.message ?: "Connection failed", e.retry)
        } catch (e: Throwable) {
            val msg = when (e) {
                is java.net.ConnectException, is java.net.SocketTimeoutException ->
                    "Couldn't reach $address. Is Moush running as host on that computer?"
                is java.io.EOFException -> "$hostName closed the connection."
                else -> e.message ?: e.javaClass.simpleName
            }
            _state.value = LinkState.Failed(msg)
        } finally {
            runCatching { socket?.close() }
            if (_state.value is LinkState.Connected) _state.value = LinkState.Failed("Connection lost.")
        }
    }

    private class SessionFailure(msg: String, val retry: Boolean) : Exception(msg)

    private fun fail(msg: String, retry: Boolean = true): Nothing = throw SessionFailure(msg, retry)

    private suspend inline fun <reified T : Msg> nextMsg(inbox: Channel<Msg>, timeoutMs: Long): T {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (true) {
            val left = deadline - System.currentTimeMillis()
            val m = withTimeoutOrNull(left.coerceAtLeast(1)) { inbox.receive() } ?: fail("The computer did not respond.")
            if (m is Msg.Bye) fail(m.reason.ifBlank { "The computer closed the connection." })
            if (m is T) return m
        }
    }

    /** Authorized: pump input out, answer pings, measure latency, detect a dead link. */
    private suspend fun coroutineScopeRun(ch: SecureChannel, inbox: Channel<Msg>, hostName: String, hostId: String) {
        val writer = scope.launch { runCatching { writerLoop(ch) } }
        var lastRx = System.currentTimeMillis()
        var pingId = 1
        var nextPing = 0L
        var rtt: Int? = null
        val sentAt = HashMap<Int, Long>()
        try {
            while (scope.isActive && writer.isActive) {
                val now = System.currentTimeMillis()
                if (now >= nextPing) {
                    val id = pingId++
                    sentAt[id] = System.nanoTime()
                    outbox.trySend(Protocol.encode(Msg.Ping(id, System.nanoTime() / 1000)))
                    nextPing = now + 1000
                }
                val m = withTimeoutOrNull(100) { inbox.receiveCatching() }
                if (m != null) {
                    if (m.isClosed) {
                        throw m.exceptionOrNull() ?: java.io.EOFException("closed")
                    }
                    lastRx = now
                    when (val msg = m.getOrNull()) {
                        is Msg.Ping -> outbox.trySend(Protocol.encode(Msg.Pong(msg.id, msg.tUs)))
                        is Msg.Pong -> sentAt.remove(msg.id)?.let {
                            val ms = ((System.nanoTime() - it) / 1_000_000L).toInt()
                            rtt = if (rtt == null) ms else ((rtt!! * 3 + ms) / 4)
                            _state.value = LinkState.Connected(hostName, hostId, rtt)
                        }
                        is Msg.Bye -> fail(msg.reason.ifBlank { "$hostName closed the connection." })
                        is Msg.Capabilities -> _peerCaps.value = msg.features
                        is Msg.Feature -> features.tryEmit(msg)
                        else -> {}
                    }
                }
                if (now - lastRx > 3500) fail("Connection lost.")
            }
        } finally {
            writer.cancel()
        }
    }

    /** Sends queued messages; consecutive pointer moves are merged into one. */
    private suspend fun writerLoop(ch: SecureChannel) {
        val buf = ByteArrayOutputStream()
        var pending: Move? = null
        fun flushMove() {
            val p = pending ?: return
            pending = null
            var dx = p.dx
            var dy = p.dy
            while (dx != 0 || dy != 0) {
                val sx = dx.coerceIn(-32000, 32000)
                val sy = dy.coerceIn(-32000, 32000)
                Protocol.encode(Msg.MouseRel(sx, sy), buf)
                dx -= sx
                dy -= sy
            }
        }
        while (scope.isActive) {
            val first = outbox.receive()
            var item: Any? = first
            while (item != null) {
                when (item) {
                    is Move -> {
                        val p = pending
                        if (p == null) pending = Move(item.dx, item.dy) else { p.dx += item.dx; p.dy += item.dy }
                    }
                    is ByteArray -> {
                        flushMove()
                        buf.write(item)
                    }
                }
                if (buf.size() > 16_000) break
                item = outbox.tryReceive().getOrNull()
            }
            flushMove()
            if (buf.size() > 0) {
                withContext(Dispatchers.IO) { ch.write(buf.toByteArray()) }
                buf.reset()
            }
        }
    }
}
