//! Optional features shared between paired computers (clipboard, file transfer, text
//! sharing): channel ids, capability bits and their user settings.
//!
//! Feature traffic travels in [`Message::Feature`](crate::protocol::Message::Feature)
//! envelopes. Each side announces which features it currently allows with a peer in
//! [`Message::Capabilities`](crate::protocol::Message::Capabilities); a feature is used
//! only when *both* sides allow it, and every receiver re-checks its own settings before
//! acting on incoming feature traffic (a peer's announcement is never trusted alone).

use serde::{Deserialize, Serialize};

/// Channel ids of [`Message::Feature`](crate::protocol::Message::Feature). 0 is reserved.
pub mod channel {
    pub const CLIPBOARD: u8 = 1;
    pub const FILES: u8 = 2;
}

/// Capability bits of [`Message::Capabilities`](crate::protocol::Message::Capabilities).
pub mod cap {
    /// The sender accepts clipboard updates from this peer and sends its own.
    pub const CLIPBOARD: u32 = 1 << 0;
    /// The sender accepts file transfers from this peer (subject to approval).
    pub const FILES: u32 = 1 << 1;
    /// The sender accepts manually shared text, links and snippets.
    pub const TEXT_SHARE: u32 = 1 << 2;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClipboardSettings {
    /// Master switch for clipboard synchronization.
    pub enabled: bool,
    /// Plain Unicode text.
    pub text: bool,
    /// Rich text (HTML and RTF) alongside the plain text.
    pub rich_text: bool,
    /// Images (bitmaps).
    pub images: bool,
    /// Larger clipboard contents are not synchronized.
    pub max_size_kb: u32,
    /// Skip content that its source marks as sensitive (password managers set
    /// `ExcludeClipboardContentFromMonitorProcessing` / `CanIncludeInClipboardHistory = 0`).
    pub skip_sensitive: bool,
    /// Keep a history of synchronized and shared clips (in memory only, never on disk).
    pub history: bool,
    pub history_limit: u32,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            text: true,
            rich_text: true,
            images: true,
            max_size_kb: 10 * 1024,
            skip_sensitive: true,
            history: true,
            history_limit: 25,
        }
    }
}

