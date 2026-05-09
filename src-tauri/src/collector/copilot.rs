use crate::collector::{Collector, SessionFile};
use crate::heartbeat::Heartbeat;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct CopilotCollector {
    offsets: Mutex<HashMap<PathBuf, u64>>,
    offsets_path: PathBuf,
}

#[derive(Default)]
struct SessionContext {
    cwd: String,
    git_branch: String,
}

impl Default for CopilotCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl CopilotCollector {
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

impl Collector for CopilotCollector {
    fn name(&self) -> &str {
        "copilot-agent"
    }

    fn collect(&self, machine_id: &str) -> Result<Vec<SessionFile>, String> {
        let root = dirs::home_dir()
            .ok_or("cannot find home directory")?
            .join(".copilot")
            .join("session-state");

        if !root.exists() {
            return Ok(vec![]);
        }

        let files = find_events_files(&root);
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
        self.scan_all_with_progress(machine_id, &mut |_, _| {})
    }

    fn scan_all_with_progress(
        &self,
        machine_id: &str,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Vec<SessionFile>, String> {
        let root = dirs::home_dir()
            .ok_or("cannot find home directory")?
            .join(".copilot")
            .join("session-state");

        if !root.exists() {
            return Ok(vec![]);
        }

        let files = find_events_files(&root);
        let total = files.len();
        let mut sessions = Vec::new();
        for (index, file) in files.into_iter().enumerate() {
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
            progress(index + 1, total);
        }
        Ok(sessions)
    }

    fn commit_file(&self, path: &Path, offset: u64) {
        let mut offsets = self.offsets.lock().unwrap();
        offsets.insert(path.to_path_buf(), offset);
        save_offsets(&self.offsets_path, &offsets);
    }
}

fn find_events_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(dir, 0, &mut files);
    files
}

fn walk(dir: &Path, depth: u32, files: &mut Vec<PathBuf>) {
    if depth > 3 {
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
        if path.is_file() && path.file_name().and_then(|n| n.to_str()) == Some("events.jsonl") {
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
        let line_start = bytes_read;
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }

        if let Some(items) = parse_line(
            &line,
            &format!("{}:{line_start}", path.display()),
            &machine_id,
            platform,
            &mut context,
        ) {
            bytes_read += n as u64;
            for hb in items {
                dedup.insert(hb.event_id.clone(), hb);
            }
            continue;
        }

        if !line.ends_with('\n') {
            break;
        }

        bytes_read += n as u64;
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
        if parse_session_start_context(&line, &mut context) {
            break;
        }
    }

    context
}

fn parse_session_start_context(line: &str, context: &mut SessionContext) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let record: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if record.get("type").and_then(|v| v.as_str()) != Some("session.start") {
        return false;
    }
    let data = match record.get("data") {
        Some(v) => v,
        None => return false,
    };
    let ctx = match data.get("context") {
        Some(v) => v,
        None => return false,
    };
    context.cwd = ctx
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    context.git_branch = ctx
        .get("branch")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    true
}

fn parse_line(
    line: &str,
    base_event_id: &str,
    machine_id: &str,
    platform: &str,
    context: &mut SessionContext,
) -> Option<Vec<Heartbeat>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let record: Value = serde_json::from_str(trimmed).ok()?;
    let record_type = record.get("type")?.as_str()?;

    if record_type == "session.start" {
        parse_session_start_context(trimmed, context);
        return None;
    }

    if record_type != "session.shutdown" {
        return None;
    }

    let data = record.get("data")?;
    let model_metrics = data.get("modelMetrics")?.as_object()?;
    let timestamp = record.get("timestamp")?.as_str()?;
    let event_ts = chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.timestamp_millis())
        .ok()
        .filter(|&ts| ts > 0)?;
    let source_event_id = record.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let project = extract_project(&context.cwd);

    let mut out = Vec::new();
    for (model_name, metric) in model_metrics {
        let usage = metric.get("usage")?;
        let raw_input_tokens = usage
            .get("inputTokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output_tokens = usage
            .get("outputTokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let cache_read_tokens = usage
            .get("cacheReadTokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let cache_write_tokens = usage
            .get("cacheWriteTokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let input_tokens = raw_input_tokens
            .saturating_sub(cache_read_tokens)
            .saturating_sub(cache_write_tokens);

        if input_tokens == 0
            && output_tokens == 0
            && cache_read_tokens == 0
            && cache_write_tokens == 0
        {
            continue;
        }

        let model = if model_name.is_empty() {
            "unknown".to_string()
        } else {
            model_name.to_string()
        };
        let event_id = if source_event_id.is_empty() {
            format!("{base_event_id}:{model}")
        } else {
            format!("{source_event_id}:{model}")
        };

        out.push(Heartbeat {
            event_id,
            project: project.clone(),
            provider: extract_provider(&model),
            model,
            source: "copilot-agent".to_string(),
            os: platform.to_string(),
            machine_id: machine_id.to_string(),
            git_branch: context.git_branch.clone(),
            language: String::new(),
            tool: String::new(),
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            event_ts,
        });
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn extract_project(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
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
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_start_json() -> String {
        serde_json::json!({
            "type": "session.start",
            "timestamp": "2026-05-08T10:00:00.000Z",
            "data": {
                "context": {
                    "cwd": "/home/user/myproject",
                    "branch": "main"
                }
            }
        })
        .to_string()
    }

    fn shutdown_json() -> String {
        serde_json::json!({
            "type": "session.shutdown",
            "id": "evt-1",
            "timestamp": "2026-05-08T10:01:00.000Z",
            "data": {
                "modelMetrics": {
                    "gpt-5.5": {
                        "usage": {
                            "inputTokens": 1200,
                            "outputTokens": 110,
                            "cacheReadTokens": 900,
                            "cacheWriteTokens": 0
                        }
                    }
                }
            }
        })
        .to_string()
    }

    #[test]
    fn copilot_collector_name() {
        assert_eq!(CopilotCollector::new().name(), "copilot-agent");
    }

    #[test]
    fn parse_line_reads_shutdown_usage() {
        let mut ctx = SessionContext::default();
        parse_line(&session_start_json(), "base-1", "host", "macos", &mut ctx);
        let items = parse_line(&shutdown_json(), "base-2", "host", "macos", &mut ctx)
            .expect("expected shutdown heartbeats");
        assert_eq!(items.len(), 1);
        let hb = &items[0];
        assert_eq!(hb.project, "myproject");
        assert_eq!(hb.git_branch, "main");
        assert_eq!(hb.model, "gpt-5.5");
        assert_eq!(hb.provider, "openai");
        assert_eq!(hb.input_tokens, 300);
        assert_eq!(hb.output_tokens, 110);
        assert_eq!(hb.cache_read_tokens, 900);
    }

    #[test]
    fn parse_line_skips_zero_usage() {
        let mut ctx = SessionContext::default();
        let line = serde_json::json!({
            "type":"session.shutdown",
            "timestamp":"2026-05-08T10:01:00.000Z",
            "data":{"modelMetrics":{"gpt-5.5":{"usage":{"inputTokens":0,"outputTokens":0}}}}
        })
        .to_string();
        assert!(parse_line(&line, "base", "host", "macos", &mut ctx).is_none());
    }
}
