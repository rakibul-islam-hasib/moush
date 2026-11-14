package app.moush.remote.net

import org.bouncycastle.crypto.digests.Blake2sDigest
import org.bouncycastle.crypto.macs.HMac
import org.bouncycastle.crypto.modes.ChaCha20Poly1305
import org.bouncycastle.crypto.params.AEADParameters
import org.bouncycastle.crypto.params.KeyParameter
import org.bouncycastle.math.ec.rfc7748.X25519
import java.io.EOFException
import java.io.InputStream
import java.io.OutputStream
import java.security.SecureRandom

/** Device identity: a long-term X25519 key pair, same format as the desktop app (private + public). */
class Identity(val privateKey: ByteArray, val publicKey: ByteArray) {
    val deviceId: String get() = Noise.deviceId(publicKey)
    fun toBytes(): ByteArray = privateKey + publicKey

    companion object {
        fun generate(): Identity {
            val priv = ByteArray(32).also { SecureRandom().nextBytes(it) }
            val pub = ByteArray(32)
            X25519.scalarMultBase(priv, 0, pub, 0)
            return Identity(priv, pub)
        }

        fun fromBytes(b: ByteArray): Identity? =
            if (b.size == 64) Identity(b.copyOfRange(0, 32), b.copyOfRange(32, 64)) else null
    }
}

/**
 * `Noise_XX_25519_ChaChaPoly_BLAKE2s` with prologue `Moush/1`: the exact handshake the
 * desktop app (snow) runs. The phone is always the initiator.
 */
object Noise {
    private const val NAME = "Noise_XX_25519_ChaChaPoly_BLAKE2s"
    private val PROLOGUE = "Moush/1".toByteArray()
    const val MAX_MSG = 65535
    const val TAG = 16
    const val MAX_FRAME_PLAINTEXT = MAX_MSG - TAG

    fun blake2s(vararg parts: ByteArray): ByteArray {
        val d = Blake2sDigest(256)
        for (p in parts) d.update(p, 0, p.size)
        return ByteArray(32).also { d.doFinal(it, 0) }
    }

    fun hex(b: ByteArray) = b.joinToString("") { "%02x".format(it) }

