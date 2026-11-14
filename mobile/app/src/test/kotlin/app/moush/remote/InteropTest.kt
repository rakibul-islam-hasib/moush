package app.moush.remote

import app.moush.remote.net.Identity
import app.moush.remote.net.Msg
import app.moush.remote.net.Noise
import app.moush.remote.net.Protocol
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.TimeUnit
import kotlin.io.path.createTempDirectory

/** Talks to a real `moush-cli host` over loopback. Runs only when MOUSH_CLI points at the binary. */
class InteropTest {
    private fun cursor(): String {
        val ps = ProcessBuilder("powershell", "-NoProfile", "-Command", "Add-Type -AssemblyName System.Windows.Forms; \$p=[System.Windows.Forms.Cursor]::Position; Write-Output (\"\$(\$p.X),\$(\$p.Y)\")").start()
        return ps.inputStream.bufferedReader().readText().trim()
    }

    @Test
    fun encodedSizesMatchTheDesktopProtocol() {
        assertEquals(5, Protocol.encode(Msg.MouseRel(-3, 7)).size)
        assertEquals(3, Protocol.encode(Msg.Button(1, true)).size)
        assertEquals(4, Protocol.encode(Msg.Wheel(true, -120)).size)
        assertEquals(6, Protocol.encode(Msg.Key(0x41, 0x1E, 1)).size)
        assertEquals(13, Protocol.encode(Msg.Ping(9, 123L)).size)
        assertEquals(1, Protocol.encode(Msg.ReleaseAll).size)
        // Host-side messages decode: Hello + Authorize in one frame.
        val frame = byteArrayOf(1, 2, 0, 1, 1, 'H'.code.toByte(), 1, 2, 1)
        assertEquals(2, Protocol.decodeAll(frame).size)
    }

    @Test
    fun handshakeWithRealHost() {
        val cli = System.getenv("MOUSH_CLI")
        assumeTrue(cli != null && File(cli).exists())
        val id = Identity.generate()
        val dir = createTempDirectory("moush-e2e").toFile()
        val pub = Noise.hex(id.publicKey)
        File(dir, "trusted.json").writeText(
            """[{"id":"${id.deviceId}","publicKey":"$pub","name":"Test Phone","role":"remote","pairedAt":1}]""",
        )
        val proc = ProcessBuilder(cli, "host", "--config-dir", dir.absolutePath, "--port", "24899").redirectErrorStream(true).start()
        val log = StringBuffer()
        Thread { proc.inputStream.bufferedReader().forEachLine { log.append(it).append('\n') } }.apply { isDaemon = true }.start()
        try {
            var waited = 0
            while (!log.contains("hosting on port") && waited < 100) { Thread.sleep(100); waited++ }
            assertTrue("host did not start: $log", log.contains("hosting on port"))

            Socket().use { s ->
                s.tcpNoDelay = true
                s.connect(InetSocketAddress("127.0.0.1", 24899), 3000)
                s.soTimeout = 5000
                val ch = Noise.handshake(s.getInputStream(), s.getOutputStream(), id)
                assertEquals(6, ch.verificationCode.length)
                ch.write(Protocol.encode(Msg.Hello(Protocol.VERSION, Protocol.ROLE_REMOTE, "Test Phone", true)))
                val got = ArrayList<Msg>()
                while (got.none { it is Msg.Authorize }) got += Protocol.decodeAll(ch.read())
                val hello = got.filterIsInstance<Msg.Hello>().first()
                assertEquals(Protocol.ROLE_HOST, hello.role)
                assertTrue(hello.trustsPeer)
                assertTrue((got.first { it is Msg.Authorize } as Msg.Authorize).accept)
                ch.write(Protocol.encode(Msg.Authorize(true)))
                val before = cursor()
                ch.write(Protocol.encode(Msg.Ping(42, 1L)))
                repeat(40) { ch.write(Protocol.encode(Msg.MouseRel(5, 0))); Thread.sleep(8) }
                Thread.sleep(200)
                val after = cursor()
                println("CURSOR before=$before after=$after")
                repeat(40) { ch.write(Protocol.encode(Msg.MouseRel(-5, 0))); Thread.sleep(8) }
                var pong: Msg.Pong? = null
                while (pong == null) pong = Protocol.decodeAll(ch.read()).filterIsInstance<Msg.Pong>().firstOrNull()
                assertEquals(42, pong.id)
                Thread.sleep(300)
            }
            Thread.sleep(300)
            assertTrue("host log: $log", log.contains("remote Test Phone connected"))
            println(log)
        } finally {
            proc.destroy()
            proc.waitFor(3, TimeUnit.SECONDS)
        }
    }
}
