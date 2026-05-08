use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub access_token: String,
}

fn config_path() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.wakatoken.client");
    fs::create_dir_all(&dir).ok();
    dir.join("config.json")
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_access_token_is_empty() {
        let config = AppConfig::default();
        assert_eq!(config.access_token, "");
    }

    #[test]
    fn serializes_access_token() {
        let config = AppConfig {
            access_token: "access-token".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["access_token"].as_str().unwrap(), "access-token");
    }

    #[test]
    fn deserializes_missing_access_token_as_empty_string() {
        let config: AppConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(config.access_token, "");
    }

    #[test]
    fn round_trips_access_token_through_json() {
        let original = AppConfig {
            access_token: "round-trip-token".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.access_token, original.access_token);
    }

    #[test]
    fn load_returns_default_when_file_absent() {
        let _ = AppConfig::load();
    }
}
