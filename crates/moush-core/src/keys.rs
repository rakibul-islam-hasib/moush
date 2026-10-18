//! Windows virtual-key codes used by the router and the injector, modifier bookkeeping
//! and keyboard shortcut parsing.
//! Values are fixed by the Win32 API; they are duplicated here so the core stays OS-free.

use std::fmt;

pub const VK_CANCEL: u16 = 0x03;
pub const VK_TAB: u16 = 0x09;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_MENU: u16 = 0x12;
pub const VK_PAUSE: u16 = 0x13;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;
pub const VK_PRIOR: u16 = 0x21;
pub const VK_NEXT: u16 = 0x22;
pub const VK_END: u16 = 0x23;
pub const VK_HOME: u16 = 0x24;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;
pub const VK_SNAPSHOT: u16 = 0x2C;
pub const VK_INSERT: u16 = 0x2D;
pub const VK_DELETE: u16 = 0x2E;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_RWIN: u16 = 0x5C;
pub const VK_SLEEP: u16 = 0x5F;
pub const VK_F1: u16 = 0x70;
pub const VK_F4: u16 = 0x73;
pub const VK_F24: u16 = 0x87;
pub const VK_NUMLOCK: u16 = 0x90;
pub const VK_SCROLL: u16 = 0x91;
pub const VK_LSHIFT: u16 = 0xA0;
pub const VK_RSHIFT: u16 = 0xA1;
pub const VK_LCONTROL: u16 = 0xA2;
pub const VK_RCONTROL: u16 = 0xA3;
pub const VK_LMENU: u16 = 0xA4;
pub const VK_RMENU: u16 = 0xA5;
/// VK_BROWSER_BACK ..= VK_LAUNCH_APP2: browser, volume, media and launch keys.
pub const VK_BROWSER_FIRST: u16 = 0xA6;
pub const VK_LAUNCH_LAST: u16 = 0xB7;
/// An unassigned virtual-key code, pressed and released between a Win (or Alt) press and
/// its release so that the shell does not treat the release as a lone tap: a lone Win
/// tap opens the Start menu and a lone Alt tap activates the menu bar. 0xE8 is
/// "unassigned" in the Win32 key table and has no effect in applications (it is also
/// AutoHotkey's default "menu mask key"); Ctrl, the classic choice, has side effects in
/// some programs (e.g. Ctrl taps that highlight the cursor).
pub const VK_MASK: u16 = 0xE8;

/// Modifier bits (a set of modifier families, left and right keys alike).
pub mod mods {
    pub const CTRL: u8 = 1;
    pub const ALT: u8 = 2;
    pub const SHIFT: u8 = 4;
    pub const WIN: u8 = 8;
}

/// Every modifier key code. Low-level hooks report the left/right codes; the generic
/// ones are listed for completeness.
pub const MODIFIER_VKS: [u16; 11] = [
    VK_LSHIFT,
    VK_RSHIFT,
    VK_LCONTROL,
    VK_RCONTROL,
    VK_LMENU,
    VK_RMENU,
    VK_LWIN,
    VK_RWIN,
    VK_SHIFT,
    VK_CONTROL,
    VK_MENU,
];

pub fn is_modifier(vk: u16) -> bool {
    modifier_bit(vk) != 0
}

/// The modifier family of `vk`, or 0 for other keys.
pub fn modifier_bit(vk: u16) -> u8 {
    match vk {
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => mods::SHIFT,
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => mods::CTRL,
        VK_MENU | VK_LMENU | VK_RMENU => mods::ALT,
        VK_LWIN | VK_RWIN => mods::WIN,
        _ => 0,
    }
}

/// Keys whose lone press-and-release triggers something in Windows (Start menu, menu bar).
pub fn is_menu_modifier(vk: u16) -> bool {
    modifier_bit(vk) & (mods::ALT | mods::WIN) != 0
}

