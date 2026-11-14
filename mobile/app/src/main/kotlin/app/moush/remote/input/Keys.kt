package app.moush.remote.input

import app.moush.remote.net.Session

/** A physical key: Windows virtual-key code, set-1 scan code, extended flag. */
data class Key(val vk: Int, val scan: Int, val extended: Boolean = false)

object Keys {
    val Esc = Key(0x1B, 0x01)
    val Tab = Key(0x09, 0x0F)
    val Enter = Key(0x0D, 0x1C)
    val Backspace = Key(0x08, 0x0E)
    val Space = Key(0x20, 0x39)
    val Delete = Key(0x2E, 0x53, true)
    val Home = Key(0x24, 0x47, true)
    val End = Key(0x23, 0x4F, true)
    val PageUp = Key(0x21, 0x49, true)
    val PageDown = Key(0x22, 0x51, true)
    val Left = Key(0x25, 0x4B, true)
    val Up = Key(0x26, 0x48, true)
    val Right = Key(0x27, 0x4D, true)
    val Down = Key(0x28, 0x50, true)
    val Shift = Key(0x10, 0x2A)
    val Ctrl = Key(0x11, 0x1D)
    val Alt = Key(0x12, 0x38)
    val Win = Key(0x5B, 0x5B, true)
    val F5 = Key(0x74, 0x3F)
    val F4 = Key(0x73, 0x3E)

    fun f(n: Int) = Key(0x6F + n, if (n <= 10) 0x3A + n else 0x57 + (n - 11))
    fun letter(c: Char) = Key(c.uppercaseChar().code, LETTER_SCAN.getValue(c.lowercaseChar()))

    // Media keys are injected by virtual-key code on the PC.
    val PlayPause = Key(0xB3, 0)
    val NextTrack = Key(0xB0, 0)
    val PrevTrack = Key(0xB1, 0)
    val VolUp = Key(0xAF, 0)
    val VolDown = Key(0xAE, 0)
    val Mute = Key(0xAD, 0)

    private val LETTER_SCAN: Map<Char, Int> = buildMap {
        "qwertyuiop".forEachIndexed { i, c -> put(c, 0x10 + i) }
        "asdfghjkl".forEachIndexed { i, c -> put(c, 0x1E + i) }
        "zxcvbnm".forEachIndexed { i, c -> put(c, 0x2C + i) }
    }

    /** US-layout characters: (key, needs shift). The PC maps them through its own layout. */
    fun forChar(c: Char): Pair<Key, Boolean>? {
        if (c.isLetter() && c.code < 128) return letter(c) to c.isUpperCase()
        fun digit(i: Int) = if (i == 9) Key(0x30, 0x0B) else Key(0x31 + i, 0x02 + i)
        "1234567890".indexOf(c).takeIf { it >= 0 }?.let { return digit(it) to false }
        "!@#$%^&*()".indexOf(c).takeIf { it >= 0 }?.let { return digit(it) to true }
        return when (c) {
            ' ' -> Space to false
            '\n' -> Enter to false
            '\t' -> Tab to false
            '-' -> Key(0xBD, 0x0C) to false
            '_' -> Key(0xBD, 0x0C) to true
            '=' -> Key(0xBB, 0x0D) to false
            '+' -> Key(0xBB, 0x0D) to true
            '[' -> Key(0xDB, 0x1A) to false
            '{' -> Key(0xDB, 0x1A) to true
            ']' -> Key(0xDD, 0x1B) to false
            '}' -> Key(0xDD, 0x1B) to true
            '\\' -> Key(0xDC, 0x2B) to false
            '|' -> Key(0xDC, 0x2B) to true
            ';' -> Key(0xBA, 0x27) to false
            ':' -> Key(0xBA, 0x27) to true
            '\'' -> Key(0xDE, 0x28) to false
            '"' -> Key(0xDE, 0x28) to true
            '`' -> Key(0xC0, 0x29) to false
            '~' -> Key(0xC0, 0x29) to true
            ',' -> Key(0xBC, 0x33) to false
            '<' -> Key(0xBC, 0x33) to true
            '.' -> Key(0xBE, 0x34) to false
            '>' -> Key(0xBE, 0x34) to true
            '/' -> Key(0xBF, 0x35) to false
            '?' -> Key(0xBF, 0x35) to true
            else -> null
        }
    }
}

/** Key helpers on top of a session. */
fun Session.press(k: Key) {
    key(k.vk, k.scan, k.extended, true)
    key(k.vk, k.scan, k.extended, false)
}

/** Presses [keys] together (all down in order, released in reverse): shortcuts. */
fun Session.chord(vararg keys: Key) {
    keys.forEach { key(it.vk, it.scan, it.extended, true) }
    keys.reversed().forEach { key(it.vk, it.scan, it.extended, false) }
}

fun Session.typeText(text: String) {
    for (c in text) {
        val (k, shift) = Keys.forChar(c) ?: continue
        if (shift) key(Keys.Shift.vk, Keys.Shift.scan, false, true)
        press(k)
        if (shift) key(Keys.Shift.vk, Keys.Shift.scan, false, false)
    }
}
