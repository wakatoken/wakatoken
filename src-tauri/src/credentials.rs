use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const SERVICE: &str = "com.wakatoken.client";
const ACCOUNT: &str = "realmroot-oauth";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthCredentials {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at: i64,
}

impl AuthCredentials {
    pub fn load() -> Self {
        let entry = match Entry::new(SERVICE, ACCOUNT) {
            Ok(entry) => entry,
            Err(_) => return Self::load_legacy(),
        };
        match entry.get_password() {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::load_legacy(),
        }
    }

    fn load_legacy() -> Self {
        let path = legacy_credentials_path();
        let credentials = fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str::<Self>(&json).ok())
            .unwrap_or_default();
        if credentials.signed_in() && credentials.save().is_ok() {
            let _ = fs::remove_file(path);
        }
        credentials
    }

    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|error| error.to_string())?;
        Entry::new(SERVICE, ACCOUNT)
            .map_err(|error| error.to_string())?
            .set_password(&json)
            .map_err(|error| error.to_string())
    }

    pub fn clear() -> Result<(), String> {
        let entry = Entry::new(SERVICE, ACCOUNT).map_err(|error| error.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(error) => return Err(error.to_string()),
        }
        let path = legacy_credentials_path();
        if path.exists() {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn signed_in(&self) -> bool {
        !self.access_token.is_empty()
    }
}

fn legacy_credentials_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(SERVICE)
        .join("credentials.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_signed_out() {
        assert!(!AuthCredentials::default().signed_in());
    }

    #[test]
    fn serializes_refresh_and_expiry() {
        let credentials = AuthCredentials {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at: 123,
        };
        let value = serde_json::to_value(credentials).unwrap();
        assert_eq!(value["refresh_token"], "refresh");
        assert_eq!(value["expires_at"], 123);
    }
}
