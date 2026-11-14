package app.moush.remote.data

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import app.moush.remote.net.FileSender
import app.moush.remote.net.Protocol
import app.moush.remote.net.Session
import app.moush.remote.net.SourceEntry
import app.moush.remote.net.TMsg
import app.moush.remote.net.TransferFailed
import app.moush.remote.net.TransferWire
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.BufferedInputStream
import java.util.concurrent.ConcurrentHashMap

enum class TState { Preparing, Waiting, Sending, Done, Failed, Rejected, Cancelled }

data class TransferItem(
    val id: Long,
    val title: String,
    val files: Int,
    val totalBytes: Long,
    val doneBytes: Long,
    val state: TState,
    val message: String? = null,
    val bytesPerSec: Double = 0.0,
    val folder: Boolean = false,
) {
    val fraction: Float get() = if (totalBytes <= 0) (if (state == TState.Done) 1f else 0f) else (doneBytes.toFloat() / totalBytes).coerceIn(0f, 1f)
    val active: Boolean get() = state == TState.Preparing || state == TState.Waiting || state == TState.Sending
}

/** Sends files and folders from the phone to the connected computer. */
class Transfers(private val ctx: Context, private val session: Session, private val scope: CoroutineScope) {
    private val _items = MutableStateFlow<List<TransferItem>>(emptyList())
    val items: StateFlow<List<TransferItem>> = _items.asStateFlow()

    private val inboxes = ConcurrentHashMap<Long, Channel<TMsg>>()
    private val jobs = ConcurrentHashMap<Long, Job>()

    init {
        scope.launch {
            session.features.collect { f ->
                if (f.channel != Protocol.CHANNEL_FILES) return@collect
                val id = TransferWire.idOf(f.body) ?: return@collect
                when (val m = TransferWire.decode(f.body)) {
                    null -> {}
                    TMsg.Offer -> session.sendFeature(
                        Protocol.CHANNEL_FILES,
                        TransferWire.reject(id, "This phone can send files, but cannot receive them yet."),
                    )
                    else -> inboxes[id]?.trySend(m)
                }
            }
        }
    }

    private fun patch(id: Long, f: (TransferItem) -> TransferItem) = _items.update { l -> l.map { if (it.id == id) f(it) else it } }

    fun cancel(id: Long) {
        session.sendFeature(Protocol.CHANNEL_FILES, TransferWire.cancel(id, "Cancelled on the phone."))
        jobs[id]?.cancel()
        patch(id) { if (it.active) it.copy(state = TState.Cancelled, message = "Cancelled.") else it }
    }

    fun clearFinished() = _items.update { l -> l.filter { it.active } }

    /** Individually picked files (document picker or the Android share sheet). */
    fun sendFiles(uris: List<Uri>) = start(uris.size.toString(), folder = false) { uris.mapNotNull { fileEntry(it) } }

    /** A whole folder (document tree) with its sub-folders. */
    fun sendFolder(tree: Uri) = start("folder", folder = true) { folderEntries(tree) }

    private fun start(label: String, folder: Boolean, collect: suspend () -> List<SourceEntry>) {
        val id = java.security.SecureRandom().nextLong() and Long.MAX_VALUE
        val placeholder = TransferItem(id, if (folder) "Reading folder…" else "Preparing…", 0, 0, 0, TState.Preparing, folder = folder)
        _items.update { listOf(placeholder) + it }
        val job = scope.launch(Dispatchers.IO) { run(id, label, folder, collect) }
        jobs[id] = job
        job.invokeOnCompletion { jobs.remove(id); inboxes.remove(id) }
    }

