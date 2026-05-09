use crate::collector::{Collector, SessionFile};
use crate::heartbeat::Heartbeat;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SKIP_DIRS: &[&str] = &["cache", "marketplaces", "fixtures"];

const EXT_LANGUAGES: &[(&str, &str)] = &[
    (".ts", "TypeScript"),
    (".tsx", "TypeScript"),
    (".js", "JavaScript"),
    (".jsx", "JavaScript"),
    (".py", "Python"),
    (".go", "Go"),
    (".rs", "Rust"),
    (".java", "Java"),
    (".rb", "Ruby"),
    (".php", "PHP"),
    (".swift", "Swift"),
    (".kt", "Kotlin"),
    (".c", "C"),
    (".h", "C"),
    (".cpp", "C++"),
    (".cc", "C++"),
    (".hpp", "C++"),
    (".cs", "C#"),
    (".vue", "Vue"),
    (".svelte", "Svelte"),
    (".html", "HTML"),
    (".css", "CSS"),
    (".scss", "CSS"),
    (".less", "CSS"),
    (".json", "JSON"),
    (".yaml", "YAML"),
    (".yml", "YAML"),
    (".md", "Markdown"),
    (".sql", "SQL"),
    (".sh", "Shell"),
    (".bash", "Shell"),
    (".zsh", "Shell"),
    (".toml", "TOML"),
    (".xml", "XML"),
];

pub struct ClaudeCollector {
    offsets: Mutex<HashMap<PathBuf, u64>>,
    offsets_path: PathBuf,
}

impl Default for ClaudeCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCollector {
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

impl Collector for ClaudeCollector {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn collect(&self, machine_id: &str) -> Result<Vec<SessionFile>, String> {
        let claude_dir = dirs::home_dir()
            .ok_or("cannot find home directory")?
            .join(".claude")
            .join("projects");

        if !claude_dir.exists() {
            return Ok(vec![]);
        }

        let files = find_jsonl_files(&claude_dir);
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
        let claude_dir = dirs::home_dir()
            .ok_or("cannot find home directory")?
            .join(".claude")
            .join("projects");

        if !claude_dir.exists() {
            return Ok(vec![]);
        }

        let files = find_jsonl_files(&claude_dir);
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

    fn scan_since(
        &self,
        machine_id: &str,
        offsets: &HashMap<PathBuf, u64>,
    ) -> Result<Vec<SessionFile>, String> {
        let claude_dir = dirs::home_dir()
            .ok_or("cannot find home directory")?
            .join(".claude")
            .join("projects");

        if !claude_dir.exists() {
            return Ok(vec![]);
        }

        let mut sessions = Vec::new();
        for file in find_jsonl_files(&claude_dir) {
            let prev_offset = offsets.get(&file).copied().unwrap_or(0);
            if let Ok((heartbeats, offset)) =
                parse_jsonl_incremental(&file, prev_offset, machine_id)
            {
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

// ── File scanning ───────────────────────────────────────────────────────

fn find_jsonl_files(dir: &Path) -> Vec<PathBuf> {
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
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if path.is_dir() && !SKIP_DIRS.contains(&name_str.as_ref()) {
            walk(&path, depth + 1, files);
        } else if path.is_file() && name_str.ends_with(".jsonl") {
            files.push(path);
        }
    }
}

// ── JSONL parsing ───────────────────────────────────────────────────────

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

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }

        if let Some((key, hb)) = parse_line(&line, &machine_id, platform) {
            bytes_read += n as u64;
            dedup.insert(key, hb);
            continue;
        }

        if !line.ends_with('\n') {
            break;
        }

        bytes_read += n as u64;
    }

    Ok((dedup.into_values().collect(), bytes_read))
}

fn parse_line(line: &str, machine_id: &str, platform: &str) -> Option<(String, Heartbeat)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let record: Value = serde_json::from_str(trimmed).ok()?;

    if record.get("type")?.as_str()? != "assistant" {
        return None;
    }

    let message = record.get("message")?;
    let usage = message.get("usage")?;

    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if input_tokens == 0 && output_tokens == 0 {
        return None;
    }

    let timestamp = record.get("timestamp")?.as_str()?;
    let event_ts = chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.timestamp_millis())
        .ok()
        .filter(|&ts| ts > 0)?;

    let msg_id = message.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let request_id = record
        .get("requestId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if msg_id.is_empty() && request_id.is_empty() {
        return None;
    }
    let dedup_key = format!("{msg_id}:{request_id}");

    let cwd = record.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
    let model = message.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let git_branch = record
        .get("gitBranch")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let content = message.get("content").and_then(|v| v.as_array());

    let (tool, language) = extract_tool_and_language(content);

