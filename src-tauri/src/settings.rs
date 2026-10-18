//! Persistent configuration: role plus user settings. Mirrors `Settings` in types.ts.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
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
                log::warn!("settings file unreadable ({e}); using defaults");
                Config::default()
            }),
            Err(_) => Config::default(),
        };
        if c.settings.device_name.trim().is_empty() {
            c.settings.device_name = default_name.to_owned();
        }
        c.settings = c.settings.clone().validated().unwrap_or_else(|_| Settings { device_name: default_name.to_owned(), ..Settings::default() });
        c
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
    }
}
