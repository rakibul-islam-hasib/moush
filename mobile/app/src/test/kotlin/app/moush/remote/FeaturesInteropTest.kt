package app.moush.remote

import app.moush.remote.net.FileSender
import app.moush.remote.net.Identity
import app.moush.remote.net.Msg
import app.moush.remote.net.Noise
import app.moush.remote.net.Protocol
import app.moush.remote.net.SecureChannel
import app.moush.remote.net.SourceEntry
import app.moush.remote.net.TMsg
import app.moush.remote.net.TransferWire
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.InetSocketAddress
import java.net.Socket
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.MessageDigest
import java.util.concurrent.TimeUnit
import kotlin.io.path.createTempDirectory

/** Files and screen streaming against a real `moush-cli host` (set MOUSH_CLI to run). */
class FeaturesInteropTest {
    private fun connect(port: Int, id: Identity, announce: Int): Pair<Socket, SecureChannel> {
        val s = Socket()
        s.tcpNoDelay = true
        s.connect(InetSocketAddress("127.0.0.1", port), 3000)
        s.soTimeout = 15000
        val ch = Noise.handshake(s.getInputStream(), s.getOutputStream(), id)
        ch.write(Protocol.encode(Msg.Hello(Protocol.VERSION, Protocol.ROLE_REMOTE, "Test Phone", true)))
        val got = ArrayList<Msg>()
        while (got.none { it is Msg.Authorize }) got += Protocol.decodeAll(ch.read())
        ch.write(Protocol.encode(Msg.Authorize(true), Msg.Capabilities(announce)))
        return s to ch
    }

    private fun startHost(id: Identity, port: Int, recv: File?): Triple<Process, StringBuffer, File> {
        val cli = System.getenv("MOUSH_CLI")
        val dir = createTempDirectory("moush-e2e").toFile()
        File(dir, "trusted.json").writeText(
            """[{"id":"${id.deviceId}","publicKey":"${Noise.hex(id.publicKey)}","name":"Test Phone","role":"remote","pairedAt":1}]""",
        )
        val args = mutableListOf(cli, "host", "--config-dir", dir.absolutePath, "--port", port.toString())
        if (recv != null) args += listOf("--receive-dir", recv.absolutePath)
        val proc = ProcessBuilder(args).redirectErrorStream(true).start()
        val log = StringBuffer()
        Thread { proc.inputStream.bufferedReader().forEachLine { log.append(it).append('\n') } }.apply { isDaemon = true }.start()
        var waited = 0
        while (!log.contains("hosting on port") && waited < 100) {
            Thread.sleep(100)
            waited++
        }
        assertTrue("host did not start: $log", log.contains("hosting on port"))
        return Triple(proc, log, dir)
    }

    private fun sha(b: ByteArray) = Noise.hex(MessageDigest.getInstance("SHA-256").digest(b))

    private fun hostAvailable(): Boolean {
        val cli = System.getenv("MOUSH_CLI")
        return cli != null && File(cli).exists()
    }

