//! Drives the production Rust root and the real static-session index headlessly.
use nsh_core::{
    element::Root,
    geometry::{px, size},
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_claude(root: &Path) {
    let project = root.join("-tmp-project-x");
    fs::create_dir_all(&project).unwrap();
    let lines = [
        serde_json::json!({"type":"user","sessionId":"s1","cwd":"/tmp/project-x","timestamp":"2026-01-02T03:04:05Z","message":{"role":"user","content":"fix the tests"}}),
        serde_json::json!({"type":"assistant","sessionId":"s1","cwd":"/tmp/project-x","timestamp":"2026-01-02T03:04:06Z","message":{"model":"claude-sonnet-4-20250514","usage":{"input_tokens":1200,"output_tokens":3400,"cache_read_input_tokens":9000,"cache_creation_input_tokens":500},"content":[{"type":"text","text":"done"}]}}),
    ]
    .iter()
    .map(|line| line.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    fs::write(project.join("s1.jsonl"), lines).unwrap();
}

fn write_opencode(db: &Path) {
    let connection = rusqlite::Connection::open(db).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE session(
                id TEXT PRIMARY KEY, title TEXT, directory TEXT,
                time_created INTEGER, time_updated INTEGER, cost REAL,
                tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER,
                tokens_cache_read INTEGER, tokens_cache_write INTEGER, model TEXT
             );
             CREATE TABLE message(id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![
                "ses_1",
                "OpenCode fixture session",
                "/tmp/oc-project",
                1_795_000_000_000_i64,
                1_795_000_100_000_i64,
                0.25_f64,
                2000_i64,
                800_i64,
                100_i64,
                500_i64,
                0_i64,
                "{\"id\":\"claude-sonnet-4\",\"providerID\":\"anthropic\"}",
            ],
        )
        .unwrap();
    for index in 0..3 {
        connection
            .execute(
                "INSERT INTO message VALUES(?1,?2,?3,'{}')",
                rusqlite::params![
                    format!("msg_{index}"),
                    "ses_1",
                    1_795_000_000_000_i64 + index
                ],
            )
            .unwrap();
    }
}

#[test]
fn rust_ui_scans_and_drills_into_sessions() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".nsh/test-data")
        .join(format!(
            "sessiontree-rust-ui-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    let fixture = Fixture(dir.clone());
    fs::create_dir_all(&dir).unwrap();
    write_claude(&dir.join("claude"));
    write_opencode(&dir.join("opencode.db"));
    unsafe {
        std::env::set_var("SESSIONTREE_CLAUDE_ROOT", dir.join("claude"));
        std::env::set_var("SESSIONTREE_CODEX_ROOT", dir.join("no-codex"));
        std::env::set_var("SESSIONTREE_PI_ROOT", dir.join("no-pi"));
        std::env::set_var("SESSIONTREE_OPENCODE_DB", dir.join("opencode.db"));
        std::env::set_var("SESSIONTREE_CURSOR_DB", dir.join("no-cursor.db"));
        std::env::set_var("SESSIONTREE_COPILOT_DB", dir.join("no-copilot.db"));
        std::env::set_var("SESSIONTREE_DSH_ROOT", dir.join("no-dsh"));
        std::env::set_var("SESSIONTREE_ZCODE_ROOT", dir.join("no-zcode"));
        std::env::set_var("SESSIONTREE_ZCODE_LEGACY_ROOT", dir.join("no-zcode-legacy"));
        std::env::set_var("SESSIONTREE_ZCODE_DB", dir.join("no-zcode.db"));
        std::env::set_var("SESSIONTREE_MIMO_DB", dir.join("no-mimo.db"));
        std::env::set_var("SESSIONTREE_ANTIGRAVITY_ROOT", dir.join("no-antigravity"));
        std::env::set_var("SESSIONTREE_KIRO_ROOT", dir.join("no-kiro"));
        std::env::set_var("SESSIONTREE_CACHE", dir.join("cache.json"));
    }

    let mut ui = Headless::start(EngineConfig {
        viewport: size(px(1280.), px(850.)),
        ..EngineConfig::new(Root::of::<sessiontree::App>())
    })
    .unwrap();

    // Root lists the agents found on disk.
    ui.wait_for("label:Open Claude Code").unwrap();
    ui.wait_for("label:Open OpenCode").unwrap();
    ui.wait_for("text:Claude Code · 1").unwrap();

    // Agent -> project -> session.
    ui.press("label:Open Claude Code").unwrap();
    ui.wait_for("label:Open project-x").unwrap();
    ui.press("label:Open project-x").unwrap();
    ui.wait_for("label:Open fix the tests").unwrap();
    ui.press("label:Open fix the tests").unwrap();
    ui.wait_for("text:MESSAGES").unwrap();
    ui.wait_for("text:claude-sonnet-4-20250514").unwrap();

    // The metric switch changes the map without losing the selection.
    ui.press("label:Cost").unwrap();
    ui.wait_for("text:Cost").unwrap();
    ui.press("label:Tokens").unwrap();

    // The share sheet opens, takes its redaction toggles and closes.
    ui.press("label:Share image").unwrap();
    ui.wait_for("text:Share screenshot").unwrap();
    ui.press("label:Agent names").unwrap();
    ui.press("label:Project names").unwrap();
    ui.press("label:Cancel").unwrap();

    // The compare sheet puts every agent side by side.
    ui.press("label:Compare agents").unwrap();
    ui.wait_for("text:TOTAL").unwrap();
    ui.press("label:Close compare").unwrap();

    // Global search reaches every project and session.
    ui.type_text("id:global-search", "project-x").unwrap();
    ui.wait_for("text:SEARCH").unwrap();
    ui.press("label:Clear search").unwrap();

    // A selected project shows its sessions in the right panel, searchable.
    ui.press("label:Go to All sessions").unwrap();
    ui.press("label:Open Claude Code").unwrap();
    ui.wait_for("label:Open project-x").unwrap();
    ui.press("label:project-x").unwrap();
    ui.wait_for("text:Search this project").unwrap();
    ui.type_text("id:project-search", "fix").unwrap();
    ui.wait_for("text:fix the tests").unwrap();

    // The time range filters the whole tree and restores it from All.
    ui.press("label:Last 7 days").unwrap();
    ui.wait_for("text:Last 7 days · 1 sessions · 3 messages · 2.9K tokens · 500 cache · $0.250")
        .unwrap();
    ui.press("label:All time").unwrap();
    ui.wait_for("label:Open Claude Code").unwrap();

    // A custom range includes the OpenCode fixture and excludes Claude.
    ui.press("label:Custom range").unwrap();
    ui.wait_for("text:Sessions are included by their last activity, in local time.")
        .unwrap();
    ui.press("label:Apply range").unwrap();
    ui.wait_for("text:Custom range · 1 sessions · 3 messages · 2.9K tokens · 500 cache · $0.250")
        .unwrap();
    ui.press("label:All time").unwrap();
    ui.wait_for("label:Open Claude Code").unwrap();

    drop(fixture);
}