    fun unhex(s: String): ByteArray? =
        if (s.length % 2 != 0) null
        else runCatching { ByteArray(s.length / 2) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() } }.getOrNull()

    fun deviceId(pub: ByteArray): String = hex(blake2s(pub).copyOf(8))

    fun fingerprint(pub: ByteArray): String = deviceId(pub).chunked(4).joinToString("-")

    private fun hmac(key: ByteArray, data: ByteArray): ByteArray {
        val m = HMac(Blake2sDigest(256))
        m.init(KeyParameter(key))
        m.update(data, 0, data.size)
        return ByteArray(32).also { m.doFinal(it, 0) }
    }

    private fun hkdf(ck: ByteArray, ikm: ByteArray, n: Int): List<ByteArray> {
        val temp = hmac(ck, ikm)
        val out = ArrayList<ByteArray>()
        var prev = ByteArray(0)
        for (i in 1..n) {
            prev = hmac(temp, prev + byteArrayOf(i.toByte()))
            out += prev
        }
        return out
    }

    private fun nonce(n: Long): ByteArray {
        val b = ByteArray(12)
        for (i in 0 until 8) b[4 + i] = (n ushr (8 * i)).toByte()
        return b
    }

    fun encrypt(key: ByteArray, n: Long, ad: ByteArray, plain: ByteArray): ByteArray {
        val c = ChaCha20Poly1305()
        c.init(true, AEADParameters(KeyParameter(key), 128, nonce(n), ad))
        val out = ByteArray(c.getOutputSize(plain.size))
        val len = c.processBytes(plain, 0, plain.size, out, 0)
        c.doFinal(out, len)
        return out
    }

    fun decrypt(key: ByteArray, n: Long, ad: ByteArray, cipher: ByteArray): ByteArray {
        val c = ChaCha20Poly1305()
        c.init(false, AEADParameters(KeyParameter(key), 128, nonce(n), ad))
        val out = ByteArray(c.getOutputSize(cipher.size))
        val len = c.processBytes(cipher, 0, cipher.size, out, 0)
        c.doFinal(out, len)
        return out
    }

    private fun dh(priv: ByteArray, pub: ByteArray): ByteArray {
        val out = ByteArray(32)
        check(X25519.calculateAgreement(priv, 0, pub, 0, out, 0)) { "bad key exchange" }
        return out
    }

    private class Sym {
        var ck: ByteArray
        var h: ByteArray
        var k: ByteArray? = null
        var n = 0L

        init {
            val name = NAME.toByteArray()
            h = if (name.size <= 32) name.copyOf(32) else blake2s(name)
            ck = h
            mixHash(PROLOGUE)
        }

        fun mixHash(d: ByteArray) {
            h = blake2s(h, d)
        }

        fun mixKey(ikm: ByteArray) {
            val out = hkdf(ck, ikm, 2)
            ck = out[0]
            k = out[1]
            n = 0
        }

        fun encryptAndHash(p: ByteArray): ByteArray {
            val key = k
            if (key == null) {
                mixHash(p)
                return p
            }
            val c = encrypt(key, n++, h, p)
            mixHash(c)
            return c
        }

        fun decryptAndHash(c: ByteArray): ByteArray {
            val key = k
            if (key == null) {
                mixHash(c)
                return c
            }
            val p = decrypt(key, n++, h, c)
            mixHash(c)
            return p
        }
    }

    /** Runs the initiator side over a raw stream (frames are `[u16 LE length][bytes]`). */
    fun handshake(input: InputStream, output: OutputStream, me: Identity): SecureChannel {
        val s = Sym()
        val ePriv = ByteArray(32).also { SecureRandom().nextBytes(it) }
        val ePub = ByteArray(32).also { X25519.scalarMultBase(ePriv, 0, it, 0) }

        // -> e
        s.mixHash(ePub)
        writeRaw(output, ePub + s.encryptAndHash(ByteArray(0)))

        // <- e, ee, s, es
        val m2 = readRaw(input)
        require(m2.size >= 32 + 48 + 16) { "short handshake reply" }
        val re = m2.copyOfRange(0, 32)
        s.mixHash(re)
        s.mixKey(dh(ePriv, re))
        val rs = s.decryptAndHash(m2.copyOfRange(32, 80))
        s.mixKey(dh(ePriv, rs))
        s.decryptAndHash(m2.copyOfRange(80, m2.size))

        // -> s, se
        val sEnc = s.encryptAndHash(me.publicKey)
        s.mixKey(dh(me.privateKey, re))
        val pay = s.encryptAndHash(ByteArray(0))
        writeRaw(output, sEnc + pay)

        val keys = hkdf(s.ck, ByteArray(0), 2)
        return SecureChannel(input, output, keys[0], keys[1], rs, s.h)
    }

    fun writeRaw(out: OutputStream, data: ByteArray) {
        val buf = ByteArray(2 + data.size)
        buf[0] = data.size.toByte()
        buf[1] = (data.size ushr 8).toByte()
        System.arraycopy(data, 0, buf, 2, data.size)
        out.write(buf)
        out.flush()
    }

    fun readRaw(input: InputStream): ByteArray {
        val l = readFully(input, 2)
        val n = (l[0].toInt() and 0xFF) or ((l[1].toInt() and 0xFF) shl 8)
        require(n > 0) { "empty frame" }
        return readFully(input, n)
    }

    private fun readFully(input: InputStream, n: Int): ByteArray {
        val b = ByteArray(n)
        var off = 0
        while (off < n) {
            val r = input.read(b, off, n - off)
            if (r < 0) throw EOFException("connection closed")
            off += r
        }
        return b
    }
}

/** Established channel with independent send and receive nonce counters. */
class SecureChannel(
    private val input: InputStream,
    private val output: OutputStream,
    private val sendKey: ByteArray,
    private val recvKey: ByteArray,
    val remoteStatic: ByteArray,
    private val handshakeHash: ByteArray,
) {
    private var sendN = 0L
    private var recvN = 0L

    /** The 6-digit code both screens show; a man in the middle ends up with different codes. */
    val verificationCode: String
        get() {
            val d = Noise.blake2s("pairing-code".toByteArray(), handshakeHash)
            val n = ((d[0].toLong() and 0xFF) or ((d[1].toLong() and 0xFF) shl 8) or
                ((d[2].toLong() and 0xFF) shl 16) or ((d[3].toLong() and 0xFF) shl 24)) % 1_000_000L
            return "%06d".format(n)
        }

    @Synchronized
    fun write(plain: ByteArray) {
        var off = 0
        while (off < plain.size) {
            val len = minOf(Noise.MAX_FRAME_PLAINTEXT, plain.size - off)
            val c = Noise.encrypt(sendKey, sendN++, ByteArray(0), plain.copyOfRange(off, off + len))
            Noise.writeRaw(output, c)
            off += len
        }
    }

    /** Blocks for the next frame and returns its plaintext. */
    fun read(): ByteArray = Noise.decrypt(recvKey, recvN++, ByteArray(0), Noise.readRaw(input))
}
