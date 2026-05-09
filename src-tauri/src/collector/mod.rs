pub mod claude;
pub mod codex;
pub mod copilot;
pub mod gemini;

use crate::heartbeat::Heartbeat;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// One session file's parse result.
pub struct SessionFile {
    pub runtime: String,
    pub path: PathBuf,
    pub offset: u64,
    pub heartbeats: Vec<Heartbeat>,
}

pub trait Collector: Send + Sync {
    fn name(&self) -> &str;
    /// Scan and return session files with their heartbeats.
    fn collect(&self, machine_id: &str) -> Result<Vec<SessionFile>, String>;
    fn scan_all(&self, machine_id: &str) -> Result<Vec<SessionFile>, String>;
    fn scan_all_with_progress(
        &self,
        machine_id: &str,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Vec<SessionFile>, String> {
        let sessions = self.scan_all(machine_id)?;
        progress(1, 1);
        Ok(sessions)
    }
    fn scan_since(
        &self,
        machine_id: &str,
        offsets: &HashMap<PathBuf, u64>,
    ) -> Result<Vec<SessionFile>, String> {
        let _ = offsets;
        self.scan_all(machine_id)
    }
    /// Commit offset for a single file after its heartbeats are uploaded.
    fn commit_file(&self, path: &Path, offset: u64);
}

pub fn create_collectors() -> Vec<Box<dyn Collector>> {
    vec![
        Box::new(claude::ClaudeCollector::new()),
        Box::new(codex::CodexCollector::new()),
        Box::new(copilot::CopilotCollector::new()),
        Box::new(gemini::GeminiCollector::new()),
    ]
}

pub(crate) fn project_name_from_cwd(cwd: &str) -> String {
    if cwd.is_empty() {
        return "unknown".to_string();
    }

    let path = Path::new(cwd);
    git_worktree_project(path).unwrap_or_else(|| path_project_name(path))
}

pub(crate) fn project_name_from_repository_text(text: &str) -> Option<String> {
    let marker = "Repository:";
    let start = text.find(marker)? + marker.len();
    let raw = first_repository_line(&text[start..]).trim();
    let repo = raw.trim_matches(|c: char| c == '`' || c == '"' || c == '\'');
    let repo_path = repo
        .split_whitespace()
        .next()
        .unwrap_or(repo)
        .trim_end_matches(|c: char| c == ',' || c == ';' || c == ')');
    let project = repo_path
        .trim_end_matches(".git")
        .trim_end_matches('/')
        .rsplit('/')
        .next()?;

    if project.is_empty() {
        None
    } else {
        Some(project.to_string())
    }
}

fn first_repository_line(text: &str) -> &str {
    ["\n", "\\n", "\r", "\\r"]
        .iter()
        .filter_map(|delimiter| text.find(delimiter))
        .min()
        .map(|end| &text[..end])
        .unwrap_or(text)
}

fn git_worktree_project(path: &Path) -> Option<String> {
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        return path
            .file_name()
            .map(|name| name.to_string_lossy().to_string());
    }

    let content = fs::read_to_string(dot_git).ok()?;
    let git_dir = content.trim().strip_prefix("gitdir:")?.trim();
    let git_dir = Path::new(git_dir);
    let common_git_dir = git_dir.parent()?.parent()?;
    if common_git_dir.file_name()? != ".git" {
        return None;
    }
    common_git_dir
        .parent()?
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
}

fn path_project_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_collectors_returns_at_least_one_collector() {
        let collectors = create_collectors();
        assert!(!collectors.is_empty());
    }

    #[test]
    fn create_collectors_first_entry_is_claude_code() {
        let collectors = create_collectors();
        assert_eq!(collectors[0].name(), "claude-code");
    }

    #[test]
    fn create_collectors_contains_codex_cli() {
        let collectors = create_collectors();
        assert!(collectors.iter().any(|c| c.name() == "codex-cli"));
    }

    #[test]
    fn create_collectors_contains_copilot_agent() {
        let collectors = create_collectors();
        assert!(collectors.iter().any(|c| c.name() == "copilot-agent"));
    }

    #[test]
    fn create_collectors_contains_gemini_cli() {
        let collectors = create_collectors();
        assert!(collectors.iter().any(|c| c.name() == "gemini-cli"));
    }

    #[test]
    fn project_name_from_cwd_returns_last_path_component() {
        assert_eq!(project_name_from_cwd("/home/user/wakatoken"), "wakatoken");
    }

    #[test]
    fn project_name_from_cwd_reads_git_worktree_parent_repo() {
        let dir = tempdir().unwrap();
        let repo = dir.path().join("repos/github.com/saltbo/zpan");
        let worktree = dir.path().join("worktrees/9c908343");
        let git_dir = repo.join(".git/worktrees/9c908343");
        fs::create_dir_all(&git_dir).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .unwrap();

        assert_eq!(project_name_from_cwd(worktree.to_str().unwrap()), "zpan");
    }

    #[test]
    fn project_name_from_repository_text_returns_repo_name() {
        let text = "Priority: high\nRepository: https://github.com/saltbo/zpan\nBoard: 1";
        assert_eq!(
            project_name_from_repository_text(text),
            Some("zpan".to_string())
        );
    }

    #[test]
    fn project_name_from_repository_text_handles_json_escaped_newline() {
        let text = r#"{"text":"Repository: https://github.com/saltbo/zpan\nBoard: 1"}"#;
        assert_eq!(
            project_name_from_repository_text(text),
            Some("zpan".to_string())
        );
    }
}