    Some((
        dedup_key.clone(),
        Heartbeat {
            event_id: dedup_key,
            project: extract_project(cwd),
            provider: extract_provider(model),
            model: model.to_string(),
            source: "claude-code".to_string(),
            os: platform.to_string(),
            machine_id: machine_id.to_string(),
            git_branch: git_branch.to_string(),
            language,
            tool,
            input_tokens,
            output_tokens,
            cache_read_tokens: usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            cache_write_tokens: usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            event_ts,
        },
    ))
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn extract_tool_and_language(content: Option<&Vec<Value>>) -> (String, String) {
    let content = match content {
        Some(c) => c,
        None => return (String::new(), String::new()),
    };

    let mut tool_counts: HashMap<&str, usize> = HashMap::new();
    let mut lang_counts: HashMap<&str, usize> = HashMap::new();

    for block in content {
        if block.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
            continue;
        }
        if let Some(name) = block.get("name").and_then(|v| v.as_str()) {
            *tool_counts.entry(name).or_default() += 1;
        }
        if let Some(file_path) = block
            .get("input")
            .and_then(|v| v.get("file_path"))
            .and_then(|v| v.as_str())
        {
            if let Some(ext) = Path::new(file_path).extension().and_then(|e| e.to_str()) {
                let dot_ext = format!(".{}", ext.to_lowercase());
                for &(e, lang) in EXT_LANGUAGES {
                    if e == dot_ext {
                        *lang_counts.entry(lang).or_default() += 1;
                        break;
                    }
                }
            }
        }
    }

    let top_tool = tool_counts
        .into_iter()
        .max_by_key(|e| e.1)
        .map(|e| e.0.to_string())
        .unwrap_or_default();
    let top_lang = lang_counts
        .into_iter()
        .max_by_key(|e| e.1)
        .map(|e| e.0.to_string())
        .unwrap_or_default();
    (top_tool, top_lang)
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
    {
        "openai".to_string()
    } else {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const TEST_MACHINE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn extract_project_returns_last_component() {
        assert_eq!(extract_project("/home/user/proj"), "proj");
    }

    #[test]
    fn extract_project_empty_returns_unknown() {
        assert_eq!(extract_project(""), "unknown");
    }

    #[test]
    fn extract_provider_claude() {
        assert_eq!(extract_provider("claude-3-5-sonnet"), "anthropic");
    }

    #[test]
    fn extract_provider_gpt() {
        assert_eq!(extract_provider("gpt-4o"), "openai");
    }

    #[test]
    fn extract_provider_unknown() {
        assert_eq!(extract_provider("gemini"), "unknown");
    }

    fn assistant_json(msg_id: &str, req_id: &str, input: u64, output: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "requestId": req_id,
            "timestamp": "2024-03-10T12:00:00.000Z",
            "cwd": "/home/user/myproject",
            "gitBranch": "main",
            "message": {
                "id": msg_id,
                "model": "claude-3-5-sonnet",
                "content": [],
                "usage": { "input_tokens": input, "output_tokens": output }
            }
        })
        .to_string()
    }

    #[test]
    fn parse_line_valid() {
        let line = assistant_json("m1", "r1", 100, 50);
        let r = parse_line(&line, "host", "macos");
        assert!(r.is_some());
        let (key, hb) = r.unwrap();
        assert_eq!(key, "m1:r1");
        assert_eq!(hb.input_tokens, 100);
    }

    #[test]
    fn parse_line_skips_zero_tokens() {
        let line = assistant_json("m1", "r1", 0, 0);
        assert!(parse_line(&line, "host", "macos").is_none());
    }

    #[test]
    fn parse_line_skips_non_assistant() {
        assert!(parse_line(r#"{"type":"user"}"#, "h", "m").is_none());
    }

    fn write_temp_jsonl(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{}", l).unwrap();
        }
        f
    }

    #[test]
    fn incremental_reads_new_lines_only() {
        let l1 = assistant_json("m1", "r1", 10, 5);
        let l2 = assistant_json("m2", "r2", 20, 10);
        let f = write_temp_jsonl(&[&l1, &l2]);

        let (hbs, off1) = parse_jsonl_incremental(f.path(), 0, TEST_MACHINE_ID).unwrap();
        assert_eq!(hbs.len(), 2);

        let (hbs2, _) = parse_jsonl_incremental(f.path(), off1, TEST_MACHINE_ID).unwrap();
        assert_eq!(hbs2.len(), 0);
    }

    #[test]
    fn incremental_deduplicates() {
        let l = assistant_json("m1", "r1", 10, 5);
        let f = write_temp_jsonl(&[&l, &l]);
        let (hbs, _) = parse_jsonl_incremental(f.path(), 0, TEST_MACHINE_ID).unwrap();
        assert_eq!(hbs.len(), 1);
    }

    #[test]
    fn incremental_keeps_partial_final_line_uncommitted() {
        let complete = assistant_json("m1", "r1", 10, 5);
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "{}", complete).unwrap();
        let complete_offset = f.as_file().metadata().unwrap().len();
        write!(f, r#"{{"type":"assistant""#).unwrap();

        let (hbs, offset) = parse_jsonl_incremental(f.path(), 0, TEST_MACHINE_ID).unwrap();

        assert_eq!(hbs.len(), 1);
        assert_eq!(offset, complete_offset);
    }

    #[test]
    fn claude_collector_name() {
        assert_eq!(ClaudeCollector::new().name(), "claude-code");
    }

    #[test]
    fn real_jsonl_parses_without_errors() {
        let claude_dir = dirs::home_dir().unwrap().join(".claude").join("projects");
        if !claude_dir.exists() {
            return;
        }

        let files = find_jsonl_files(&claude_dir);
        let mut total = 0usize;
        for file in &files {
            let (hbs, off) = parse_jsonl_incremental(file, 0, TEST_MACHINE_ID).unwrap();
            assert!(off > 0 || fs::metadata(file).unwrap().len() == 0);
            for hb in &hbs {
                assert!(hb.event_id != ":");
                assert!(hb.event_ts > 0);
            }
            total += hbs.len();
        }
        eprintln!("parsed {} files, {total} heartbeats", files.len());
    }
}
