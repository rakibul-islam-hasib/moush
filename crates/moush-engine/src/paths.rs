use std::path::PathBuf;

/// Where Moush keeps its files.
#[derive(Clone, Debug)]
pub struct AppPaths {
    /// `%APPDATA%\Moush`: settings, identity, paired devices, layout.
    pub config_dir: PathBuf,
    /// `%LOCALAPPDATA%\Moush\logs`.
    pub log_dir: PathBuf,
}

impl AppPaths {
    pub fn from_env() -> Self {
        let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| roaming.clone());
        Self::with_dirs(roaming.join("Moush"), local.join("Moush").join("logs"))
    }

    pub fn with_dirs(config_dir: PathBuf, log_dir: PathBuf) -> Self {
        Self { config_dir, log_dir }
    }

    pub fn settings(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }
    pub fn identity(&self) -> PathBuf {
        self.config_dir.join("identity.key")
    }
    pub fn trusted(&self) -> PathBuf {
        self.config_dir.join("trusted.json")
    }
    pub fn layout(&self) -> PathBuf {
        self.config_dir.join("layout.json")
    }
    pub fn log_file(&self) -> PathBuf {
        self.log_dir.join("moush.log")
    }
}
