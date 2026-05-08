use crate::collector::{Collector, SessionFile};
use crate::heartbeat::Heartbeat;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct GeminiCollector {
    offsets: Mutex<HashMap<PathBuf, u64>>,
    offsets_path: PathBuf,
}

impl Default for GeminiCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiCollector {
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

impl Collector for GeminiCollector {
    fn name(&self) -> &str {
        "gemini-cli"
    }

    fn collect(&self) -> Result<Vec<SessionFile>, String> {
        let root = dirs::home_dir()
            .ok_or("cannot find home directory")?
            .join(".gemini")
            .join("tmp");

        if !root.exists() {
            return Ok(vec![]);
        }

        let files = find_session_files(&root);
        let offsets = self.offsets.lock().map_err(|e| e.to_string())?;
        let mut sessions = Vec::new();

        for file in &files {
            let prev_offset = offsets.get(file).copied().unwrap_or(0);
            match parse_file_incremental(file, prev_offset) {
                Ok((heartbeats, new_offset)) => {
                    if !heartbeats.is_empty() {
                        sessions.push(SessionFile {
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

    fn commit_file(&self, path: &Path, offset: u64) {
        let mut offsets = self.offsets.lock().unwrap();
        offsets.insert(path.to_path_buf(), offset);
        save_offsets(&self.offsets_path, &offsets);
    }
}

fn find_session_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(dir, 0, &mut files);
    files
}

fn walk(dir: &Path, depth: u32, files: &mut Vec<PathBuf>) {
    if depth > 5 {
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
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_session = name.starts_with("session-");
        let is_supported = name.ends_with(".jsonl") || name.ends_with(".json");
        if is_session && is_supported {
            files.push(path);
        }
    }
}

fn parse_file_incremental(path: &Path, offset: u64) -> Result<(Vec<Heartbeat>, u64), String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == "jsonl" {
        parse_jsonl_incremental(path, offset)
    } else {
        parse_json_snapshot(path, offset)
    }
}

fn parse_jsonl_incremental(path: &Path, offset: u64) -> Result<(Vec<Heartbeat>, u64), String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let file_len = file.metadata().map_err(|e| e.to_string())?.len();
    let seek_to = if offset > file_len { 0 } else { offset };

    file.seek(SeekFrom::Start(seek_to))
        .map_err(|e| e.to_string())?;

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();
    let platform = std::env::consts::OS;
    let project = extract_project(path);

    let mut reader = BufReader::new(file);
    let mut dedup: HashMap<String, Heartbeat> = HashMap::new();
    let mut bytes_read = seek_to;

    let mut line = String::new();
    loop {
        line.clear();
        let line_start = bytes_read;
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes_read += n as u64;

        let event_fallback = format!("{}:{line_start}", path.display());
        if let Some(hb) = parse_message_record(&line, &event_fallback, &project, &hostname, platform) {
            dedup.insert(hb.event_id.clone(), hb);
        }
    }

    Ok((dedup.into_values().collect(), bytes_read))
}

fn parse_json_snapshot(path: &Path, offset: u64) -> Result<(Vec<Heartbeat>, u64), String> {
    let file_len = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if file_len == offset {
        return Ok((vec![], file_len));
    }

    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let data: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let messages = data
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or("invalid gemini session json: missing messages")?;

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();
    let platform = std::env::consts::OS;
    let project = extract_project(path);
    let mut dedup: HashMap<String, Heartbeat> = HashMap::new();

    for (idx, msg) in messages.iter().enumerate() {
        let event_fallback = format!("{}:{idx}", path.display());
        if let Some(hb) = parse_message_value(msg, &event_fallback, &project, &hostname, platform) {
            dedup.insert(hb.event_id.clone(), hb);
        }
    }

    Ok((dedup.into_values().collect(), file_len))
}

fn parse_message_record(
    line: &str,
    event_fallback: &str,
    project: &str,
    hostname: &str,
    platform: &str,
) -> Option<Heartbeat> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let record: Value = serde_json::from_str(trimmed).ok()?;
    parse_message_value(&record, event_fallback, project, hostname, platform)
}

fn parse_message_value(
    record: &Value,
    event_fallback: &str,
    project: &str,
    hostname: &str,
    platform: &str,
) -> Option<Heartbeat> {
    if record.get("type").and_then(|v| v.as_str()) != Some("gemini") {
        return None;
    }

    let tokens = record.get("tokens")?;
    let input_tokens = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
    let output_tokens = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read_tokens = tokens.get("cached").and_then(|v| v.as_u64()).unwrap_or(0);
    if input_tokens == 0 && output_tokens == 0 {
        return None;
    }

    let timestamp = record.get("timestamp")?.as_str()?;
    let event_ts = chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.timestamp_millis())
        .ok()
        .filter(|&ts| ts > 0)?;

    let model = record
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|m| !m.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let msg_id = record.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let event_id = if msg_id.is_empty() {
        event_fallback.to_string()
    } else {
        format!("gemini:{msg_id}")
    };

    Some(Heartbeat {
        event_id,
        project: project.to_string(),
        provider: "google".to_string(),
        model,
        source: "gemini-cli".to_string(),
        os: platform.to_string(),
        machine: hostname.to_string(),
        git_branch: String::new(),
        language: String::new(),
        tool: String::new(),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens: 0,
        event_ts,
    })
}

fn extract_project(path: &Path) -> String {
    let workspace = path
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let project_root_file = workspace.join(".project_root");
    if let Ok(root) = fs::read_to_string(project_root_file) {
        let root = root.trim();
        if !root.is_empty() {
            return Path::new(root)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
        }
    }
    workspace
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_collector_name() {
        assert_eq!(GeminiCollector::new().name(), "gemini-cli");
    }

    #[test]
    fn parse_message_value_reads_tokens() {
        let msg = serde_json::json!({
            "id": "m1",
            "timestamp": "2026-05-08T10:00:01.000Z",
            "type": "gemini",
            "model": "gemini-3-flash-preview",
            "tokens": {
                "input": 120,
                "output": 40,
                "cached": 20
            }
        });
        let hb = parse_message_value(&msg, "fallback", "proj", "host", "macos")
            .expect("expected gemini heartbeat");
        assert_eq!(hb.event_id, "gemini:m1");
        assert_eq!(hb.provider, "google");
        assert_eq!(hb.model, "gemini-3-flash-preview");
        assert_eq!(hb.input_tokens, 120);
        assert_eq!(hb.output_tokens, 40);
        assert_eq!(hb.cache_read_tokens, 20);
    }

    #[test]
    fn parse_message_value_skips_non_gemini_type() {
        let msg = serde_json::json!({
            "id": "m1",
            "timestamp": "2026-05-08T10:00:01.000Z",
            "type": "user",
            "tokens": { "input": 10, "output": 1 }
        });
        assert!(parse_message_value(&msg, "fallback", "proj", "host", "macos").is_none());
    }
}
