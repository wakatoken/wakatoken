use crate::collector::SessionFile;
use crate::heartbeat::Heartbeat;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

const DB_FILE: &str = "wakatoken.db";
const STATUS_LOCAL: &str = "local";
const STATUS_UPLOADED: &str = "uploaded";
const STATUS_FAILED: &str = "failed";

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

#[derive(Debug, Clone)]
pub struct PendingHeartbeat {
    pub event_id: String,
    pub heartbeat: Heartbeat,
}

pub fn get_local_dashboard() -> Result<LocalDashboard, String> {
    let conn = open_db()?;
    let sessions = query_sessions(&conn, None, None)?;
    Ok(build_dashboard(&sessions))
}

pub fn has_store() -> Result<bool, String> {
    if !db_path()?.exists() {
        return Ok(false);
    }
    let conn = open_db()?;
    has_events(&conn)
}

pub fn list_sessions(
    runtime: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<SessionSummary>, String> {
    let conn = open_db()?;
    query_sessions(&conn, runtime, limit)
}

pub fn replace_runtime_sessions(
    runtimes: &[String],
    sessions: &[SessionFile],
) -> Result<LocalDashboard, String> {
    let mut conn = open_db()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    let mut seen = HashSet::new();
    for session in sessions {
        update_file_state(&tx, session)?;
        for heartbeat in &session.heartbeats {
            seen.insert(heartbeat.event_id.clone());
            upsert_event(&tx, session, heartbeat)?;
        }
    }

    for runtime in runtimes {
        let mut stmt = tx
            .prepare("SELECT event_id FROM events WHERE runtime = ?1")
            .map_err(|e| e.to_string())?;
        let event_ids = stmt
            .query_map(params![runtime], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);

        for event_id in event_ids {
            if !seen.contains(&event_id) {
                tx.execute("DELETE FROM events WHERE event_id = ?1", params![event_id])
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    tx.commit().map_err(|e| e.to_string())?;
    get_local_dashboard()
}

pub fn upsert_sessions(sessions: &[SessionFile]) -> Result<(), String> {
    let mut conn = open_db()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for session in sessions {
        update_file_state(&tx, session)?;
        for heartbeat in &session.heartbeats {
            upsert_event(&tx, session, heartbeat)?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn file_offsets(runtime: &str) -> Result<HashMap<PathBuf, u64>, String> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare("SELECT path, offset FROM file_scan_state WHERE runtime = ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![runtime], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                row.get::<_, i64>(1)? as u64,
            ))
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(|e| e.to_string())
}

pub fn pending_heartbeats(limit: usize) -> Result<Vec<PendingHeartbeat>, String> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT event_id, project, provider, model, runtime, os, machine_id, git_branch, language, tool,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, event_ts
             FROM events
             WHERE upload_status != ?1
             ORDER BY event_ts ASC
             LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![STATUS_UPLOADED, limit as i64], |row| {
            let event_id: String = row.get(0)?;
            Ok(PendingHeartbeat {
                event_id: event_id.clone(),
                heartbeat: Heartbeat {
                    event_id,
                    project: row.get(1)?,
                    provider: row.get(2)?,
                    model: row.get(3)?,
                    source: row.get(4)?,
                    os: row.get(5)?,
                    machine_id: row.get(6)?,
                    git_branch: row.get(7)?,
                    language: row.get(8)?,
                    tool: row.get(9)?,
                    input_tokens: row.get::<_, i64>(10)? as u64,
                    output_tokens: row.get::<_, i64>(11)? as u64,
                    cache_read_tokens: row.get::<_, i64>(12)? as u64,
                    cache_write_tokens: row.get::<_, i64>(13)? as u64,
                    event_ts: row.get(14)?,
                },
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn mark_uploaded(event_ids: &[String]) -> Result<(), String> {
    update_status(event_ids, STATUS_UPLOADED, "")
}

pub fn mark_failed(event_ids: &[String], error: &str) -> Result<(), String> {
    update_status(event_ids, STATUS_FAILED, error)
}

fn update_status(event_ids: &[String], status: &str, last_error: &str) -> Result<(), String> {
    if event_ids.is_empty() {
        return Ok(());
    }
    let mut conn = open_db()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp_millis();
    for event_id in event_ids {
        tx.execute(
            "UPDATE events
             SET upload_status = ?1, last_error = ?2, uploaded_at = CASE WHEN ?1 = ?3 THEN ?4 ELSE uploaded_at END
             WHERE event_id = ?5",
            params![status, last_error, STATUS_UPLOADED, now, event_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

fn upsert_event(
    conn: &Connection,
    session: &SessionFile,
    heartbeat: &Heartbeat,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO events (
            event_id, session_path, runtime, project, provider, model, os, machine_id, git_branch,
            language, tool, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
            event_ts, upload_status, last_error, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, '', ?18, ?18)
        ON CONFLICT(event_id) DO UPDATE SET
            session_path = excluded.session_path,
            runtime = excluded.runtime,
            project = excluded.project,
            provider = excluded.provider,
            model = excluded.model,
            os = excluded.os,
            machine_id = excluded.machine_id,
            git_branch = excluded.git_branch,
            language = excluded.language,
            tool = excluded.tool,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            cache_write_tokens = excluded.cache_write_tokens,
            event_ts = excluded.event_ts,
            updated_at = excluded.updated_at",
        params![
            heartbeat.event_id,
            session.path.display().to_string(),
            session.runtime,
            heartbeat.project,
            heartbeat.provider,
            heartbeat.model,
            heartbeat.os,
            heartbeat.machine_id,
            heartbeat.git_branch,
            heartbeat.language,
            heartbeat.tool,
            heartbeat.input_tokens as i64,
            heartbeat.output_tokens as i64,
            heartbeat.cache_read_tokens as i64,
            heartbeat.cache_write_tokens as i64,
            heartbeat.event_ts,
            STATUS_LOCAL,
            chrono::Utc::now().timestamp_millis(),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn update_file_state(conn: &Connection, session: &SessionFile) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO file_scan_state (runtime, path, offset, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(runtime, path) DO UPDATE SET
            offset = excluded.offset,
            updated_at = excluded.updated_at",
        params![
            session.runtime,
            session.path.display().to_string(),
            session.offset as i64,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn query_sessions(
    conn: &Connection,
    runtime: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<SessionSummary>, String> {
    let mut sql = String::from(
        "SELECT session_path, runtime,
                MIN(event_ts), MAX(event_ts),
                SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens),
                COUNT(*),
                COALESCE((SELECT project FROM events e2 WHERE e2.session_path = events.session_path ORDER BY event_ts ASC LIMIT 1), ''),
                COALESCE((SELECT model FROM events e2 WHERE e2.session_path = events.session_path ORDER BY event_ts ASC LIMIT 1), ''),
                CASE
                    WHEN SUM(CASE WHEN upload_status = 'failed' THEN 1 ELSE 0 END) > 0 THEN 'failed'
                    WHEN SUM(CASE WHEN upload_status != 'uploaded' THEN 1 ELSE 0 END) = 0 THEN 'synced'
                    ELSE 'local'
                END,
                COALESCE(MAX(NULLIF(last_error, '')), '')
         FROM events",
    );

    let mut params_vec: Vec<String> = Vec::new();
    if let Some(runtime) = runtime.filter(|value| !value.is_empty() && value != "all") {
        sql.push_str(" WHERE runtime = ?1");
        params_vec.push(runtime);
    }
    sql.push_str(" GROUP BY session_path, runtime ORDER BY MAX(event_ts) DESC");
    if limit.is_some() {
        sql.push_str(&format!(" LIMIT {}", limit.unwrap()));
    }

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = if params_vec.is_empty() {
        stmt.query_map([], session_from_row)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
    } else {
        stmt.query_map(params![params_vec[0].clone()], session_from_row)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
    };
    rows.map_err(|e| e.to_string())
}

fn session_from_row(row: &rusqlite::Row) -> rusqlite::Result<SessionSummary> {
    let input_tokens = row.get::<_, i64>(4)? as u64;
    let output_tokens = row.get::<_, i64>(5)? as u64;
    let cache_read_tokens = row.get::<_, i64>(6)? as u64;
    let cache_write_tokens = row.get::<_, i64>(7)? as u64;
    Ok(SessionSummary {
        id: row.get(0)?,
        path: row.get(0)?,
        runtime: row.get(1)?,
        started_at: row.get(2)?,
        ended_at: row.get(3)?,
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
        event_count: row.get::<_, i64>(8)? as u64,
        project: row.get(9)?,
        model: row.get(10)?,
        status: row.get(11)?,
        last_error: row.get(12)?,
    })
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

fn open_db() -> Result<Connection, String> {
    let path = db_path()?;
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS events (
            event_id TEXT PRIMARY KEY,
            session_path TEXT NOT NULL,
            runtime TEXT NOT NULL,
            project TEXT NOT NULL,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            os TEXT NOT NULL,
            machine_id TEXT NOT NULL,
            git_branch TEXT NOT NULL,
            language TEXT NOT NULL,
            tool TEXT NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cache_read_tokens INTEGER NOT NULL,
            cache_write_tokens INTEGER NOT NULL,
            event_ts INTEGER NOT NULL,
            upload_status TEXT NOT NULL,
            last_error TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            uploaded_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_events_runtime ON events(runtime);
        CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_path);
        CREATE INDEX IF NOT EXISTS idx_events_upload ON events(upload_status, event_ts);
        CREATE TABLE IF NOT EXISTS file_scan_state (
            runtime TEXT NOT NULL,
            path TEXT NOT NULL,
            offset INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (runtime, path)
        );",
    )
    .map_err(|e| e.to_string())
}

fn has_events(conn: &Connection) -> Result<bool, String> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

fn db_path() -> Result<PathBuf, String> {
    let dir = dirs::config_dir()
        .ok_or("cannot find config directory")?
        .join("com.wakatoken.client");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(DB_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn token_total_counts_cache_tokens() {
        assert_eq!(token_total(10, 5, 3, 2), 20);
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
            status: "local".to_string(),
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

    #[test]
    fn upsert_preserves_uploaded_status() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let session = SessionFile {
            runtime: "claude-code".to_string(),
            path: PathBuf::from("/tmp/session.jsonl"),
            offset: 10,
            heartbeats: vec![heartbeat("e1", "claude-code", 10, 3)],
        };
        upsert_event(&conn, &session, &session.heartbeats[0]).unwrap();
        update_status_on_conn(&conn, &["e1".to_string()], STATUS_UPLOADED, "").unwrap();
        upsert_event(&conn, &session, &session.heartbeats[0]).unwrap();

        let status: String = conn
            .query_row(
                "SELECT upload_status FROM events WHERE event_id = 'e1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, STATUS_UPLOADED);
    }

    #[test]
    fn empty_database_is_not_a_local_store() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        assert!(!has_events(&conn).unwrap());
    }

    #[test]
    fn database_with_events_is_a_local_store() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let session = SessionFile {
            runtime: "claude-code".to_string(),
            path: PathBuf::from("/tmp/session.jsonl"),
            offset: 10,
            heartbeats: vec![heartbeat("e1", "claude-code", 10, 3)],
        };
        upsert_event(&conn, &session, &session.heartbeats[0]).unwrap();

        assert!(has_events(&conn).unwrap());
    }

    fn update_status_on_conn(
        conn: &Connection,
        event_ids: &[String],
        status: &str,
        last_error: &str,
    ) -> Result<(), String> {
        for event_id in event_ids {
            conn.execute(
                "UPDATE events SET upload_status = ?1, last_error = ?2 WHERE event_id = ?3",
                params![status, last_error, event_id],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
