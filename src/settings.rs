use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub emulator_binary: PathBuf,
    pub default_port: u16,
    pub poll_interval_ms: u64,
    pub auth_enabled: bool,
    pub telemetry_enabled: bool,
    pub idle_timeout_secs: u64,
    pub proxy_enabled: bool,
    pub proxy_port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            emulator_binary: PathBuf::from("aws-emulator"),
            default_port: 9297,
            poll_interval_ms: 5000,
            auth_enabled: true,
            telemetry_enabled: false,
            idle_timeout_secs: 0,
            proxy_enabled: false,
            proxy_port: 9298,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        if let Some(config_dir) = dirs::config_dir() {
            let path = config_dir
                .join("aws-emulator-dashboard")
                .join("config.toml");
            if let Ok(contents) = std::fs::read_to_string(&path)
                && let Ok(settings) = toml::from_str::<Settings>(&contents)
            {
                return settings;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        if let Some(config_dir) = dirs::config_dir() {
            let dir = config_dir.join("aws-emulator-dashboard");
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("config.toml");
            std::fs::write(&path, toml::to_string_pretty(self)?)?;
        }
        Ok(())
    }
}
