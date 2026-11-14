package app.moush.remote.data

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import app.moush.remote.net.Protocol
import app.moush.remote.net.Session
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** A display of the computer, positioned in its virtual desktop (physical pixels). */
data class RemoteDisplay(val index: Int, val x: Int, val y: Int, val width: Int, val height: Int, val primary: Boolean, val name: String)

/** What the phone currently wants to see: a display, optionally cropped (zoomed). */
data class ViewRequest(
    val display: Int,
    val cropX: Int = 0,
    val cropY: Int = 0,
    val cropW: Int = 0,
    val cropH: Int = 0,
    val outW: Int = 1280,
    val outH: Int = 720,
    val quality: Int = 60,
    val fps: Int = 15,
)

/** A decoded picture plus the region of the display it shows. */
class ScreenFrame(val bitmap: Bitmap, val display: Int, val cropX: Int, val cropY: Int, val cropW: Int, val cropH: Int, val seq: Int)

data class RemoteCursor(val display: Int, val x: Int, val y: Int)

/** Receives the computer's screen over the feature channel (see `moush-core::screen`). */
class ScreenClient(private val session: Session, private val scope: CoroutineScope) {
    private val _displays = MutableStateFlow<List<RemoteDisplay>>(emptyList())
    val displays: StateFlow<List<RemoteDisplay>> = _displays.asStateFlow()

    private val _frame = MutableStateFlow<ScreenFrame?>(null)
    val frame: StateFlow<ScreenFrame?> = _frame.asStateFlow()

    private val _cursor = MutableStateFlow<RemoteCursor?>(null)
    val cursor: StateFlow<RemoteCursor?> = _cursor.asStateFlow()

    private val _fps = MutableStateFlow(0)
    val fps: StateFlow<Int> = _fps.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    private class Partial(val seq: Int, val total: Int, val buf: ByteArray, var got: Int, val meta: IntArray)

    private var partial: Partial? = null
    private var lastSeq = 0
    private var framesThisSecond = 0
    private var secondStart = System.nanoTime()

    init {
        scope.launch(Dispatchers.Default) {
            session.features.collect { f -> if (f.channel == Protocol.CHANNEL_SCREEN) runCatching { handle(f.body) } }
        }
    }

    private fun le(n: Int) = ByteBuffer.allocate(n).order(ByteOrder.LITTLE_ENDIAN)

    fun requestDisplays() {
        _error.value = null
        session.sendFeature(Protocol.CHANNEL_SCREEN, byteArrayOf(1))
    }

    fun setView(v: ViewRequest) {
        val b = le(1 + 1 + 16 + 4 + 2)
        b.put(2).put(v.display.toByte()).putInt(v.cropX).putInt(v.cropY).putInt(v.cropW).putInt(v.cropH)
            .putShort(v.outW.toShort()).putShort(v.outH.toShort()).put(v.quality.toByte()).put(v.fps.toByte())
        session.sendFeature(Protocol.CHANNEL_SCREEN, b.array())
    }

    fun stop() {
        session.sendFeature(Protocol.CHANNEL_SCREEN, byteArrayOf(3))
        _frame.value = null
        _cursor.value = null
        _fps.value = 0
        partial = null
    }

    fun reset() {
        _displays.value = emptyList()
        _frame.value = null
        _cursor.value = null
        _error.value = null
        partial = null
    }

    private fun handle(body: ByteArray) {
        val b = ByteBuffer.wrap(body).order(ByteOrder.LITTLE_ENDIAN)
        when (b.get().toInt() and 0xFF) {
            10 -> {
                val n = b.get().toInt() and 0xFF
                val list = ArrayList<RemoteDisplay>()
                repeat(n) { i ->
                    val x = b.int
                    val y = b.int
                    val w = b.int
                    val h = b.int
                    val primary = b.get().toInt() == 1
                    val len = b.get().toInt() and 0xFF
                    val nm = ByteArray(len).also { b.get(it) }
                    list += RemoteDisplay(i, x, y, w, h, primary, String(nm, Charsets.UTF_8))
                }
                _displays.value = list
            }
            11 -> {
                val seq = b.int
                val display = b.get().toInt() and 0xFF
                val cx = b.int
                val cy = b.int
                val cw = b.int
                val ch = b.int
                val curX = b.int
                val curY = b.int
                b.short; b.short // image size: decoded from the JPEG itself
                val total = b.int
                val offset = b.int
                val len = b.int
                if (seq < lastSeq || total <= 0 || total > (8 shl 20) || len < 0 || offset < 0 || offset + len > total || len > b.remaining()) return
                var p = partial
                if (p == null || p.seq != seq) {
                    p = Partial(seq, total, ByteArray(total), 0, intArrayOf(display, cx, cy, cw, ch, curX, curY))
                    partial = p
                }
                b.get(p.buf, offset, len)
                p.got += len
                if (p.got >= p.total) {
                    partial = null
                    lastSeq = seq
                    val bmp = BitmapFactory.decodeByteArray(p.buf, 0, p.total)
                    // Acknowledge even a frame we cannot decode, or the computer stalls.
                    session.sendFeature(Protocol.CHANNEL_SCREEN, le(5).put(4).putInt(seq).array())
                    if (bmp != null) {
                        _error.value = null
                        _frame.value = ScreenFrame(bmp, display, cx, cy, cw, ch, seq)
                        _cursor.value = if (curX == Int.MIN_VALUE) null else RemoteCursor(display, curX, curY)
                        framesThisSecond++
                        val now = System.nanoTime()
                        if (now - secondStart >= 1_000_000_000L) {
                            _fps.value = framesThisSecond
                            framesThisSecond = 0
                            secondStart = now
                        }
                    }
                }
            }
            12 -> {
                val len = b.get().toInt() and 0xFF
                _error.value = String(ByteArray(len).also { b.get(it) }, Charsets.UTF_8)
            }
            13 -> {
                val display = b.get().toInt() and 0xFF
                val x = b.int
                val y = b.int
                _cursor.value = if (x == Int.MIN_VALUE) null else RemoteCursor(display, x, y)
            }
        }
    }
}
