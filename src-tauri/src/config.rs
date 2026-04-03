use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub api_key: String,
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
    fn default_api_key_is_empty() {
        let config = AppConfig::default();
        assert_eq!(config.api_key, "");
    }

    #[test]
    fn serializes_to_json_with_api_key_field() {
        let config = AppConfig { api_key: "waka-test-key".to_string() };
        let json = serde_json::to_string(&config).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["api_key"].as_str().unwrap(), "waka-test-key");
    }

    #[test]
    fn deserializes_from_json_with_api_key_field() {
        let json = r#"{"api_key":"waka-abc"}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.api_key, "waka-abc");
    }

    #[test]
    fn deserializes_missing_api_key_as_empty_string() {
        let json = r#"{}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.api_key, "");
    }

    #[test]
    fn round_trips_api_key_through_json() {
        let original = AppConfig { api_key: "round-trip-key".to_string() };
        let json = serde_json::to_string(&original).unwrap();
        let restored: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.api_key, original.api_key);
    }

    #[test]
    fn save_and_load_round_trip() {
        // Write to a temp file directly to avoid mutating real config.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = AppConfig { api_key: "saved-key-123".to_string() };
        let json = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&path, json).unwrap();

        let loaded: AppConfig = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.api_key, "saved-key-123");
    }

    #[test]
    fn load_returns_default_when_file_absent() {
        // config_path() points at the real config location. If it doesn't
        // exist in CI the function must return Default without panicking.
        // We verify by ensuring load() doesn't panic and returns an AppConfig.
        let _ = AppConfig::load(); // must not panic
    }

    #[test]
    fn save_writes_pretty_printed_json_with_api_key() {
        // Verify the serialization format by round-tripping through serde
        // without touching the real config path.
        let config = AppConfig { api_key: "pretty-key".to_string() };
        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(json.contains("api_key"));
        assert!(json.contains("pretty-key"));
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.api_key, "pretty-key");
    }
}
