use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_enabled_runtimes")]
    pub enabled_runtimes: Vec<String>,
    #[serde(default)]
    pub onboarding_completed: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            enabled_runtimes: default_enabled_runtimes(),
            onboarding_completed: false,
        }
    }
}

pub fn default_enabled_runtimes() -> Vec<String> {
    vec![
        "claude-code".to_string(),
        "codex-cli".to_string(),
        "copilot-agent".to_string(),
        "gemini-cli".to_string(),
    ]
}

impl AppConfig {
    pub fn runtime_enabled(&self, runtime: &str) -> bool {
        self.enabled_runtimes.iter().any(|item| item == runtime)
    }
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
    fn default_enabled_runtimes_are_present() {
        let config = AppConfig::default();
        assert!(config.runtime_enabled("claude-code"));
        assert!(config.runtime_enabled("codex-cli"));
    }

    #[test]
    fn serializes_runtime_settings_without_credentials() {
        let config = AppConfig {
            enabled_runtimes: default_enabled_runtimes(),
            onboarding_completed: false,
        };
        let json = serde_json::to_string(&config).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("access_token").is_none());
    }

    #[test]
    fn deserializes_missing_runtime_settings_as_defaults() {
        let config: AppConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(config.runtime_enabled("gemini-cli"));
    }

    #[test]
    fn round_trips_runtime_settings_through_json() {
        let original = AppConfig {
            enabled_runtimes: vec!["codex-cli".to_string()],
            onboarding_completed: true,
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.enabled_runtimes, original.enabled_runtimes);
        assert_eq!(restored.onboarding_completed, original.onboarding_completed);
    }

    #[test]
    fn load_returns_default_when_file_absent() {
        let _ = AppConfig::load();
    }
}
