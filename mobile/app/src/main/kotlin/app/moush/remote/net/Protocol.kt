package app.moush.remote.net

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Wire messages (little-endian, `[type u8][fields]`), matching `moush-core::protocol`. */
sealed interface Msg {
    data class Hello(val version: Int, val role: Int, val name: String, val trustsPeer: Boolean) : Msg
    data class Authorize(val accept: Boolean) : Msg
    data class Bye(val reason: String) : Msg
    data class MouseRel(val dx: Int, val dy: Int) : Msg

    /** Absolute position in virtual-desktop pixels (used by the screen viewer). */
    data class MouseAbs(val x: Int, val y: Int) : Msg
    data class Button(val button: Int, val down: Boolean) : Msg
    data class Wheel(val horizontal: Boolean, val delta: Int) : Msg
    data class Key(val vk: Int, val scan: Int, val flags: Int) : Msg
    data object ReleaseAll : Msg
    data class Ping(val id: Int, val tUs: Long) : Msg
    data class Pong(val id: Int, val tUs: Long) : Msg

    data class Capabilities(val features: Int) : Msg
    class Feature(val channel: Int, val body: ByteArray) : Msg

    /** Anything the host sends that a remote has no use for (capabilities, features, ...). */
    data object Ignored : Msg
}

object Protocol {
    const val VERSION = 2
    const val ROLE_HOST = 1
    const val ROLE_REMOTE = 3
    const val DEFAULT_PORT = 24800
    const val DISCOVERY_PORT = 24801

    const val BTN_LEFT = 1
    const val BTN_RIGHT = 2
    const val BTN_MIDDLE = 3
    const val BTN_X1 = 4
    const val BTN_X2 = 5

    const val CHANNEL_FILES = 2
    const val CHANNEL_SCREEN = 3
    const val CAP_FILES = 1 shl 1
    const val CAP_SCREEN = 1 shl 3

    const val KEY_DOWN = 1
    const val KEY_EXTENDED = 2

    private const val MAX_STRING = 128

    fun encode(m: Msg, out: ByteArrayOutputStream) {
        if (m is Msg.Feature) {
            val f = ByteBuffer.allocate(6 + m.body.size).order(ByteOrder.LITTLE_ENDIAN)
            f.put(31).put(m.channel.toByte()).putInt(m.body.size).put(m.body)
            out.write(f.array())
            return
        }
        val b = ByteBuffer.allocate(MAX_STRING + 32).order(ByteOrder.LITTLE_ENDIAN)
        when (m) {
            is Msg.Hello -> {
                b.put(1).putShort(m.version.toShort()).put(m.role.toByte()); putStr(b, m.name); b.put(if (m.trustsPeer) 1 else 0)
            }
            is Msg.Authorize -> b.put(2).put(if (m.accept) 1 else 0)
            is Msg.Bye -> { b.put(3); putStr(b, m.reason) }
            is Msg.MouseAbs -> b.put(12).putInt(m.x).putInt(m.y)
            is Msg.Capabilities -> b.put(30).putInt(m.features)
            is Msg.Feature -> return
            is Msg.MouseRel -> b.put(17).putShort(m.dx.toShort()).putShort(m.dy.toShort())
            is Msg.Button -> b.put(13).put(m.button.toByte()).put(if (m.down) 1 else 0)
            is Msg.Wheel -> b.put(14).put(if (m.horizontal) 1 else 0).putShort(m.delta.toShort())
            is Msg.Key -> b.put(15).putShort(m.vk.toShort()).putShort(m.scan.toShort()).put(m.flags.toByte())
            Msg.ReleaseAll -> b.put(16)
            is Msg.Ping -> b.put(20).putInt(m.id).putLong(m.tUs)
            is Msg.Pong -> b.put(21).putInt(m.id).putLong(m.tUs)
            Msg.Ignored -> return
        }
        out.write(b.array(), 0, b.position())
    }

    fun encode(vararg msgs: Msg): ByteArray {
        val o = ByteArrayOutputStream()
        for (m in msgs) encode(m, o)
        return o.toByteArray()
    }

    private fun putStr(b: ByteBuffer, s: String) {
        var bytes = s.toByteArray(Charsets.UTF_8)
        if (bytes.size > MAX_STRING) {
            var end = MAX_STRING
            while (end > 0 && (bytes[end].toInt() and 0xC0) == 0x80) end--
            bytes = bytes.copyOf(end)
        }
        b.put(bytes.size.toByte()).put(bytes)
    }

    /** Decodes every message of one frame. Throws on malformed data. */
    fun decodeAll(buf: ByteArray): List<Msg> {
        val b = ByteBuffer.wrap(buf).order(ByteOrder.LITTLE_ENDIAN)
        val out = ArrayList<Msg>()
        fun str(): String {
            val n = b.get().toInt() and 0xFF
            val bytes = ByteArray(n)
            b.get(bytes)
            return String(bytes, Charsets.UTF_8)
        }
        while (b.hasRemaining()) {
            when (val t = b.get().toInt() and 0xFF) {
                1 -> out += Msg.Hello(b.short.toInt() and 0xFFFF, b.get().toInt(), str(), b.get().toInt() == 1)
                2 -> out += Msg.Authorize(b.get().toInt() == 1)
                3 -> out += Msg.Bye(str())
                10 -> { b.position(b.position() + 8); out += Msg.Ignored }
                11 -> out += Msg.Ignored
                12 -> { b.position(b.position() + 8); out += Msg.Ignored }
                13 -> { b.position(b.position() + 2); out += Msg.Ignored }
                14 -> { b.position(b.position() + 3); out += Msg.Ignored }
                15 -> { b.position(b.position() + 5); out += Msg.Ignored }
                16 -> out += Msg.Ignored
                20 -> out += Msg.Ping(b.int, b.long)
                21 -> out += Msg.Pong(b.int, b.long)
                30 -> out += Msg.Capabilities(b.int)
                31 -> {
                    val ch = b.get().toInt() and 0xFF
                    val n = b.int
                    require(n in 0..b.remaining()) { "bad feature length" }
                    val body = ByteArray(n)
                    b.get(body)
                    out += Msg.Feature(ch, body)
                }
                else -> throw IllegalArgumentException("unknown message type $t")
            }
        }
        return out
    }
}
