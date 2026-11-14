package app.moush.remote.net

import android.content.Context
import android.net.wifi.WifiManager
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.NetworkInterface
import java.net.SocketTimeoutException

data class FoundHost(val id: String, val name: String, val address: String, val port: Int)

/** LAN discovery: broadcast a probe, hosts answer with `MSHD` announcements. */
object Discovery {
    private val probe = byteArrayOf('M'.code.toByte(), 'S'.code.toByte(), 'H'.code.toByte(), 'D'.code.toByte(), 1, 1)

    private fun broadcastTargets(): List<InetAddress> {
        val out = linkedSetOf<InetAddress>()
        runCatching { out += InetAddress.getByName("255.255.255.255") }
        runCatching {
            for (nif in NetworkInterface.getNetworkInterfaces()) {
                if (!nif.isUp || nif.isLoopback) continue
                for (ia in nif.interfaceAddresses) {
                    val b = ia.broadcast
                    if (b is Inet4Address) out += b
                }
            }
        }
        return out.toList()
    }

    /** Scans for [windowMs]; [onFound] is called once per newly seen host. */
    suspend fun scan(context: Context, windowMs: Int = 1600, onFound: (FoundHost) -> Unit) = withContext(Dispatchers.IO) {
        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        val lock = wifi?.createMulticastLock("moush-discovery")?.apply { setReferenceCounted(false); acquire() }
        try {
            DatagramSocket().use { sock ->
                sock.broadcast = true
                sock.soTimeout = 200
                val targets = broadcastTargets()
                val seen = HashSet<String>()
                val end = System.currentTimeMillis() + windowMs
                var nextProbe = 0L
                val buf = ByteArray(512)
                while (System.currentTimeMillis() < end) {
                    val now = System.currentTimeMillis()
                    if (now >= nextProbe) {
                        for (t in targets) runCatching { sock.send(DatagramPacket(probe, probe.size, t, Protocol.DISCOVERY_PORT)) }
                        nextProbe = now + 500
                    }
                    try {
                        val pkt = DatagramPacket(buf, buf.size)
                        sock.receive(pkt)
                        parse(pkt.data.copyOf(pkt.length), pkt.address.hostAddress ?: continue)?.let {
                            if (seen.add(it.id)) onFound(it)
                        }
                    } catch (_: SocketTimeoutException) {
                    }
                }
            }
        } finally {
            lock?.release()
        }
    }

    internal fun parse(b: ByteArray, from: String): FoundHost? {
        if (b.size < 8 || b[0] != 'M'.code.toByte() || b[1] != 'S'.code.toByte() || b[2] != 'H'.code.toByte() || b[3] != 'D'.code.toByte()) return null
        if (b[4].toInt() != 1 || b[5].toInt() != 2) return null
        var pos = 6
        fun str(): String? {
            val n = b.getOrNull(pos)?.toInt()?.and(0xFF) ?: return null
            if (pos + 1 + n > b.size) return null
            val s = String(b, pos + 1, n, Charsets.UTF_8)
            pos += 1 + n
            return s
        }
        val id = str() ?: return null
        val name = str() ?: return null
        if (pos + 2 > b.size || id.isEmpty()) return null
        val port = (b[pos].toInt() and 0xFF) or ((b[pos + 1].toInt() and 0xFF) shl 8)
        return if (port == 0) null else FoundHost(id, name, from, port)
    }
}
