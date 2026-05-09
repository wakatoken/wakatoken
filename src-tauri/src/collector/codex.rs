use crate::collector::{Collector, SessionFile};
use crate::heartbeat::Heartbeat;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct CodexCollector {
    offsets: Mutex<HashMap<PathBuf, u64>>,
    offsets_path: PathBuf,
}

#[derive(Default)]
struct SessionContext {
    cwd: String,
    git_branch: String,
    model: String,
    provider: String,
}

impl Default for CodexCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexCollector {
    pub fn new() -> Self {
        let offsets_path = offsets_file_path();
        let offsets = load_offsets(&offsets_path);
        Self {
            offsets: Mutex::new(offsets),
            offsets_path,
        }
    }
}

fn offsets_file_path() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.wakatoken.client");
    fs::create_dir_all(&dir).ok();
    dir.join("offsets.json")
}

fn load_offsets(path: &Path) -> HashMap<PathBuf, u64> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_offsets(path: &Path, offsets: &HashMap<PathBuf, u64>) {
    if let Ok(json) = serde_json::to_string(offsets) {
        fs::write(path, json).ok();
    }
}

impl Collector for CodexCollector {
    fn name(&self) -> &str {
        "codex-cli"
    }

    fn collect(&self, machine_id: &str) -> Result<Vec<SessionFile>, String> {
        let codex_home = std::env::var("CODEX_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
            .ok_or("cannot find codex home directory")?;
        let codex_dir = codex_home.join("sessions");

        if !codex_dir.exists() {
            return Ok(vec![]);
        }

        let files = find_jsonl_files(&codex_dir);
        let offsets = self.offsets.lock().map_err(|e| e.to_string())?;
        let mut sessions = Vec::new();

        for file in &files {
            let prev_offset = offsets.get(file).copied().unwrap_or(0);
            match parse_jsonl_incremental(file, prev_offset, machine_id) {
                Ok((heartbeats, new_offset)) => {
                    if !heartbeats.is_empty() {
                        sessions.push(SessionFile {
                            runtime: self.name().to_string(),
                            path: file.clone(),
                            offset: new_offset,
                            heartbeats,
                        });
                    }
                }
                Err(e) => log::warn!("skipping {}: {e}", file.display()),
            }
        }

        Ok(sessions)
    }

    fn scan_all(&self, machine_id: &str) -> Result<Vec<SessionFile>, String> {
        let codex_home = std::env::var("CODEX_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
            .ok_or("cannot find codex home directory")?;
        let codex_dir = codex_home.join("sessions");

        if !codex_dir.exists() {
            return Ok(vec![]);
        }

        let mut sessions = Vec::new();
        for file in find_jsonl_files(&codex_dir) {
            if let Ok((heartbeats, offset)) = parse_jsonl_incremental(&file, 0, machine_id) {
                if !heartbeats.is_empty() {
                    sessions.push(SessionFile {
                        runtime: self.name().to_string(),
                        path: file,
                        offset,
                        heartbeats,
                    });
                }
            }
        }
        Ok(sessions)
    }

    fn commit_file(&self, path: &Path, offset: u64) {
        let mut offsets = self.offsets.lock().unwrap();
        offsets.insert(path.to_path_buf(), offset);
        save_offsets(&self.offsets_path, &offsets);
    }
}

fn find_jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(dir, 0, &mut files);
    files
}

fn walk(dir: &Path, depth: u32, files: &mut Vec<PathBuf>) {
    if depth > 6 {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, depth + 1, files);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with("rollout-") && name.ends_with(".jsonl") {
            files.push(path);
        }
    }
}

fn parse_jsonl_incremental(
    path: &Path,
    offset: u64,
    machine_id: &str,
) -> Result<(Vec<Heartbeat>, u64), String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let file_len = file.metadata().map_err(|e| e.to_string())?.len();
    let seek_to = if offset > file_len { 0 } else { offset };

    file.seek(SeekFrom::Start(seek_to))
        .map_err(|e| e.to_string())?;

    let platform = std::env::consts::OS;

    let mut reader = BufReader::new(file);
    let mut dedup: HashMap<String, Heartbeat> = HashMap::new();
    let mut bytes_read = seek_to;
    let mut context = read_initial_context(path);

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes_read += n as u64;

        if let Some(hb) = parse_line(&line, path, &machine_id, platform, &mut context) {
            let eid = hb.event_id.clone();
            dedup.insert(eid, hb);
        }
    }

    Ok((dedup.into_values().collect(), bytes_read))
}

