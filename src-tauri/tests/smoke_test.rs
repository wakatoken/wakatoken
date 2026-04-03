/// Smoke test: collect real session files and upload.
/// Run with: cargo test --test smoke_test -- --nocapture

#[test]
fn collect_and_report_smoke() {
    let collectors = wakatoken_client_lib::collector::create_collectors();
    let mut all_sessions = Vec::new();
    for c in &collectors {
        match c.collect() {
            Ok(sessions) => all_sessions.extend(sessions),
            Err(e) => panic!("collector error: {e}"),
        }
    }
    let msg_count: usize = all_sessions.iter().map(|s| s.heartbeats.len()).sum();
    eprintln!("Found {} session files, {} messages", all_sessions.len(), msg_count);

    if all_sessions.is_empty() {
        eprintln!("Nothing to upload");
        return;
    }

    let config = wakatoken_client_lib::config::AppConfig::load();
    if config.api_key.is_empty() {
        eprintln!("SKIP upload: no API key configured.");
        return;
    }

    // Upload first 3 files only for smoke test
    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = reqwest::Client::new();
    for (i, session) in all_sessions.iter().take(3).enumerate() {
        let n = session.heartbeats.len();
        eprintln!("Uploading file {}/{} ({n} msgs)...", i + 1, all_sessions.len().min(3));
        let result = rt.block_on(wakatoken_client_lib::reporter::send_heartbeats(
            &client, &config.api_key, session.heartbeats.clone(),
        ));
        match result {
            Ok(r) => {
                eprintln!("  {} new, {} dedup", r.new_count, r.dedup_count);
                for c in &collectors {
                    c.commit_file(&session.path, session.offset);
                }
            }
            Err(e) => panic!("UPLOAD FAILED: {e}"),
        }
    }
}
