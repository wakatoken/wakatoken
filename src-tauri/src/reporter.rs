use crate::heartbeat::Heartbeat;
use crate::BASE_URL;

const BATCH_SIZE: usize = 100;

pub struct ReportResult {
    pub new_count: u64,
    pub dedup_count: u64,
}

pub async fn send_heartbeats(
    client: &reqwest::Client,
    access_token: &str,
    heartbeats: Vec<Heartbeat>,
) -> Result<ReportResult, String> {
    let url = format!("{BASE_URL}/api/v1/heartbeat/batch");
    let mut total_new = 0u64;
    let mut total_dedup = 0u64;

    for chunk in heartbeats.chunks(BATCH_SIZE) {
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {access_token}"))
            .json(&chunk)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<body read error: {e}>"));
            return Err(format!("HTTP {status}: {body}"));
        }

        let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        total_new += result.get("new").and_then(|v| v.as_u64()).unwrap_or(0);
        total_dedup += result
            .get("deduplicated")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
    }

    Ok(ReportResult {
        new_count: total_new,
        dedup_count: total_dedup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heartbeat::Heartbeat;

    fn sample_heartbeat(id: &str) -> Heartbeat {
        Heartbeat {
            event_id: id.to_string(),
            project: "proj".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-3".to_string(),
            source: "claude-code".to_string(),
            os: "linux".to_string(),
            machine_id: "6f1ed002ab5595859014ebf0951522d9".to_string(),
            git_branch: "main".to_string(),
            language: "Rust".to_string(),
            tool: "Edit".to_string(),
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            input_context_tokens: 100,
            event_ts: 1710000000000,
        }
    }

    #[test]
    fn empty_list_returns_zero() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let r = rt
            .block_on(send_heartbeats(&client, "access-token", vec![]))
            .unwrap();
        assert_eq!(r.new_count, 0);
        assert_eq!(r.dedup_count, 0);
    }

    #[test]
    fn heartbeats_serialize_machine_id_as_camel_case() {
        let json = serde_json::to_value(&[sample_heartbeat("e1")]).unwrap();
        assert!(json[0].get("eventId").is_some());
        assert!(json[0].get("machineId").is_some());
        assert_eq!(json[0]["inputContextTokens"], 100);
        assert!(json[0].get("machine").is_none());
    }
}