    private suspend fun run(id: Long, label: String, folder: Boolean, collect: suspend () -> List<SourceEntry>) {
        try {
            if (session.peerCaps.value and Protocol.CAP_FILES == 0) {
                throw TransferFailed("File transfer is turned off for this phone on the computer (Moush › Devices).", TransferFailed.Kind.Failed)
            }
            val entries = unique(collect())
            val files = entries.count { !it.isDir }
            if (files == 0 && entries.none { it.isDir }) throw TransferFailed("Moush could not read that file. Pick it again with the Files button.", TransferFailed.Kind.Failed)
            val total = entries.sumOf { it.size }
            val title = when {
                folder -> entries.firstOrNull()?.path?.substringBefore('/') ?: "Folder"
                files == 1 -> entries.first { !it.isDir }.path
                else -> "$files files"
            }
            patch(id) { it.copy(title = title, files = files, totalBytes = total, state = TState.Waiting, message = "Waiting for the computer…") }

            val inbox = Channel<TMsg>(Channel.UNLIMITED)
            inboxes[id] = inbox
            var lastT = System.nanoTime()
            var lastB = 0L
            var speed = 0.0
            val sender = FileSender(id, entries, { session.sendFeature(Protocol.CHANNEL_FILES, it) }, inbox) { done, accepted ->
                val now = System.nanoTime()
                if (now - lastT > 400_000_000L) {
                    val inst = (done - lastB) * 1e9 / (now - lastT)
                    speed = if (speed == 0.0) inst else speed * 0.6 + inst * 0.4
                    lastT = now
                    lastB = done
                }
                patch(id) { it.copy(doneBytes = done, state = if (accepted) TState.Sending else it.state, message = null, bytesPerSec = speed) }
            }
            sender.run()
            patch(id) { it.copy(doneBytes = it.totalBytes, state = TState.Done, message = null, bytesPerSec = 0.0) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: TransferFailed) {
            val st = when (e.kind) {
                TransferFailed.Kind.Rejected -> TState.Rejected
                TransferFailed.Kind.Cancelled -> TState.Cancelled
                TransferFailed.Kind.Failed -> TState.Failed
            }
            patch(id) { it.copy(title = if (it.title.endsWith("…")) "Files" else it.title, state = st, message = e.message, bytesPerSec = 0.0) }
            if (st == TState.Failed) runCatching { session.sendFeature(Protocol.CHANNEL_FILES, TransferWire.cancel(id, "Failed on the phone.")) }
        } catch (e: Throwable) {
            patch(id) { it.copy(title = if (it.title.endsWith("…")) "Files" else it.title, state = TState.Failed, message = e.message ?: "Could not read the files.", bytesPerSec = 0.0) }
        }
    }

    // ---- reading entries -------------------------------------------------------------------

    private val resolver: ContentResolver get() = ctx.contentResolver

    private fun fileEntry(uri: Uri): SourceEntry? {
        var name: String? = null
        var size = -1L
        runCatching {
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
                if (c.moveToFirst()) {
                    name = c.getString(0)
                    size = if (c.isNull(1)) -1 else c.getLong(1)
                }
            }
        }
        if (size < 0) size = runCatching { resolver.openAssetFileDescriptor(uri, "r")?.use { it.length } ?: -1L }.getOrDefault(-1L)
        if (size < 0) return null
        val clean = TransferWire.cleanPath(name ?: uri.lastPathSegment ?: "file")
        return SourceEntry(clean, size, false) { BufferedInputStream(resolver.openInputStream(uri) ?: error("Cannot open $clean"), 64 * 1024) }
    }

    private suspend fun folderEntries(tree: Uri): List<SourceEntry> = withContext(Dispatchers.IO) {
        val rootId = DocumentsContract.getTreeDocumentId(tree)
        val rootUri = DocumentsContract.buildDocumentUriUsingTree(tree, rootId)
        var rootName = "Folder"
        resolver.query(rootUri, arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) rootName = c.getString(0) ?: rootName
        }
        val root = TransferWire.cleanPath(rootName).ifEmpty { "Folder" }
        val out = ArrayList<SourceEntry>()

        fun walk(docId: String, rel: String, depth: Int) {
            if (depth > 32) return
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId)
            var count = 0
            resolver.query(
                children,
                arrayOf(
                    DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                    DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                    DocumentsContract.Document.COLUMN_MIME_TYPE,
                    DocumentsContract.Document.COLUMN_SIZE,
                ),
                null, null, null,
            )?.use { c ->
                while (c.moveToNext()) {
                    count++
                    val cid = c.getString(0)
                    val name = TransferWire.cleanPath(c.getString(1) ?: "_").ifEmpty { "_" }
                    val mime = c.getString(2)
                    val path = "$rel/$name"
                    if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                        walk(cid, path, depth + 1)
                    } else {
                        val size = if (c.isNull(3)) -1L else c.getLong(3)
                        val docUri = DocumentsContract.buildDocumentUriUsingTree(tree, cid)
                        val real = if (size >= 0) size else runCatching { resolver.openAssetFileDescriptor(docUri, "r")?.use { it.length } ?: -1L }.getOrDefault(-1L)
                        if (real >= 0) {
                            out += SourceEntry(path, real, false) { BufferedInputStream(resolver.openInputStream(docUri) ?: error("Cannot open $path"), 64 * 1024) }
                        }
                    }
                    if (out.size > TransferWire.MAX_FILES) throw TransferFailed("This folder has more than ${TransferWire.MAX_FILES} files. Send it in parts.", TransferFailed.Kind.Failed)
                }
            }
            if (count == 0) out += SourceEntry(rel, 0, true) { error("folder") }
        }
        walk(rootId, root, 0)
        out
    }

    /** Makes every path unique (case-insensitive), as the receiver requires. */
    private fun unique(list: List<SourceEntry>): List<SourceEntry> {
        val seen = HashSet<String>()
        return list.map { e ->
            var path = e.path
            var n = 1
            while (!seen.add(path.lowercase())) {
                val dot = path.lastIndexOf('.').takeIf { it > path.lastIndexOf('/') + 1 && !e.isDir } ?: path.length
                n++
                path = path.substring(0, dot) + " ($n)" + path.substring(dot)
            }
            if (path == e.path) e else SourceEntry(path, e.size, e.isDir, e.open)
        }
    }
}