    @Test
    fun sendsFilesAndFoldersToRealHost() {
        assumeTrue(hostAvailable())
        val id = Identity.generate()
        val recv = createTempDirectory("moush-recv").toFile()
        val (proc, log, _) = startHost(id, 24897, recv)
        try {
            val rnd = java.util.Random(7)
            val big = ByteArray(700_000).also { rnd.nextBytes(it) }
            val small = "hello from the phone\n".toByteArray()
            val nested = ByteArray(33_000).also { rnd.nextBytes(it) }
            val entries = listOf(
                SourceEntry("Trip/notes.txt", small.size.toLong(), false) { small.inputStream() },
                SourceEntry("Trip/photos/big.bin", big.size.toLong(), false) { big.inputStream() },
                SourceEntry("Trip/photos/deep/nested.bin", nested.size.toLong(), false) { nested.inputStream() },
                SourceEntry("Trip/empty.txt", 0, false) { ByteArray(0).inputStream() },
                SourceEntry("Trip/emptydir", 0, true) { error("dir") },
            )
            val (sock, ch) = connect(24897, id, Protocol.CAP_FILES)
            sock.use {
                val inbox = Channel<TMsg>(Channel.UNLIMITED)
                val reader = Thread {
                    try {
                        while (true) for (m in Protocol.decodeAll(ch.read())) {
                            if (m is Msg.Feature && m.channel == Protocol.CHANNEL_FILES) TransferWire.decode(m.body)?.let { inbox.trySend(it) }
                            if (m is Msg.Ping) ch.write(Protocol.encode(Msg.Pong(m.id, m.tUs)))
                        }
                    } catch (_: Throwable) {
                    }
                }.apply { isDaemon = true; start() }
                var progress = 0L
                runBlocking {
                    FileSender(0x1234_5678_9ABCL, entries, { ch.write(Protocol.encode(Msg.Feature(Protocol.CHANNEL_FILES, it))) }, inbox) { d, _ -> progress = d }.run()
                }
                assertEquals((small.size + big.size + nested.size).toLong(), progress)
                reader.interrupt()
            }
            val root = File(recv, "Trip")
            assertTrue("received: ${recv.walkTopDown().toList()}\n$log", root.isDirectory)
            assertEquals(sha(small), sha(File(root, "notes.txt").readBytes()))
            assertEquals(sha(big), sha(File(root, "photos/big.bin").readBytes()))
            assertEquals(sha(nested), sha(File(root, "photos/deep/nested.bin").readBytes()))
            assertEquals(0L, File(root, "empty.txt").length())
            assertTrue(File(root, "emptydir").isDirectory)
        } finally {
            proc.destroy()
            proc.waitFor(3, TimeUnit.SECONDS)
        }
    }

    @Test
    fun streamsTheScreenFromRealHost() {
        assumeTrue(hostAvailable())
        val id = Identity.generate()
        val (proc, log, _) = startHost(id, 24896, null)
        try {
            val (sock, ch) = connect(24896, id, Protocol.CAP_FILES)
            sock.use {
                fun feature(body: ByteArray) = ch.write(Protocol.encode(Msg.Feature(Protocol.CHANNEL_SCREEN, body)))
                fun le(n: Int) = ByteBuffer.allocate(n).order(ByteOrder.LITTLE_ENDIAN)
                feature(byteArrayOf(1))
                var displays = 0
                var total = -1
                val jpeg = ByteArrayOutputStream()
                var got = 0
                var seq = -1
                val end = System.currentTimeMillis() + 15000
                var asked = false
                while (System.currentTimeMillis() < end && (total < 0 || got < total)) {
                    for (m in Protocol.decodeAll(ch.read())) {
                        if (m is Msg.Ping) ch.write(Protocol.encode(Msg.Pong(m.id, m.tUs)))
                        if (m !is Msg.Feature || m.channel != Protocol.CHANNEL_SCREEN) continue
                        val b = ByteBuffer.wrap(m.body).order(ByteOrder.LITTLE_ENDIAN)
                        when (b.get().toInt()) {
                            10 -> {
                                displays = b.get().toInt()
                                assertTrue("no displays", displays >= 1)
                                if (!asked) {
                                    asked = true
                                    feature(le(1 + 1 + 16 + 4 + 2).put(2).put(0).putInt(0).putInt(0).putInt(0).putInt(0).putShort(640).putShort(360).put(50).put(10).array())
                                }
                            }
                            11 -> {
                                seq = b.int
                                b.position(b.position() + 1 + 16 + 8 + 4)
                                total = b.int
                                val off = b.int
                                val len = b.int
                                val data = ByteArray(len).also { b.get(it) }
                                assertEquals(got, off)
                                jpeg.write(data)
                                got += len
                            }
                        }
                    }
                }
                assertTrue("never got a frame; host log: $log", total > 0 && got == total)
                val bytes = jpeg.toByteArray()
                assertEquals(0xFF, bytes[0].toInt() and 0xFF)
                assertEquals(0xD8, bytes[1].toInt() and 0xFF)
                println("SCREEN frame ok: $displays display(s), ${bytes.size} bytes, seq=$seq")
                feature(le(5).put(4).putInt(seq).array())
                feature(byteArrayOf(3))
            }
        } finally {
            proc.destroy()
            proc.waitFor(3, TimeUnit.SECONDS)
        }
    }
}