/// Keys that must be injected by virtual-key code rather than scan code: keys without a
/// scan code, keys whose real scan code needs an E1 prefix (Pause), and multimedia keys.
pub fn inject_by_vk(vk: u16, scan: u16) -> bool {
    scan == 0
        || matches!(vk, VK_PAUSE | VK_CANCEL | VK_SNAPSHOT | VK_SLEEP)
        || (VK_BROWSER_FIRST..=VK_LAUNCH_LAST).contains(&vk)
}

/// Shortcuts the Windows shell acts on: Alt+Tab, Alt+Shift+Tab, Ctrl+Alt+Tab, Alt+Esc,
/// Alt+F4, Ctrl+Esc, Ctrl+Shift+Esc and Win+<any key>. A lone Win tap (Start menu) is
/// recognised by the router separately, at the Win release.
pub fn is_system_shortcut(held: u8, vk: u16) -> bool {
    if held & mods::WIN != 0 {
        return true;
    }
    if held & mods::ALT != 0 && (matches!(vk, VK_TAB | VK_F4) || (vk == VK_ESCAPE && held & mods::CTRL == 0)) {
        return true;
    }
    held & mods::CTRL != 0 && held & mods::ALT == 0 && vk == VK_ESCAPE
}

/// A key combination: modifier families plus one non-modifier key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub mods: u8,
    pub vk: u8,
}

impl Shortcut {
    /// True when exactly these modifiers are held and `vk` is the key.
    pub fn matches(&self, held: u8, vk: u16) -> bool {
        self.mods == held && self.vk as u16 == vk
    }
}

const KEY_NAMES: [(&str, u16); 18] = [
    ("Tab", VK_TAB),
    ("Esc", VK_ESCAPE),
    ("Escape", VK_ESCAPE),
    ("Space", VK_SPACE),
    ("Enter", VK_RETURN),
    ("Return", VK_RETURN),
    ("Home", VK_HOME),
    ("End", VK_END),
    ("PgUp", VK_PRIOR),
    ("PageUp", VK_PRIOR),
    ("PgDn", VK_NEXT),
    ("PageDown", VK_NEXT),
    ("Insert", VK_INSERT),
    ("Delete", VK_DELETE),
    ("Left", VK_LEFT),
    ("Right", VK_RIGHT),
    ("Up", VK_UP),
    ("Down", VK_DOWN),
];

fn key_from_name(s: &str) -> Option<u16> {
    let b = s.as_bytes();
    if b.len() == 1 {
        let c = b[0].to_ascii_uppercase();
        return (c.is_ascii_uppercase() || c.is_ascii_digit()).then_some(c as u16);
    }
    if let Some(n) =
        s.strip_prefix(['F', 'f']).filter(|n| n.bytes().all(|c| c.is_ascii_digit())).and_then(|n| n.parse::<u16>().ok())
        && (1..=24).contains(&n)
    {
        return Some(VK_F1 + n - 1);
    }
    let alias = match s.to_ascii_lowercase().as_str() {
        "ins" => Some(VK_INSERT),
        "del" => Some(VK_DELETE),
        "arrowleft" => Some(VK_LEFT),
        "arrowright" => Some(VK_RIGHT),
        "arrowup" => Some(VK_UP),
        "arrowdown" => Some(VK_DOWN),
        _ => None,
    };
    alias.or_else(|| KEY_NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(s)).map(|(_, vk)| *vk))
}

fn key_name(vk: u16) -> Option<String> {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => Some((vk as u8 as char).to_string()),
        VK_F1..=VK_F24 => Some(format!("F{}", vk - VK_F1 + 1)),
        _ => KEY_NAMES.iter().find(|(_, v)| *v == vk).map(|(n, _)| (*n).to_owned()),
    }
}

