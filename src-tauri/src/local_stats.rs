use crate::collector::SessionFile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const STORE_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalDashboard {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_write_tokens: u64,
    pub total_tokens: u64,
    pub session_count: u64,
    pub today_input_tokens: u64,
    pub today_output_tokens: u64,
    pub today_cache_read_tokens: u64,
    pub today_cache_write_tokens: u64,
    pub today_tokens: u64,
    pub today_session_count: u64,
    pub runtimes: Vec<RuntimeSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSummary {
    pub runtime: String,
    pub session_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub last_seen_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub path: String,
    pub runtime: String,
    pub project: String,
    pub model: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub event_count: u64,
    pub status: String,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LocalStatsStore {
    #[serde(default)]
    version: u32,
    sessions: Vec<SessionSummary>,
}

pub fn get_local_dashboard() -> Result<LocalDashboard, String> {
    let store = load_store()?;
    Ok(build_dashboard(&store.sessions))
}

pub fn list_sessions(
    runtime: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<SessionSummary>, String> {
    let mut sessions = load_store()?.sessions;
    if let Some(runtime) = runtime.filter(|value| !value.is_empty() && value != "all") {
        sessions.retain(|session| session.runtime == runtime);
    }
    sessions.sort_by(|a, b| b.ended_at.cmp(&a.ended_at));
    sessions.truncate(limit.unwrap_or(50));
    Ok(sessions)
}

pub fn record_session(session: &SessionFile, status: &str, last_error: &str) -> Result<(), String> {
    let mut store = load_store()?;
    let summary = summarize_session(session, status, last_error);
    store.sessions.retain(|item| item.id != summary.id);
    store.sessions.push(summary);
    save_store(&store)
}

pub fn rebuild_from_sessions(sessions: &[SessionFile]) -> Result<LocalDashboard, String> {
    let store = LocalStatsStore {
        version: STORE_VERSION,
        sessions: sessions
            .iter()
            .map(|session| summarize_session(session, "local", ""))
            .collect(),
    };
    save_store(&store)?;
    Ok(build_dashboard(&store.sessions))
}

fn summarize_session(session: &SessionFile, status: &str, last_error: &str) -> SessionSummary {
    let first = session.heartbeats.first();
    let started_at = session
        .heartbeats
        .iter()
        .map(|heartbeat| heartbeat.event_ts)
        .min()
        .unwrap_or(0);
    let ended_at = session
        .heartbeats
        .iter()
        .map(|heartbeat| heartbeat.event_ts)
        .max()
        .unwrap_or(0);
    let input_tokens = session.heartbeats.iter().map(|h| h.input_tokens).sum();
    let output_tokens = session.heartbeats.iter().map(|h| h.output_tokens).sum();
    let cache_read_tokens = session.heartbeats.iter().map(|h| h.cache_read_tokens).sum();
    let cache_write_tokens = session
        .heartbeats
        .iter()
        .map(|h| h.cache_write_tokens)
        .sum();

    SessionSummary {
        id: session.path.display().to_string(),
        path: session.path.display().to_string(),
        runtime: session.runtime.clone(),
        project: first.map(|h| h.project.clone()).unwrap_or_default(),
        model: first.map(|h| h.model.clone()).unwrap_or_default(),
        started_at,
        ended_at,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens: token_total(
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        ),
        event_count: session.heartbeats.len() as u64,
        status: status.to_string(),
        last_error: last_error.to_string(),
    }
}

fn build_dashboard(sessions: &[SessionSummary]) -> LocalDashboard {
    let mut dashboard = LocalDashboard::default();
    let mut runtimes = BTreeMap::<String, RuntimeSummary>::new();
    let today_start = today_start_millis();

    for session in sessions {
        dashboard.total_input_tokens += session.input_tokens;
        dashboard.total_output_tokens += session.output_tokens;
        dashboard.total_cache_read_tokens += session.cache_read_tokens;
        dashboard.total_cache_write_tokens += session.cache_write_tokens;
        if session.ended_at >= today_start {
            dashboard.today_input_tokens += session.input_tokens;
            dashboard.today_output_tokens += session.output_tokens;
            dashboard.today_cache_read_tokens += session.cache_read_tokens;
            dashboard.today_cache_write_tokens += session.cache_write_tokens;
            dashboard.today_session_count += 1;
        }

        let runtime = runtimes
            .entry(session.runtime.clone())
            .or_insert_with(|| RuntimeSummary {
                runtime: session.runtime.clone(),
                ..RuntimeSummary::default()
            });
        runtime.session_count += 1;
        runtime.input_tokens += session.input_tokens;
        runtime.output_tokens += session.output_tokens;
        runtime.cache_read_tokens += session.cache_read_tokens;
        runtime.cache_write_tokens += session.cache_write_tokens;
        runtime.last_seen_at = runtime.last_seen_at.max(session.ended_at);
    }

    dashboard.total_tokens = token_total(
        dashboard.total_input_tokens,
        dashboard.total_output_tokens,
        dashboard.total_cache_read_tokens,
        dashboard.total_cache_write_tokens,
    );
    dashboard.today_tokens = token_total(
        dashboard.today_input_tokens,
        dashboard.today_output_tokens,
        dashboard.today_cache_read_tokens,
        dashboard.today_cache_write_tokens,
    );
    dashboard.session_count = sessions.len() as u64;
    dashboard.runtimes = runtimes
        .into_values()
        .map(|mut runtime| {
            runtime.total_tokens = token_total(
                runtime.input_tokens,
                runtime.output_tokens,
                runtime.cache_read_tokens,
                runtime.cache_write_tokens,
            );
            runtime
        })
        .collect();
    dashboard
}

fn token_total(input: u64, output: u64, cache_read: u64, cache_write: u64) -> u64 {
    input + output + cache_read + cache_write
}

fn today_start_millis() -> i64 {
    let now = chrono::Local::now();
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp_millis()
}

fn load_store() -> Result<LocalStatsStore, String> {
    let path = stats_path()?;
    if !path.exists() {
        return Ok(LocalStatsStore::default());
    }
    let data = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut store: LocalStatsStore = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    if store.version < STORE_VERSION {
        normalize_legacy_sessions(&mut store.sessions);
        store.version = STORE_VERSION;
        save_store(&store)?;
        return Ok(store);
    }
    refresh_session_totals(&mut store.sessions);
    Ok(store)
}

fn normalize_legacy_sessions(sessions: &mut [SessionSummary]) {
    for session in sessions {
        if input_includes_cache(&session.runtime) {
            session.input_tokens = session
                .input_tokens
                .saturating_sub(session.cache_read_tokens)
                .saturating_sub(session.cache_write_tokens);
        }
        session.total_tokens = token_total(
            session.input_tokens,
            session.output_tokens,
            session.cache_read_tokens,
            session.cache_write_tokens,
        );
    }
}

fn refresh_session_totals(sessions: &mut [SessionSummary]) {
    for session in sessions {
        session.total_tokens = token_total(
            session.input_tokens,
            session.output_tokens,
            session.cache_read_tokens,
            session.cache_write_tokens,
        );
    }
}

fn input_includes_cache(runtime: &str) -> bool {
    matches!(runtime, "codex-cli" | "gemini-cli" | "copilot-agent")
}

fn save_store(store: &LocalStatsStore) -> Result<(), String> {
    let path = stats_path()?;
    let data = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())
}

fn stats_path() -> Result<PathBuf, String> {
    let dir = dirs::config_dir()
        .ok_or("cannot find config directory")?
        .join("com.wakatoken.client");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("local-stats.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heartbeat::Heartbeat;

    fn heartbeat(event_id: &str, runtime: &str, input: u64, output: u64) -> Heartbeat {
        Heartbeat {
            event_id: event_id.to_string(),
            project: "project".to_string(),
            provider: "provider".to_string(),
            model: "model".to_string(),
            source: runtime.to_string(),
            os: "macos".to_string(),
            machine_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            git_branch: "main".to_string(),
            language: "Rust".to_string(),
            tool: "Edit".to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 1,
            cache_write_tokens: 2,
            event_ts: 1710000000000,
        }
    }

    #[test]
    fn summarizes_session_tokens() {
        let session = SessionFile {
            runtime: "claude-code".to_string(),
            path: PathBuf::from("/tmp/session.jsonl"),
            offset: 10,
            heartbeats: vec![
                heartbeat("e1", "claude-code", 10, 3),
                heartbeat("e2", "claude-code", 7, 2),
            ],
        };

        let summary = summarize_session(&session, "synced", "");

        assert_eq!(summary.runtime, "claude-code");
        assert_eq!(summary.input_tokens, 17);
        assert_eq!(summary.output_tokens, 5);
        assert_eq!(summary.cache_read_tokens, 2);
        assert_eq!(summary.cache_write_tokens, 4);
        assert_eq!(summary.total_tokens, 28);
        assert_eq!(summary.event_count, 2);
    }

    #[test]
    fn builds_runtime_dashboard() {
        let sessions = vec![SessionSummary {
            id: "a".to_string(),
            path: "a".to_string(),
            runtime: "codex-cli".to_string(),
            project: "p".to_string(),
            model: "m".to_string(),
            started_at: 1,
            ended_at: 2,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 3,
            cache_write_tokens: 2,
            total_tokens: 20,
            event_count: 1,
            status: "synced".to_string(),
            last_error: String::new(),
        }];

        let dashboard = build_dashboard(&sessions);

        assert_eq!(dashboard.total_tokens, 20);
        assert_eq!(dashboard.total_cache_read_tokens, 3);
        assert_eq!(dashboard.total_cache_write_tokens, 2);
        assert_eq!(dashboard.runtimes[0].runtime, "codex-cli");
        assert_eq!(dashboard.runtimes[0].total_tokens, 20);
        assert_eq!(dashboard.runtimes[0].session_count, 1);
    }
}
