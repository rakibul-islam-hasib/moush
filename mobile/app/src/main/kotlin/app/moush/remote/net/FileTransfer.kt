package app.moush.remote.net

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.withTimeoutOrNull
import org.bouncycastle.crypto.digests.Blake2sDigest
import java.io.InputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** One entry of an offer: a file with a way to open it, or an (empty) folder. */
class SourceEntry(val path: String, val size: Long, val isDir: Boolean, val open: () -> InputStream)

/** Messages on the files channel, as the sender sees them. */
sealed interface TMsg {
    data class Accept(val last: Boolean, val resume: List<Resume>) : TMsg
    data class Reject(val reason: String) : TMsg
    data class Ack(val bytes: Int) : TMsg
    data object Complete : TMsg
    data class Cancel(val reason: String) : TMsg
    data class Error(val reason: String) : TMsg

    /** The computer offers files to the phone. */
    data object Offer : TMsg
}

class Resume(val index: Int, val offset: Long, val prefix: ByteArray)

/** Wire format of `moush-core::transfer` (channel 2). */
object TransferWire {
    private const val OFFER = 1
    private const val ACCEPT = 2
    private const val REJECT = 3
    private const val DATA = 4
    private const val FILE_DONE = 5
    private const val ACK = 6
    private const val COMPLETE = 7
    private const val CANCEL = 8
    private const val ERROR = 9
    private const val OFFER_V2 = 10

    const val MAX_BODY = 32 * 1024
    const val DATA_HEADER = 1 + 8 + 4 + 8 + 4
    const val CHUNK = MAX_BODY - DATA_HEADER
    const val MAX_FILES = 10_000
    const val SKIP = -1L // u64::MAX on the wire

    private fun le(n: Int) = ByteBuffer.allocate(n).order(ByteOrder.LITTLE_ENDIAN)

    /** Splits the entry list into offer bodies that each fit one feature body. */
    fun offerParts(id: Long, entries: List<SourceEntry>): List<ByteArray> {
        val totalBytes = entries.sumOf { it.size }
        val parts = ArrayList<ByteArray>()
        var first = 0
        while (first < entries.size) {
            var used = 27
            var end = first
            while (end < entries.size && end - first < 60_000) {
                val need = 4 + entries[end].path.toByteArray(Charsets.UTF_8).size + 8 + 1
                if (used + need > MAX_BODY - 16 && end > first) break
                used += need
                end++
            }
            val b = le(used)
            b.put(OFFER_V2.toByte()).putLong(id).putInt(entries.size).putLong(totalBytes).putInt(first).putShort((end - first).toShort())
            for (i in first until end) {
                val p = entries[i].path.toByteArray(Charsets.UTF_8)
                b.putInt(p.size).put(p).putLong(entries[i].size).put(if (entries[i].isDir) 1 else 0)
            }
            parts += b.array()
            first = end
        }
        return parts
    }

    fun data(id: Long, index: Int, offset: Long, buf: ByteArray, n: Int): ByteArray {
        val b = le(DATA_HEADER + n)
        b.put(DATA.toByte()).putLong(id).putInt(index).putLong(offset).putInt(n).put(buf, 0, n)
        return b.array()
    }

    fun fileDone(id: Long, index: Int, size: Long, hash: ByteArray): ByteArray =
        le(1 + 8 + 4 + 8 + 32).put(FILE_DONE.toByte()).putLong(id).putInt(index).putLong(size).put(hash).array()

    fun cancel(id: Long, reason: String): ByteArray {
        val r = reason.take(100).toByteArray(Charsets.UTF_8).let { if (it.size > 255) it.copyOf(255) else it }
        return le(1 + 8 + 1 + r.size).put(CANCEL.toByte()).putLong(id).put(r.size.toByte()).put(r).array()
    }

    fun reject(id: Long, reason: String): ByteArray {
        val r = reason.toByteArray(Charsets.UTF_8).let { if (it.size > 255) it.copyOf(255) else it }
        return le(1 + 8 + 1 + r.size).put(REJECT.toByte()).putLong(id).put(r.size.toByte()).put(r).array()
    }

    /** The transfer id every body starts with (after its kind byte). */
    fun idOf(body: ByteArray): Long? =
        if (body.size >= 9) ByteBuffer.wrap(body, 1, 8).order(ByteOrder.LITTLE_ENDIAN).long else null

    fun decode(body: ByteArray): TMsg? = runCatching {
        val b = ByteBuffer.wrap(body).order(ByteOrder.LITTLE_ENDIAN)
        val kind = b.get().toInt() and 0xFF
        b.long // id
        fun str(): String {
            val n = b.get().toInt() and 0xFF
            val a = ByteArray(n)
            b.get(a)
            return String(a, Charsets.UTF_8)
        }
        when (kind) {
            ACCEPT -> {
                val last = b.get().toInt() == 1
                val count = b.short.toInt() and 0xFFFF
                val list = ArrayList<Resume>()
                repeat(count) {
                    val idx = b.int
                    val off = b.long
                    val prefix = ByteArray(32)
                    b.get(prefix)
                    list += Resume(idx, off, prefix)
                }
                TMsg.Accept(last, list)
            }
            REJECT -> TMsg.Reject(str())
            ACK -> TMsg.Ack(b.int)
            COMPLETE -> TMsg.Complete
            CANCEL -> TMsg.Cancel(str())
            ERROR -> TMsg.Error(str())
            OFFER, OFFER_V2 -> TMsg.Offer
            else -> null
        }
    }.getOrNull()

    /** Windows-safe path: forbidden characters become `_`, every part is trimmed. */
    fun cleanPath(raw: String): String =
        raw.split('/').filter { it.isNotEmpty() && it != "." && it != ".." }.joinToString("/") { part ->
            part.map { c -> if (c.isISOControl() || c in "<>:\"\\|?*") '_' else c }.joinToString("").trimEnd('.', ' ').take(150).ifEmpty { "_" }
        }
}