/// Parses "Ctrl+Alt+Shift+Win+<Key>" (case-insensitive, any modifier order, spaces
/// allowed around '+'). Keys: letters, digits, F1-F24, arrows (Left/Right/Up/Down), Tab,
/// Esc, Space, Enter, Home, End, PgUp, PgDn, Insert, Delete. At least one modifier is
/// required so that a plain key can never be taken away from the other computers.
pub fn parse_shortcut(text: &str) -> Result<Shortcut, String> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err(format!("\"{text}\" is not a shortcut (expected e.g. Ctrl+Shift+Esc)"));
    }
    let (key, modifiers) = parts.split_last().ok_or_else(|| "empty shortcut".to_owned())?;
    let mut m = 0u8;
    for p in modifiers {
        let bit = match p.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods::CTRL,
            "alt" => mods::ALT,
            "shift" => mods::SHIFT,
            "win" | "windows" => mods::WIN,
            _ => return Err(format!("unknown modifier \"{p}\" in \"{text}\" (use Ctrl, Alt, Shift or Win)")),
        };
        if m & bit != 0 {
            return Err(format!("\"{p}\" appears twice in \"{text}\""));
        }
        m |= bit;
    }
    let vk = key_from_name(key).ok_or_else(|| format!("unknown key \"{key}\" in \"{text}\""))?;
    if m == 0 {
        return Err(format!("\"{text}\" needs at least one modifier (Ctrl, Alt, Shift or Win)"));
    }
    if m == mods::CTRL | mods::ALT && vk == VK_DELETE {
        return Err("Ctrl+Alt+Delete is always handled by Windows on the computer it is pressed on".into());
    }
    Ok(Shortcut { mods: m, vk: vk as u8 })
}

impl fmt::Display for Shortcut {
    /// Canonical form, e.g. "Ctrl+Shift+Esc".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (bit, name) in [(mods::CTRL, "Ctrl"), (mods::ALT, "Alt"), (mods::SHIFT, "Shift"), (mods::WIN, "Win")] {
            if self.mods & bit != 0 {
                write!(f, "{name}+")?;
            }
        }
        match key_name(self.vk as u16) {
            Some(n) => f.write_str(&n),
            None => write!(f, "0x{:02X}", self.vk),
        }
    }
}

/// Up to [`ShortcutSet::CAPACITY`] shortcuts in a fixed array, so that router settings
/// stay `Copy` and lookups in the hook callback never allocate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShortcutSet {
    items: [Shortcut; ShortcutSet::CAPACITY],
    len: u8,
}

impl ShortcutSet {
    pub const CAPACITY: usize = 16;

    /// Parses every entry; returns the set and the error of each entry that was ignored.
    pub fn parse<S: AsRef<str>>(entries: &[S]) -> (Self, Vec<String>) {
        let mut set = Self::default();
        let mut errors = Vec::new();
        for e in entries {
            match parse_shortcut(e.as_ref()) {
                Ok(s) if set.as_slice().contains(&s) => {}
                Ok(s) if set.len as usize == Self::CAPACITY => {
                    errors.push(format!("\"{s}\" ignored: at most {} shortcuts", Self::CAPACITY));
                }
                Ok(s) => {
                    set.items[set.len as usize] = s;
                    set.len += 1;
                }
                Err(err) => errors.push(err),
            }
        }
        (set, errors)
    }

