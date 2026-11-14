package app.moush.remote.data

import android.content.Context
import android.content.SharedPreferences
import app.moush.remote.net.Identity
import app.moush.remote.net.Noise

data class PairedHost(val id: String, val name: String, val publicKey: ByteArray, val lastAddress: String?, val lastPort: Int)

enum class ThemeMode { System, Light, Dark }

data class Settings(
    val sensitivity: Float = 1.0f,
    val scrollSpeed: Float = 1.0f,
    val naturalScroll: Boolean = true,
    val tapToClick: Boolean = true,
    val haptics: Boolean = true,
    val keepAwake: Boolean = true,
    val autoReconnect: Boolean = true,
    val theme: ThemeMode = ThemeMode.System,
    val deviceName: String = android.os.Build.MODEL ?: "Phone",
)

/** Everything persisted on the phone: identity, paired computers and preferences. */
class Store(context: Context) {
    private val p: SharedPreferences = context.getSharedPreferences("moush", Context.MODE_PRIVATE)

    val identity: Identity by lazy {
        p.getString("identity", null)?.let { Noise.unhex(it) }?.let { Identity.fromBytes(it) }
            ?: Identity.generate().also { p.edit().putString("identity", Noise.hex(it.toBytes())).apply() }
    }

    fun pairedHosts(): List<PairedHost> =
        (p.getString("hosts", "") ?: "").lines().mapNotNull { line ->
            val f = line.split('\t')
            if (f.size < 5) return@mapNotNull null
            val key = Noise.unhex(f[2]) ?: return@mapNotNull null
            if (key.size != 32 || Noise.deviceId(key) != f[0]) return@mapNotNull null
            PairedHost(f[0], f[1], key, f[3].ifEmpty { null }, f[4].toIntOrNull() ?: 24800)
        }

    fun isTrusted(publicKey: ByteArray): Boolean = pairedHosts().any { it.publicKey.contentEquals(publicKey) }

    fun saveHost(h: PairedHost) {
        val all = pairedHosts().filter { it.id != h.id } + h
        write(all)
    }

    fun forget(id: String) = write(pairedHosts().filter { it.id != id })

    private fun write(list: List<PairedHost>) {
        val text = list.joinToString("\n") {
            listOf(it.id, it.name.replace('\t', ' ').replace('\n', ' '), Noise.hex(it.publicKey), it.lastAddress ?: "", it.lastPort).joinToString("\t")
        }
        p.edit().putString("hosts", text).apply()
    }

    var lastHostId: String?
        get() = p.getString("lastHost", null)
        set(v) { p.edit().putString("lastHost", v).apply() }

    fun settings(): Settings {
        val d = Settings()
        return Settings(
            sensitivity = p.getFloat("sens", d.sensitivity),
            scrollSpeed = p.getFloat("scroll", d.scrollSpeed),
            naturalScroll = p.getBoolean("natural", d.naturalScroll),
            tapToClick = p.getBoolean("tap", d.tapToClick),
            haptics = p.getBoolean("haptics", d.haptics),
            keepAwake = p.getBoolean("awake", d.keepAwake),
            autoReconnect = p.getBoolean("auto", d.autoReconnect),
            theme = ThemeMode.entries.getOrElse(p.getInt("theme", 0)) { ThemeMode.System },
            deviceName = p.getString("name", null) ?: d.deviceName,
        )
    }

    fun save(s: Settings) {
        p.edit()
            .putFloat("sens", s.sensitivity).putFloat("scroll", s.scrollSpeed)
            .putBoolean("natural", s.naturalScroll).putBoolean("tap", s.tapToClick)
            .putBoolean("haptics", s.haptics).putBoolean("awake", s.keepAwake)
            .putBoolean("auto", s.autoReconnect).putInt("theme", s.theme.ordinal)
            .putString("name", s.deviceName)
            .apply()
    }
}
