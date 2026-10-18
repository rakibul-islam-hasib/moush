//! Persistent configuration: role plus user settings. Mirrors `Settings` in types.ts.
//!
//! The file carries a schema `version`. Missing fields take their defaults, unknown
//! fields are ignored, older versions are migrated on load, and an unreadable file is
//! kept aside as `settings.corrupt.json` instead of being silently overwritten.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use moush_engine::core::features::{ClipboardSettings, KeyboardSettings, NotificationSettings, TransferSettings};
use moush_engine::core::protocol::DEFAULT_PORT;
use moush_engine::core::router::RouterSettings;
use moush_engine::core::util::write_atomic;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Host,
    Client,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Host => "host",
            Role::Client => "client",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub device_name: String,
    pub theme: String,
    pub launch_at_startup: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
    pub auto_start: bool,
    pub port: u16,
    pub discovery_enabled: bool,
    pub switch_delay_ms: u32,
    pub corner_dead_zone_px: i32,
    pub client_pointer_speed: f64,
    pub scroll_lock_toggle: bool,
    pub last_host_address: Option<String>,
    pub clipboard: ClipboardSettings,
    pub keyboard: KeyboardSettings,
    pub transfer: TransferSettings,
    pub notifications: NotificationSettings,
    pub appearance: AppearanceSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceSettings {
    /// Mica backdrop (Windows 11). Off = opaque window.
    pub transparency: bool,
    /// "system" follows the Windows animation setting; "reduce" or "full" override it.
    pub motion: String,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self { transparency: true, motion: "system".into() }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: String::new(),
            theme: "system".into(),
            launch_at_startup: false,
            start_minimized: false,
            close_to_tray: true,
            auto_start: true,
            port: DEFAULT_PORT,
            discovery_enabled: true,
            switch_delay_ms: 0,
            corner_dead_zone_px: 0,
            client_pointer_speed: 1.0,
            scroll_lock_toggle: true,
            last_host_address: None,
            clipboard: ClipboardSettings::default(),
            keyboard: KeyboardSettings::default(),
            transfer: TransferSettings::default(),
            notifications: NotificationSettings::default(),
            appearance: AppearanceSettings::default(),
        }
    }
}

impl Settings {
    /// Rejects invalid values and clamps ranged ones.
    pub fn validated(mut self) -> Result<Self, String> {
        self.device_name = self.device_name.trim().chars().filter(|c| !c.is_control()).take(64).collect();
        if self.device_name.is_empty() {
            return Err("Device name can't be empty".into());
        }
        if self.port < 1024 {
            return Err("Port must be between 1024 and 65535".into());
        }
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            self.theme = "system".into();
        }
        self.switch_delay_ms = self.switch_delay_ms.min(1000);
        self.corner_dead_zone_px = self.corner_dead_zone_px.clamp(0, 200);
        if !self.client_pointer_speed.is_finite() {
            self.client_pointer_speed = 1.0;
        }
        self.client_pointer_speed = self.client_pointer_speed.clamp(0.25, 4.0);
        self.clipboard = self.clipboard.validated();
        self.keyboard = self.keyboard.validated();
        self.transfer = self.transfer.validated();
        if !matches!(self.appearance.motion.as_str(), "system" | "reduce" | "full") {
            self.appearance.motion = "system".into();
        }
        Ok(self)
    }

    pub fn router(&self) -> RouterSettings {
        RouterSettings {
            switch_delay_ms: self.switch_delay_ms,
            corner_dead_zone: self.corner_dead_zone_px,
            pointer_speed: self.client_pointer_speed,
            scroll_lock_toggle: self.scroll_lock_toggle,
        }
    }
}

/// Current schema version of the settings file.
pub const CONFIG_VERSION: u32 = 2;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Schema version; 0/1 = written by Moush 0.1 (before feature settings existed).
    pub version: u32,
    pub role: Option<Role>,
    pub settings: Settings,
    /// Device id of the host last connected to (client role), for reconnecting by
    /// identity when its address changes.
    pub last_host_id: Option<String>,
}

impl Config {
    pub fn load(path: &Path, default_name: &str) -> Config {
        let mut c = match fs::read_to_string(path) {
            Ok(s) => serde_json::from_str::<Config>(&s).unwrap_or_else(|e| {
                log::warn!("settings file unreadable ({e}); keeping it as settings.corrupt.json and using defaults");
                let _ = fs::copy(path, path.with_file_name("settings.corrupt.json"));
                Config::default()
            }),
            Err(_) => Config { version: CONFIG_VERSION, ..Config::default() },
        };
        c.migrate();
        if c.settings.device_name.trim().is_empty() {
            c.settings.device_name = default_name.to_owned();
        }
        c.settings = c.settings.clone().validated().unwrap_or_else(|_| Settings { device_name: default_name.to_owned(), ..Settings::default() });
        c
    }

    /// Upgrades an older schema in place.
    fn migrate(&mut self) {
        if self.version < 2 {
            // v1 -> v2: feature settings were added; their defaults already apply.
            if self.version != 0 {
                log::info!("migrating settings from version {} to {CONFIG_VERSION}", self.version);
            }
        }
        self.version = CONFIG_VERSION;
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        write_atomic(path, json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        let s = Settings { device_name: "  Desk\n PC ".into(), switch_delay_ms: 5000, client_pointer_speed: 9.0, ..Default::default() };
        let v = s.validated().unwrap();
        assert_eq!(v.device_name, "Desk PC");
        assert_eq!(v.switch_delay_ms, 1000);
        assert_eq!(v.client_pointer_speed, 4.0);
        assert!(Settings { device_name: "x".into(), port: 80, ..Default::default() }.validated().is_err());
        assert!(Settings::default().validated().is_err(), "empty name");
    }

    #[test]
    fn unknown_and_missing_fields_use_defaults() {
        let c: Config = serde_json::from_str(r#"{"role":"client","settings":{"port":25000,"future":1}}"#).unwrap();
        assert_eq!(c.role, Some(Role::Client));
        assert_eq!(c.settings.port, 25000);
        assert!(c.settings.close_to_tray);
        assert!(c.settings.clipboard.enabled && c.settings.transfer.ask_before_receiving);
    }

    #[test]
    fn v1_file_migrates_and_corrupt_file_is_kept() {
        let dir = std::env::temp_dir().join(format!("moush-settings-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"role":"host","settings":{"deviceName":"Desk","switchDelayMs":200}}"#).unwrap();
        let c = Config::load(&path, "Fallback");
        assert_eq!(c.version, CONFIG_VERSION);
        assert_eq!((c.settings.device_name.as_str(), c.settings.switch_delay_ms), ("Desk", 200));
        assert_eq!(c.settings.keyboard, KeyboardSettings::default());

        fs::write(&path, "{ not json").unwrap();
        let c = Config::load(&path, "Fallback");
        assert_eq!(c.settings.device_name, "Fallback");
        assert!(dir.join("settings.corrupt.json").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
