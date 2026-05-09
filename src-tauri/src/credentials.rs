use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthCredentials {
    #[serde(default)]
    pub access_token: String,
}

impl AuthCredentials {
    pub fn load() -> Self {
        let path = credentials_path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = credentials_path();
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }

    pub fn clear() -> Result<(), String> {
        let path = credentials_path();
        if path.exists() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn signed_in(&self) -> bool {
        !self.access_token.is_empty()
    }
}

fn credentials_path() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.wakatoken.client");
    fs::create_dir_all(&dir).ok();
    dir.join("credentials.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_access_token_is_empty() {
        let credentials = AuthCredentials::default();
        assert_eq!(credentials.access_token, "");
        assert!(!credentials.signed_in());
    }

    #[test]
    fn serializes_access_token() {
        let credentials = AuthCredentials {
            access_token: "access-token".to_string(),
        };
        let json = serde_json::to_string(&credentials).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["access_token"].as_str().unwrap(), "access-token");
    }

    #[test]
    fn deserializes_missing_access_token_as_empty_string() {
        let credentials: AuthCredentials = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(credentials.access_token, "");
    }
}