fn read_initial_context(path: &Path) -> SessionContext {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return SessionContext::default(),
    };
    let mut reader = BufReader::new(file);
    let mut context = SessionContext::default();
    let mut line = String::new();
    let mut scanned = 0usize;

    loop {
        line.clear();
        let n = match reader.read_line(&mut line) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        scanned += 1;
        if scanned > 100 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if record.get("type").and_then(|v| v.as_str()) != Some("session_meta") {
            continue;
        }

        if let Some(payload) = record.get("payload").and_then(|v| v.as_object()) {
            context.cwd = payload
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            context.model = payload
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            context.provider = payload
                .get("model_provider")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            context.git_branch = payload
                .get("git")
                .and_then(|v| v.get("branch"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        break;
    }

    context
}

fn parse_line(
    line: &str,
    path: &Path,
    machine_id: &str,
    platform: &str,
    context: &mut SessionContext,
) -> Option<Heartbeat> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let record: Value = serde_json::from_str(trimmed).ok()?;
    let record_type = record.get("type")?.as_str()?;

    if let Some(payload_model) = record
        .get("payload")
        .and_then(|v| v.get("model"))
        .and_then(|v| v.as_str())
    {
        context.model = payload_model.to_string();
    }

    if record_type == "session_meta" {
        let payload = record.get("payload")?;
        context.cwd = payload
            .get("cwd")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        context.model = payload
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        context.provider = payload
            .get("model_provider")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        context.git_branch = payload
            .get("git")
            .and_then(|v| v.get("branch"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return None;
    }

    if record_type != "event_msg" {
        return None;
    }

    let payload = record.get("payload")?;
    if payload.get("type")?.as_str()? != "token_count" {
        return None;
    }

    let info = payload.get("info")?;
    let usage = info
        .get("last_token_usage")
        .or_else(|| info.get("total_token_usage"))?;

    let raw_input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read_tokens = usage
        .get("cached_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let input_tokens = raw_input_tokens.saturating_sub(cache_read_tokens);

    if input_tokens == 0 && output_tokens == 0 && cache_read_tokens == 0 {
        return None;
    }

    let total_usage = info.get("total_token_usage");
    let total_in = total_usage
        .and_then(|v| v.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_out = total_usage
        .and_then(|v| v.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let event_id = format!("{}:{total_in}:{total_out}", path.display());

    let timestamp = record.get("timestamp")?.as_str()?;
    let event_ts = chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.timestamp_millis())
        .ok()
        .filter(|&ts| ts > 0)?;

    let model = if context.model.is_empty() {
        payload
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        context.model.clone()
    };
    let model = if model.is_empty() {
        "gpt-unknown".to_string()
    } else {
        model
    };

    let provider = if context.provider.is_empty() {
        extract_provider(&model)
    } else {
        normalize_provider(&context.provider)
    };

    Some(Heartbeat {
        event_id,
        project: extract_project(&context.cwd),
        provider,
        model,
        source: "codex-cli".to_string(),
        os: platform.to_string(),
        machine_id: machine_id.to_string(),
        git_branch: context.git_branch.clone(),
        language: String::new(),
        tool: String::new(),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens: 0,
        event_ts,
    })
}

fn extract_project(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "gpt-unknown".to_string())
}

fn normalize_provider(provider: &str) -> String {
    let p = provider.to_ascii_lowercase();
    if p.contains("openai") {
        "openai".to_string()
    } else if p.contains("anthropic") {
        "anthropic".to_string()
    } else {
        provider.to_string()
    }
}

fn extract_provider(model: &str) -> String {
    if model.starts_with("claude") {
        "anthropic".to_string()
    } else if model.starts_with("gpt")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
        || model.starts_with("codex")
    {
        "openai".to_string()
    } else {
        "gpt-unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_meta_json() -> String {
        serde_json::json!({
            "timestamp": "2026-05-08T10:00:00.000Z",
            "type": "session_meta",
            "payload": {
                "cwd": "/home/user/myproject",
                "model_provider": "openai",
                "git": { "branch": "main" }
            }
        })
        .to_string()
    }

    fn model_event_json(model: &str) -> String {
        serde_json::json!({
            "timestamp": "2026-05-08T10:00:00.500Z",
            "type": "event_msg",
            "payload": {
                "type": "agent_message",
                "model": model
            }
        })
        .to_string()
    }

    fn token_count_json(input: u64, output: u64, cached: u64) -> String {
        serde_json::json!({
            "timestamp": "2026-05-08T10:00:01.000Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": input,
                        "output_tokens": output,
                        "cached_input_tokens": cached
                    }
                }
            }
        })
        .to_string()
    }

    #[test]
    fn codex_collector_name() {
        assert_eq!(CodexCollector::new().name(), "codex-cli");
    }

    #[test]
    fn parse_line_reads_token_count() {
        let mut ctx = SessionContext::default();
        parse_line(
            &session_meta_json(),
            Path::new("test"),
            "host",
            "macos",
            &mut ctx,
        );

        let hb = parse_line(
            &token_count_json(120, 40, 30),
            Path::new("test"),
            "host",
            "macos",
            &mut ctx,
        )
        .expect("expected token heartbeat");
        assert_eq!(hb.project, "myproject");
        assert_eq!(hb.provider, "openai");
        assert_eq!(hb.git_branch, "main");
        assert_eq!(hb.input_tokens, 90);
        assert_eq!(hb.output_tokens, 40);
        assert_eq!(hb.cache_read_tokens, 30);
    }

    #[test]
    fn parse_line_uses_model_from_non_token_event_context() {
        let mut ctx = SessionContext::default();
        parse_line(
            &session_meta_json(),
            Path::new("test"),
            "host",
            "macos",
            &mut ctx,
        );
        parse_line(
            &model_event_json("gpt-5.5"),
            Path::new("test"),
            "host",
            "macos",
            &mut ctx,
        );

        let hb = parse_line(
            &token_count_json(10, 5, 2),
            Path::new("test"),
            "host",
            "macos",
            &mut ctx,
        )
        .expect("expected token heartbeat");
        assert_eq!(hb.model, "gpt-5.5");
    }

    #[test]
    fn parse_line_skips_zero_tokens() {
        let mut ctx = SessionContext::default();
        assert!(parse_line(
            &token_count_json(0, 0, 0),
            Path::new("test"),
            "h",
            "m",
            &mut ctx
        )
        .is_none());
    }

    #[test]
    fn parse_line_skips_non_token_event() {
        let mut ctx = SessionContext::default();
        assert!(
            parse_line(
                r#"{"timestamp":"2026-05-08T10:00:01.000Z","type":"event_msg","payload":{"type":"user_message"}}"#,
                Path::new("test"),
                "h",
                "m",
                &mut ctx
            )
            .is_none()
        );
    }
}