class TransferFailed(message: String, val kind: Kind) : Exception(message) {
    enum class Kind { Rejected, Cancelled, Failed }
}

/**
 * Sends one offer. Runs as a coroutine; [inbox] carries the computer's answers for this
 * transfer id. At most [WINDOW] bytes are unacknowledged, so a slow disk on the other side
 * slows the phone down instead of filling memory.
 */
class FileSender(
    private val id: Long,
    private val entries: List<SourceEntry>,
    private val send: (ByteArray) -> Unit,
    private val inbox: Channel<TMsg>,
    private val onProgress: (doneBytes: Long, accepted: Boolean) -> Unit,
) {
    companion object {
        const val WINDOW = 128 * 1024
        const val ANSWER_TIMEOUT = 180_000L
        const val ACK_TIMEOUT = 30_000L
        const val COMPLETE_TIMEOUT = 300_000L
    }

    private var done = 0L
    private var inFlight = 0L

    private suspend fun next(timeoutMs: Long, what: String): TMsg =
        withTimeoutOrNull(timeoutMs) { inbox.receive() } ?: throw TransferFailed(what, TransferFailed.Kind.Failed)

    private fun fail(m: TMsg): Nothing = when (m) {
        is TMsg.Reject -> throw TransferFailed(m.reason.ifBlank { "The computer declined the files." }, TransferFailed.Kind.Rejected)
        is TMsg.Cancel -> throw TransferFailed("Cancelled on the computer.", TransferFailed.Kind.Cancelled)
        is TMsg.Error -> throw TransferFailed(m.reason.ifBlank { "The computer reported an error." }, TransferFailed.Kind.Failed)
        else -> throw TransferFailed("Unexpected answer from the computer.", TransferFailed.Kind.Failed)
    }

    private fun absorb(m: TMsg) {
        when (m) {
            is TMsg.Ack -> {
                inFlight = (inFlight - m.bytes).coerceAtLeast(0)
                done += m.bytes
                onProgress(done, true)
            }
            is TMsg.Reject, is TMsg.Cancel, is TMsg.Error -> fail(m)
            else -> {}
        }
    }

    suspend fun run() {
        if (entries.isEmpty() || entries.size > TransferWire.MAX_FILES) {
            throw TransferFailed("Too many files in one go (limit ${TransferWire.MAX_FILES}).", TransferFailed.Kind.Failed)
        }
        for (p in TransferWire.offerParts(id, entries)) send(p)

        // The computer may take a while to answer: its user might have to approve.
        val starts = LongArray(entries.size)
        val prefixes = arrayOfNulls<ByteArray>(entries.size)
        while (true) {
            when (val m = next(ANSWER_TIMEOUT, "The computer did not answer.")) {
                is TMsg.Accept -> {
                    for (r in m.resume) {
                        if (r.index in entries.indices) {
                            starts[r.index] = r.offset
                            prefixes[r.index] = r.prefix
                        }
                    }
                    if (m.last) break
                }
                else -> fail(m)
            }
        }
        onProgress(0, true)

        val buf = ByteArray(TransferWire.CHUNK)
        for ((i, e) in entries.withIndex()) {
            if (e.isDir) continue
            if (starts[i] == TransferWire.SKIP) {
                done += e.size
                onProgress(done, true)
                continue
            }
            var hasher = Blake2sDigest(256)
            var pos = 0L
            var input = e.open()
            try {
                val resume = starts[i]
                if (resume in 1..e.size) {
                    // Continue only if our own prefix hashes to what the computer holds.
                    val own = Blake2sDigest(256)
                    var left = resume
                    while (left > 0) {
                        val n = input.read(buf, 0, minOf(buf.size.toLong(), left).toInt())
                        if (n < 0) break
                        own.update(buf, 0, n)
                        left -= n
                    }
                    val mine = ByteArray(32).also { Blake2sDigest(own).doFinal(it, 0) }
                    if (left == 0L && mine.contentEquals(prefixes[i])) {
                        hasher = own
                        pos = resume
                    } else {
                        input.close()
                        input = e.open()
                    }
                }
                sendRest(i, e, input, hasher, pos, buf)
            } finally {
                runCatching { input.close() }
            }
        }

        // The computer verifies, flushes and renames, then confirms.
        while (true) {
            when (val m = next(COMPLETE_TIMEOUT, "The computer did not confirm the transfer.")) {
                TMsg.Complete -> return
                else -> absorb(m)
            }
        }
    }

    private suspend fun sendRest(index: Int, e: SourceEntry, input: InputStream, hasher: Blake2sDigest, startPos: Long, buf: ByteArray) {
        var pos = startPos
        done += pos
        while (pos < e.size) {
            val n = minOf(TransferWire.CHUNK.toLong(), e.size - pos).toInt()
            var got = 0
            while (got < n) {
                val r = input.read(buf, got, n - got)
                if (r < 0) throw TransferFailed("${e.path} changed while sending.", TransferFailed.Kind.Failed)
                got += r
            }
            hasher.update(buf, 0, n)
            while (true) {
                inbox.tryReceive().getOrNull()?.let { absorb(it) } ?: break
            }
            while (inFlight > 0 && inFlight + n > WINDOW) absorb(next(ACK_TIMEOUT, "The computer stopped responding."))
            send(TransferWire.data(id, index, pos, buf, n))
            inFlight += n
            pos += n
        }
        val hash = ByteArray(32).also { hasher.doFinal(it, 0) }
        send(TransferWire.fileDone(id, index, e.size, hash))
    }
}
