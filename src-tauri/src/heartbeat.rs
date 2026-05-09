use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heartbeat {
    pub event_id: String,
    pub project: String,
    pub provider: String,
    pub model: String,
    pub source: String,
    pub os: String,
    pub machine_id: String,
    pub git_branch: String,
    pub language: String,
    pub tool: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub input_context_tokens: u64,
    pub event_ts: i64,
}

pub fn get_machine_id() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .map_err(|e| e.to_string())?;
        let s = String::from_utf8_lossy(&output.stdout);
        let id = s
            .lines()
            .find(|line| line.contains("IOPlatformUUID"))
            .and_then(|line| line.split('"').nth(3))
            .ok_or("cannot read macOS IOPlatformUUID")?;
        return validate_machine_id(id);
    }

    #[cfg(target_os = "linux")]
    {
        let id = std::fs::read_to_string("/etc/machine-id").map_err(|e| e.to_string())?;
        return validate_machine_id(id.trim());
    }

    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("reg")
            .args([
                "query",
                r"HKLM\SOFTWARE\Microsoft\Cryptography",
                "/v",
                "MachineGuid",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&output.stdout);
        let id = text
            .lines()
            .find(|line| line.contains("MachineGuid"))
            .and_then(|line| line.split_whitespace().last())
            .ok_or("cannot read Windows MachineGuid")?;
        return validate_machine_id(id);
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err("unsupported platform for machineId".to_string())
    }
}

fn validate_machine_id(value: &str) -> Result<String, String> {
    if is_machine_id(value) {
        Ok(value.to_string())
    } else {
        Err(format!("invalid machineId format: {value}"))
    }
}

fn is_machine_id(value: &str) -> bool {
    is_hex(value, 32) || is_uuid(value)
}

fn is_uuid(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && is_hex(parts[0], 8)
        && is_hex(parts[1], 4)
        && is_hex(parts[2], 4)
        && is_hex(parts[3], 4)
        && is_hex(parts[4], 12)
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_heartbeat() -> Heartbeat {
        Heartbeat {
            event_id: "msg_abc:req_xyz".to_string(),
            project: "myproject".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            source: "claude-code".to_string(),
            os: "macos".to_string(),
            machine_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            git_branch: "main".to_string(),
            language: "Rust".to_string(),
            tool: "Edit".to_string(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_write_tokens: 100,
            input_context_tokens: 1300,
            event_ts: 1710000000000,
        }
    }

    #[test]
    fn test_machine_id_is_not_empty() {
        let id = get_machine_id().expect("machine id");
        assert!(!id.is_empty());
        println!("Machine ID: {}", id);
    }

    #[test]
    fn rejects_hostname_as_machine_id() {
        assert!(validate_machine_id("my-mac").is_err());
    }

    #[test]
    fn accepts_uuid_machine_id() {
        assert!(validate_machine_id("550e8400-e29b-41d4-a716-446655440000").is_ok());
    }

    #[test]
    fn accepts_linux_machine_id() {
        assert!(validate_machine_id("6f1ed002ab5595859014ebf0951522d9").is_ok());
    }

    #[test]
    fn serializes_machine_id_as_camel_case() {
        let hb = sample_heartbeat();
        let json: serde_json::Value = serde_json::to_value(&hb).unwrap();
        assert!(json.get("machineId").is_some());
        assert!(json.get("machine").is_none());
    }

    #[test]
    fn deserializes_camel_case_json() {
        let json = r#"{
            "eventId": "id1:req1",
            "project": "proj",
            "provider": "anthropic",
            "model": "claude-3",
            "source": "claude-code",
            "os": "linux",
            "machineId": "6f1ed002ab5595859014ebf0951522d9",
            "gitBranch": "feature",
            "language": "Go",
            "tool": "Read",
            "inputTokens": 42,
            "outputTokens": 7,
            "cacheReadTokens": 0,
            "cacheWriteTokens": 0,
            "inputContextTokens": 42,
            "eventTs": 1710000001000
        }"#;
        let hb: Heartbeat = serde_json::from_str(json).unwrap();
        assert_eq!(hb.machine_id, "6f1ed002ab5595859014ebf0951522d9");
        assert_eq!(hb.git_branch, "feature");
        assert_eq!(hb.input_tokens, 42);
        assert_eq!(hb.input_context_tokens, 42);
    }
}