    pub fn as_slice(&self) -> &[Shortcut] {
        &self.items[..self.len as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn matches(&self, held: u8, vk: u16) -> bool {
        self.as_slice().iter().any(|s| s.matches(held, vk))
    }

    /// Union of the modifiers used by the shortcuts.
    pub fn modifiers(&self) -> u8 {
        self.as_slice().iter().fold(0, |m, s| m | s.mods)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shortcuts_case_insensitively() {
        let s = parse_shortcut("ctrl+shift+esc").unwrap();
        assert_eq!(s, Shortcut { mods: mods::CTRL | mods::SHIFT, vk: VK_ESCAPE as u8 });
        assert_eq!(s.to_string(), "Ctrl+Shift+Esc");
        assert_eq!(parse_shortcut(" Win + l ").unwrap(), Shortcut { mods: mods::WIN, vk: b'L' });
        assert_eq!(parse_shortcut("Shift+Alt+Control+Windows+F12").unwrap().to_string(), "Ctrl+Alt+Shift+Win+F12");
        assert_eq!(parse_shortcut("Alt+7").unwrap().vk, b'7');
        assert_eq!(parse_shortcut("Ctrl+F24").unwrap().vk as u16, VK_F24);
        assert_eq!(parse_shortcut("Ctrl+F1").unwrap().vk as u16, VK_F1);
    }

    #[test]
    fn parses_every_named_key() {
        let cases = [
            ("Tab", VK_TAB),
            ("Escape", VK_ESCAPE),
            ("space", VK_SPACE),
            ("Return", VK_RETURN),
            ("Enter", VK_RETURN),
            ("Home", VK_HOME),
            ("END", VK_END),
            ("PgUp", VK_PRIOR),
            ("PageDown", VK_NEXT),
            ("Ins", VK_INSERT),
            ("Del", VK_DELETE),
            ("Left", VK_LEFT),
            ("ArrowRight", VK_RIGHT),
            ("up", VK_UP),
            ("Down", VK_DOWN),
        ];
        for (name, vk) in cases {
            let s = parse_shortcut(&format!("Alt+{name}")).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(s.vk as u16, vk, "{name}");
            // The canonical form parses back to the same shortcut.
            assert_eq!(parse_shortcut(&s.to_string()).unwrap(), s);
        }
    }

    #[test]
    fn rejects_invalid_shortcuts() {
        for bad in [
            "",
            "+",
            "Ctrl+",
            "Ctrl++A",
            "A",
            "Esc",
            "Hyper+A",
            "Ctrl+Ctrl+A",
            "Ctrl+F25",
            "Ctrl+F0",
            "Ctrl+Shift",
            "Ctrl+AB",
            "Ctrl+Alt+Delete",
            "Ctrl+é",
        ] {
            assert!(parse_shortcut(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn shortcut_set_is_bounded_and_deduplicated() {
        let entries: Vec<String> =
            (0..20).map(|i| format!("Ctrl+F{}", i + 1)).chain(["ctrl+f1".into(), "bogus".into()]).collect();
        let (set, errors) = ShortcutSet::parse(&entries);
        assert_eq!(set.as_slice().len(), ShortcutSet::CAPACITY);
        assert_eq!(errors.len(), 5, "{errors:?}"); // F17..F20 over capacity + "bogus"
        assert!(set.matches(mods::CTRL, VK_F1));
        assert!(!set.matches(mods::CTRL | mods::SHIFT, VK_F1), "modifiers must match exactly");
        assert_eq!(set.modifiers(), mods::CTRL);
    }

    #[test]
    fn system_shortcuts() {
        assert!(is_system_shortcut(mods::ALT, VK_TAB));
        assert!(is_system_shortcut(mods::ALT | mods::SHIFT, VK_TAB));
        assert!(is_system_shortcut(mods::ALT, VK_ESCAPE));
        assert!(is_system_shortcut(mods::ALT, VK_F4));
        assert!(is_system_shortcut(mods::CTRL, VK_ESCAPE));
        assert!(is_system_shortcut(mods::CTRL | mods::SHIFT, VK_ESCAPE));
        assert!(is_system_shortcut(mods::WIN, b'E' as u16));
        assert!(is_system_shortcut(mods::WIN | mods::SHIFT, b'S' as u16));
        assert!(!is_system_shortcut(mods::CTRL | mods::ALT, VK_ESCAPE), "Moush's own return-home hotkey");
        assert!(!is_system_shortcut(mods::CTRL, b'C' as u16));
        assert!(!is_system_shortcut(mods::ALT, b'F' as u16));
        assert!(!is_system_shortcut(0, VK_TAB));
    }
}