impl ClipboardSettings {
    pub fn validated(mut self) -> Self {
        self.max_size_kb = self.max_size_kb.clamp(16, 64 * 1024);
        self.history_limit = self.history_limit.clamp(1, 200);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyboardSettings {
    /// Forward the keyboard to the computer that has the shared cursor. When off, only
    /// the mouse moves between computers and typing always stays on the host.
    pub forward_keyboard: bool,
    /// System shortcuts (Alt+Tab, Win, Alt+F4, Ctrl+Esc, Win+<key>) go to the active
    /// computer. When off they always act on the host.
    pub forward_system_shortcuts: bool,
    /// Shortcuts that always stay on the host, e.g. "Ctrl+Shift+Esc" (at most 16).
    pub local_shortcuts: Vec<String>,
    /// Ctrl+Alt+Shift+Arrow moves the shared cursor to the neighbouring computer in that
    /// direction; Ctrl+Alt+Shift+Home brings it back to the host.
    pub switch_hotkeys: bool,
    /// When the cursor enters a client, bring the window under the cursor to the front
    /// there, so typing goes where the user is pointing.
    pub focus_on_enter: bool,
    /// Edge switching only happens while this modifier is held: "none", "ctrl", "shift" or "alt".
    pub edge_switch_modifier: String,
}

impl Default for KeyboardSettings {
    fn default() -> Self {
        Self {
            forward_keyboard: true,
            forward_system_shortcuts: true,
            local_shortcuts: Vec::new(),
            switch_hotkeys: true,
            focus_on_enter: false,
            edge_switch_modifier: "none".into(),
        }
    }
}

impl KeyboardSettings {
    pub fn validated(mut self) -> Self {
        self.local_shortcuts.retain(|s| !s.trim().is_empty() && s.len() <= 64);
        self.local_shortcuts.truncate(16);
        if !matches!(self.edge_switch_modifier.as_str(), "none" | "ctrl" | "shift" | "alt") {
            self.edge_switch_modifier = "none".into();
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferSettings {
    /// Allow sending and receiving files.
    pub enabled: bool,
    /// Where received files go; empty = `Downloads\Moush`.
    pub download_dir: String,
    /// Ask before accepting each incoming transfer. When off, transfers from paired
    /// devices that allow file transfer are accepted automatically.
    pub ask_before_receiving: bool,
    /// What to do when a received file name already exists: "rename", "overwrite" or "skip".
    pub overwrite: String,
    /// "balanced" keeps input latency lowest; "fast" allows more data in flight.
    pub speed: String,
    /// Open the destination folder in Explorer when a transfer completes (files are
    /// never opened or executed).
    pub open_folder_when_done: bool,
}

impl Default for TransferSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            download_dir: String::new(),
            ask_before_receiving: true,
            overwrite: "rename".into(),
            speed: "balanced".into(),
            open_folder_when_done: false,
        }
    }
}

impl TransferSettings {
    pub fn validated(mut self) -> Self {
        self.download_dir = self.download_dir.trim().chars().filter(|c| !c.is_control()).collect();
        if !matches!(self.overwrite.as_str(), "rename" | "overwrite" | "skip") {
            self.overwrite = "rename".into();
        }
        if !matches!(self.speed.as_str(), "balanced" | "fast") {
            self.speed = "balanced".into();
        }
        self
    }

    /// Bytes a sender may have in flight (unacknowledged) per transfer.
    pub fn window_bytes(&self) -> usize {
        if self.speed == "fast" { 1024 * 1024 } else { 128 * 1024 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    /// Master switch for Windows notifications.
    pub enabled: bool,
    /// A computer connected or disconnected.
    pub connections: bool,
    /// Incoming transfer requests and completed transfers.
    pub transfers: bool,
    /// Text or links shared from another computer.
    pub shared_text: bool,
    /// Every clipboard synchronization (noisy; off by default).
    pub clipboard: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self { enabled: true, connections: true, transfers: true, shared_text: true, clipboard: false }
    }
}

/// Per-device permissions, stored with the paired device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DevicePolicy {
    pub clipboard: bool,
    pub files: bool,
    pub text_share: bool,
}

impl Default for DevicePolicy {
    fn default() -> Self {
        Self { clipboard: true, files: true, text_share: true }
    }
}

/// Capability bits this side allows with a peer, from global settings and the peer's policy.
pub fn local_caps(clipboard: &ClipboardSettings, transfer: &TransferSettings, policy: &DevicePolicy) -> u32 {
    let mut caps = 0;
    if clipboard.enabled && policy.clipboard {
        caps |= cap::CLIPBOARD;
    }
    if transfer.enabled && policy.files {
        caps |= cap::FILES;
    }
    if policy.text_share {
        caps |= cap::TEXT_SHARE;
    }
    caps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_clamps_and_normalizes() {
        let c = ClipboardSettings { max_size_kb: 0, history_limit: 10_000, ..Default::default() }.validated();
        assert_eq!((c.max_size_kb, c.history_limit), (16, 200));
        let k = KeyboardSettings { edge_switch_modifier: "hyper".into(), local_shortcuts: vec![" ".into(); 3], ..Default::default() }
            .validated();
        assert_eq!(k.edge_switch_modifier, "none");
        assert!(k.local_shortcuts.is_empty());
        let t = TransferSettings { overwrite: "x".into(), speed: "ludicrous".into(), ..Default::default() }.validated();
        assert_eq!((t.overwrite.as_str(), t.speed.as_str()), ("rename", "balanced"));
    }

    #[test]
    fn caps_follow_settings_and_policy() {
        let c = ClipboardSettings::default();
        let t = TransferSettings::default();
        let all = local_caps(&c, &t, &DevicePolicy::default());
        assert_eq!(all, cap::CLIPBOARD | cap::FILES | cap::TEXT_SHARE);
        let off = local_caps(&ClipboardSettings { enabled: false, ..c }, &t, &DevicePolicy { files: false, ..Default::default() });
        assert_eq!(off, cap::TEXT_SHARE);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let k: KeyboardSettings = serde_json::from_str(r#"{"focusOnEnter":true}"#).unwrap();
        assert!(k.focus_on_enter && k.forward_keyboard);
    }
}
