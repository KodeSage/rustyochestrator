use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectConfig {
    pub dashboard_url: String,
    pub token: String,
    pub user_login: String,
}

fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rustyochestrator")
        .join("connect.json")
}

pub fn load() -> Option<ConnectConfig> {
    let content = std::fs::read_to_string(config_path()).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save(config: &ConnectConfig) -> std::io::Result<()> {
    let path = config_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::to_string_pretty(config).unwrap())
}

pub fn delete() -> std::io::Result<()> {
    let path = config_path();
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
