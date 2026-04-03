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
    pub machine: String,
    pub git_branch: String,
    pub language: String,
    pub tool: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub event_ts: i64,
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
            machine: "laptop".to_string(),
            git_branch: "main".to_string(),
            language: "Rust".to_string(),
            tool: "Edit".to_string(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_write_tokens: 100,
            event_ts: 1710000000000,
        }
    }

    #[test]
    fn serializes_event_id_as_camel_case() {
        let hb = sample_heartbeat();
        let json: serde_json::Value = serde_json::to_value(&hb).unwrap();
        assert!(
            json.get("eventId").is_some(),
            "expected camelCase 'eventId'"
        );
        assert!(
            json.get("event_id").is_none(),
            "snake_case 'event_id' must not appear"
        );
    }

    #[test]
    fn serializes_all_fields_as_camel_case() {
        let hb = sample_heartbeat();
        let json: serde_json::Value = serde_json::to_value(&hb).unwrap();
        let expected_keys = [
            "eventId",
            "project",
            "provider",
            "model",
            "source",
            "os",
            "machine",
            "gitBranch",
            "language",
            "tool",
            "inputTokens",
            "outputTokens",
            "cacheReadTokens",
            "cacheWriteTokens",
            "eventTs",
        ];
        for key in &expected_keys {
            assert!(json.get(key).is_some(), "missing camelCase key: {key}");
        }
    }

    #[test]
    fn serializes_token_counts_correctly() {
        let hb = sample_heartbeat();
        let json: serde_json::Value = serde_json::to_value(&hb).unwrap();
        assert_eq!(json["inputTokens"].as_u64().unwrap(), 1000);
        assert_eq!(json["outputTokens"].as_u64().unwrap(), 500);
        assert_eq!(json["cacheReadTokens"].as_u64().unwrap(), 200);
        assert_eq!(json["cacheWriteTokens"].as_u64().unwrap(), 100);
    }

    #[test]
    fn round_trips_through_json() {
        let original = sample_heartbeat();
        let json = serde_json::to_string(&original).unwrap();
        let restored: Heartbeat = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.event_id, original.event_id);
        assert_eq!(restored.input_tokens, original.input_tokens);
        assert_eq!(restored.cache_read_tokens, original.cache_read_tokens);
        assert_eq!(restored.event_ts, original.event_ts);
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
            "machine": "host",
            "gitBranch": "feature",
            "language": "Go",
            "tool": "Read",
            "inputTokens": 42,
            "outputTokens": 7,
            "cacheReadTokens": 0,
            "cacheWriteTokens": 0,
            "eventTs": 1710000001000
        }"#;
        let hb: Heartbeat = serde_json::from_str(json).unwrap();
        assert_eq!(hb.event_id, "id1:req1");
        assert_eq!(hb.git_branch, "feature");
        assert_eq!(hb.input_tokens, 42);
    }

    #[test]
    fn zero_token_counts_serialize_as_zero() {
        let mut hb = sample_heartbeat();
        hb.input_tokens = 0;
        hb.output_tokens = 0;
        let json: serde_json::Value = serde_json::to_value(&hb).unwrap();
        assert_eq!(json["inputTokens"].as_u64().unwrap(), 0);
        assert_eq!(json["outputTokens"].as_u64().unwrap(), 0);
    }
}
