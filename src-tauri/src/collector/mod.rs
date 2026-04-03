pub mod claude;

use crate::heartbeat::Heartbeat;
use std::path::{Path, PathBuf};

/// One session file's parse result.
pub struct SessionFile {
    pub path: PathBuf,
    pub offset: u64,
    pub heartbeats: Vec<Heartbeat>,
}

pub trait Collector: Send + Sync {
    fn name(&self) -> &str;
    /// Scan and return session files with their heartbeats.
    fn collect(&self) -> Result<Vec<SessionFile>, String>;
    /// Commit offset for a single file after its heartbeats are uploaded.
    fn commit_file(&self, path: &Path, offset: u64);
}

pub fn create_collectors() -> Vec<Box<dyn Collector>> {
    vec![Box::new(claude::ClaudeCollector::new())]
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
