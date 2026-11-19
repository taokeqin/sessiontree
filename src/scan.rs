//! Static session history from the agents' own logs and databases.
//!
//! This follows aishark's passive import: the history that Claude Code, Codex,
//! Pi, OpenCode, Cursor, Copilot, DeepSeek Harness, ZCode, MiMo Code,
//! Antigravity and Kiro already keep on disk is read where it lives.
//! Nothing is configured, proxied, or modified. A small cache keyed by file
//! metadata keeps relaunches fast without re-reading gigabytes of unchanged
//! transcripts.

use crate::model::*;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SCAN_VERSION: u32 = 3;

const PHASE_FILES: u8 = 0;
const PHASE_OPENCODE: u8 = 1;
const PHASE_CURSOR: u8 = 2;
const PHASE_COPILOT: u8 = 3;
const PHASE_ZCODE: u8 = 4;
const PHASE_MIMO: u8 = 5;
const PHASE_TREE: u8 = 6;
const MAX_ERRORS: usize = 24;

type FileParser = fn(&Path, i64) -> Result<Option<Session>, String>;

static DONE: AtomicU64 = AtomicU64::new(0);
static TOTAL: AtomicU64 = AtomicU64::new(0);
static SESSIONS: AtomicU64 = AtomicU64::new(0);
static PHASE: AtomicU8 = AtomicU8::new(0);

pub(crate) fn progress() -> Progress {
    let phase = match PHASE.load(Ordering::Relaxed) {
        PHASE_FILES => "Reading local transcripts",
        PHASE_OPENCODE => "Reading the OpenCode database",
        PHASE_CURSOR => "Reading the Cursor database",
        PHASE_COPILOT => "Reading the Copilot database",
        PHASE_ZCODE => "Reading the ZCode database",
        PHASE_MIMO => "Reading the MiMo database",
        _ => "Building the tree",
    };
    Progress {
        done: DONE.load(Ordering::Relaxed),
        total: TOTAL.load(Ordering::Relaxed),
        sessions: SESSIONS.load(Ordering::Relaxed),
        phase: phase.into(),
    }
}

pub(crate) fn scan() -> Snapshot {
    PHASE.store(PHASE_FILES, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    TOTAL.store(0, Ordering::Relaxed);
    SESSIONS.store(0, Ordering::Relaxed);

    let started = Instant::now();
    let mut errors = Vec::new();
    let mut cache = Cache::load();
    let mut sessions = Vec::new();
    let mut cache_hits = 0_u64;

    let sources = [claude_root(), codex_root(), pi_root()];
    let parsers: [FileParser; 3] = [parse_claude_file, parse_codex_file, parse_pi_file];
    let mut jobs = Vec::new();
    for (root, parse) in sources.into_iter().zip(parsers) {
        let Some(root) = root.filter(|root| root.is_dir()) else {
            continue;
        };
        for path in jsonl_files(&root, &mut errors) {
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            jobs.push((path, modified_millis(&metadata), metadata.len(), parse));
        }
    }
    if let Some(root) = dsh_root().filter(|root| root.is_dir()) {
        for path in dsh_files(&root, &mut errors) {
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            jobs.push((
                path,
                modified_millis(&metadata),
                metadata.len(),
                parse_dsh_file as FileParser,
            ));
        }
    }
    let extra: [(Vec<PathBuf>, FileParser); 5] = [
        (zcode_transcript_files(&mut errors), parse_zcode_file),
        (zcode_legacy_files(&mut errors), parse_zcode_legacy_file),
        (antigravity_files(&mut errors), parse_antigravity_file),
        (kiro_cli_files(&mut errors), parse_kiro_cli_file),
        (kiro_ide_files(&mut errors), parse_kiro_ide_file),
    ];
    for (paths, parse) in extra {
        for path in paths {
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            jobs.push((path, modified_millis(&metadata), metadata.len(), parse));
        }
    }
    TOTAL.store(jobs.len() as u64, Ordering::Relaxed);
    let mut fresh = BTreeMap::new();
    let mut pending = Vec::new();
    for (path, mtime, size, parse) in jobs {
        let key = path.to_string_lossy().to_string();
        if let Some(hit) = cache
            .files
            .get(&key)
            .filter(|hit| hit.mtime == mtime && hit.size == size)
        {
            cache_hits += 1;
            sessions.push(hit.session.clone());
            fresh.insert(key, hit.clone());
            DONE.fetch_add(1, Ordering::Relaxed);
        } else {
            pending.push((path, mtime, size, parse));
        }
    }
    SESSIONS.store(sessions.len() as u64, Ordering::Relaxed);

    // The first run reads every transcript; spread that work over a few
    // threads. Later runs mostly hit the metadata cache above.
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(1, 8)
        .min(pending.len().max(1));
    let next = AtomicUsize::new(0);
    let parsed = if pending.is_empty() {
        Vec::new()
    } else {
        std::thread::scope(|scope| {
            let pending = &pending;
            let next = &next;
            let handles: Vec<_> = (0..threads)
                .map(|_| {
                    scope.spawn(move || {
                        let mut found = Vec::new();
                        loop {
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            let Some((file, mtime, size, parse)) = pending.get(index) else {
                                break;
                            };
                            let result = parse(file, *mtime);
                            if let Ok(Some(session)) = &result
                                && (session.messages > 0 || session.input + session.output > 0)
                            {
                                SESSIONS.fetch_add(1, Ordering::Relaxed);
                            }
                            found.push((file.clone(), *mtime, *size, result));
                            DONE.fetch_add(1, Ordering::Relaxed);
                        }
                        found
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap_or_default())
                .collect::<Vec<_>>()
        })
    };
    for (file, mtime, size, result) in parsed {
        let path = file.to_string_lossy().to_string();
        match result {
            Ok(Some(session)) if session.messages > 0 || session.input + session.output > 0 => {
                sessions.push(session.clone());
                fresh.insert(
                    path,
                    CachedSession {
                        mtime,
                        size,
                        session,
                    },
                );
            }
            Ok(_) => {}
            Err(error) => push_error(&mut errors, error),
        }
    }
    DONE.store(TOTAL.load(Ordering::Relaxed), Ordering::Relaxed);
    SESSIONS.store(sessions.len() as u64, Ordering::Relaxed);
    cache.files = fresh;

    PHASE.store(PHASE_OPENCODE, Ordering::Relaxed);
    TOTAL.store(1, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    let (found, hits) = cached_db(
        &mut cache.opencode,
        opencode_db(),
        parse_opencode,
        &mut errors,
    );
    cache_hits += hits;
    sessions.extend(found);
    DONE.store(1, Ordering::Relaxed);
    SESSIONS.store(sessions.len() as u64, Ordering::Relaxed);

    PHASE.store(PHASE_CURSOR, Ordering::Relaxed);
    TOTAL.store(1, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    let (found, hits) = cached_db(&mut cache.cursor, cursor_db(), parse_cursor, &mut errors);
    cache_hits += hits;
    sessions.extend(found);
    DONE.store(1, Ordering::Relaxed);
    SESSIONS.store(sessions.len() as u64, Ordering::Relaxed);

    PHASE.store(PHASE_COPILOT, Ordering::Relaxed);
    TOTAL.store(1, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    let (found, hits) = cached_db(&mut cache.copilot, copilot_db(), parse_copilot, &mut errors);
    cache_hits += hits;
    sessions.extend(found);
    DONE.store(1, Ordering::Relaxed);
    SESSIONS.store(sessions.len() as u64, Ordering::Relaxed);

    PHASE.store(PHASE_ZCODE, Ordering::Relaxed);
    TOTAL.store(1, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    let (found, hits) = cached_db(&mut cache.zcode, zcode_db(), parse_zcode_db, &mut errors);
    cache_hits += hits;
    sessions.extend(found);
    DONE.store(1, Ordering::Relaxed);
    SESSIONS.store(sessions.len() as u64, Ordering::Relaxed);

    PHASE.store(PHASE_MIMO, Ordering::Relaxed);
    TOTAL.store(1, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    let (found, hits) = cached_db(&mut cache.mimo, mimocode_db(), parse_mimo, &mut errors);
    cache_hits += hits;
    sessions.extend(found);
    DONE.store(1, Ordering::Relaxed);
    SESSIONS.store(sessions.len() as u64, Ordering::Relaxed);

    let sessions = dedupe_sessions(sessions);

    PHASE.store(PHASE_TREE, Ordering::Relaxed);
    cache.save();
    let mut snapshot = build_tree(sessions, errors, started.elapsed().as_millis() as u64);
    snapshot.cache_hits = cache_hits;
    snapshot
}

pub(crate) fn report() -> Result<String, String> {
    let snapshot = scan();
    let mut out = String::new();
    out.push_str(&format!(
        "Sessiontree static index\n  sessions:  {}\n  messages:  {}\n  tokens:    {} in / {} out / {} cache\n  cost:      {}\n  scanned:   {} ms ({} cached files)\n",
        count(snapshot.root.sessions),
        count(snapshot.root.messages),
        compact(snapshot.root.input),
        compact(snapshot.root.output),
        compact(snapshot.root.cache()),
        money(snapshot.root.cost),
        snapshot.scanned_ms,
        count(snapshot.cache_hits),
    ));
    for agent in &snapshot.root.children {
        out.push_str(&format!(
            "\n{}: {} sessions, {} messages, {} tokens, {}, {} projects\n",
            agent.name,
            count(agent.sessions),
            count(agent.messages),
            compact(agent.tokens()),
            money(agent.cost),
            count(agent.children.len() as u64),
        ));
        for project in agent.children.iter().take(8) {
            out.push_str(&format!(
                "  {:<44} {:>4} sessions  {:>8} tokens  {:>8}\n",
                truncate(&project.title, 44),
                count(project.sessions),
                compact(project.tokens()),
                money(project.cost),
            ));
        }
    }
    if !snapshot.errors.is_empty() {
        out.push_str("\nSkipped:\n");
        for error in snapshot.errors.iter().take(8) {
            out.push_str(&format!("  {error}\n"));
        }
    }
    Ok(out)
}

fn truncate(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().take(width).collect();
    let mut result: String = chars.into_iter().collect();
    if text.chars().count() > width {
        result.push('…');
    }
    result
}

fn push_error(errors: &mut Vec<String>, error: String) {
    if errors.len() < MAX_ERRORS {
        errors.push(error);
    }
}

// ---------------------------------------------------------------------------
// Claude Code: ~/.claude/projects/**/*.jsonl, one transcript per session.

fn claude_root() -> Option<PathBuf> {
    env_path("SESSIONTREE_CLAUDE_ROOT")
        .or_else(|| home_dir().map(|home| home.join(".claude").join("projects")))
}

fn jsonl_files(root: &Path, errors: &mut Vec<String>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                push_error(errors, format!("{}: {error}", directory.display()));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn parse_claude_file(path: &Path, fallback_ts: i64) -> Result<Option<Session>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("session");
    let mut session_id = stem.to_owned();
    let mut project = String::new();
    let mut title = String::new();
    let mut model = String::new();
    let mut messages = 0_u64;
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut cache_read = 0_u64;
    let mut cache_write = 0_u64;
    let mut cost = 0.0;
    let mut first_ts = 0_i64;
    let mut last_ts = 0_i64;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let kind = value.get("type").and_then(Value::as_str);
        if kind != Some("user") && kind != Some("assistant") {
            continue;
        }
        if let Some(id) = value.get("sessionId").and_then(Value::as_str) {
            session_id = id.to_owned();
        }
        if let Some(cwd) = value.get("cwd").and_then(Value::as_str)
            && !cwd.is_empty()
        {
            project = cwd.to_owned();
        }
        let ts = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .map(|time| time.timestamp_millis())
            .unwrap_or(fallback_ts);
        first_ts = earliest(first_ts, ts);
        last_ts = last_ts.max(ts);
        messages += 1;
        if kind == Some("user") {
            if title.is_empty() {
                let text = message_text(&value);
                if !text.is_empty() {
                    title = text.chars().take(160).collect();
                }
            }
            continue;
        }
        if let Some(message) = value.get("message") {
            if let Some(found) = message.get("model").and_then(Value::as_str)
                && found != "<synthetic>"
            {
                model = found.to_owned();
            }
            if let Some(usage) = message.get("usage") {
                let turn_input = number(usage.get("input_tokens")).unwrap_or(0).max(0) as u64;
                let turn_output = number(usage.get("output_tokens")).unwrap_or(0).max(0) as u64;
                let turn_read = number(usage.get("cache_read_input_tokens"))
                    .unwrap_or(0)
                    .max(0) as u64;
                let turn_write = number(usage.get("cache_creation_input_tokens"))
                    .unwrap_or(0)
                    .max(0) as u64;
                input += turn_input;
                output += turn_output;
                cache_read += turn_read;
                cache_write += turn_write;
                cost += price_for("anthropic", &model)
                    .map(|price| {
                        turn_input as f64 * price.0
                            + turn_output as f64 * price.1
                            + turn_read as f64 * price.2
                            + turn_write as f64 * price.3
                    })
                    .unwrap_or(0.0)
                    / 1_000_000.0;
            }
        }
    }
    if messages == 0 {
        return Ok(None);
    }
    if project.is_empty() {
        project = decode_project(path);
    }
    if title.is_empty() {
        title = stem.to_owned();
    }
    Ok(Some(Session {
        key: format!("claude:{session_id}"),
        tool: "Claude Code".into(),
        project,
        title,
        model,
        messages,
        input,
        output,
        cache_read,
        cache_write,
        cost,
        first_ts: if first_ts == 0 { fallback_ts } else { first_ts },
        last_ts: if last_ts == 0 { fallback_ts } else { last_ts },
        file: path.display().to_string(),
    }))
}

fn message_text(value: &Value) -> String {
    let Some(content) = value
        .get("message")
        .and_then(|message| message.get("content"))
    else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.trim().to_owned();
    }
    let Some(parts) = content.as_array() else {
        return String::new();
    };
    let mut pieces = Vec::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = part.get("text").and_then(Value::as_str)
            && !text.trim().is_empty()
        {
            pieces.push(text.trim().to_owned());
        }
    }
    pieces.join("\n")
}

fn decode_project(path: &Path) -> String {
    let Some(folder) = path.parent().and_then(|parent| parent.file_name()) else {
        return String::new();
    };
    let name = folder.to_string_lossy();
    let decoded = name.trim_start_matches('-').replace('-', "/");
    if decoded.is_empty() {
        String::new()
    } else {
        format!("/{decoded}")
    }
}

// ---------------------------------------------------------------------------
// Codex: ~/.codex/sessions/**/*.jsonl, one rollout per session.

fn codex_root() -> Option<PathBuf> {
    env_path("SESSIONTREE_CODEX_ROOT")
        .or_else(|| home_dir().map(|home| home.join(".codex").join("sessions")))
}

fn parse_codex_file(path: &Path, fallback_ts: i64) -> Result<Option<Session>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("session");
    let mut id = stem.to_owned();
    let mut project = String::new();
    let mut title = String::new();
    let mut model = String::new();
    let mut messages = 0_u64;
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut cache_read = 0_u64;
    let mut cache_write = 0_u64;
    let mut cost = 0.0;
    let mut first_ts = 0_i64;
    let mut last_ts = 0_i64;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let ts = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(rfc3339_millis)
            .unwrap_or(fallback_ts);
        first_ts = earliest(first_ts, ts);
        last_ts = last_ts.max(ts);
        let payload = value.get("payload").unwrap_or(&Value::Null);
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if let Some(found) = payload.get("id").and_then(Value::as_str) {
                    id = found.to_owned();
                }
                if let Some(cwd) = payload.get("cwd").and_then(Value::as_str)
                    && !cwd.is_empty()
                {
                    project = cwd.to_owned();
                }
            }
            Some("turn_context") => {
                if let Some(found) = payload.get("model").and_then(Value::as_str) {
                    model = found.to_owned();
                }
                if let Some(cwd) = payload.get("cwd").and_then(Value::as_str)
                    && !cwd.is_empty()
                {
                    project = cwd.to_owned();
                }
            }
            Some("response_item")
                if payload.get("type").and_then(Value::as_str) == Some("message") =>
            {
                let role = payload
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("user");
                let text = payload.get("content").map(text_value).unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }
                if role == "user" || role == "assistant" {
                    messages += 1;
                }
                if role == "user" && title.is_empty() && !codex_boilerplate(&text) {
                    title = text.trim().chars().take(160).collect();
                }
            }
            Some("event_msg") => match payload.get("type").and_then(Value::as_str) {
                Some("user_message") => {
                    if title.is_empty()
                        && let Some(text) = payload.get("message").and_then(Value::as_str)
                        && !text.trim().is_empty()
                    {
                        title = text.trim().chars().take(160).collect();
                    }
                }
                Some("token_count") => {
                    let usage = payload
                        .pointer("/info/last_token_usage")
                        .unwrap_or(&Value::Null);
                    let turn_input = number(usage.get("input_tokens")).unwrap_or(0).max(0) as u64;
                    let turn_output = (number(usage.get("output_tokens")).unwrap_or(0)
                        + number(usage.get("reasoning_output_tokens")).unwrap_or(0))
                    .max(0) as u64;
                    let turn_read =
                        number(usage.get("cached_input_tokens")).unwrap_or(0).max(0) as u64;
                    let turn_write = number(usage.get("cache_write_input_tokens"))
                        .unwrap_or(0)
                        .max(0) as u64;
                    input += turn_input;
                    output += turn_output;
                    cache_read += turn_read;
                    cache_write += turn_write;
                    cost += estimate(
                        "openai",
                        &model,
                        turn_input,
                        turn_output,
                        turn_read,
                        turn_write,
                    );
                }
                _ => {}
            },
            _ => {}
        }
    }
    if messages == 0 && input == 0 && output == 0 {
        return Ok(None);
    }
    if title.is_empty() {
        title = id.clone();
    }
    Ok(Some(Session {
        key: format!("codex:{id}"),
        tool: "Codex".into(),
        project,
        title,
        model,
        messages,
        input,
        output,
        cache_read,
        cache_write,
        cost,
        first_ts: if first_ts == 0 { fallback_ts } else { first_ts },
        last_ts: if last_ts == 0 { fallback_ts } else { last_ts },
        file: path.display().to_string(),
    }))
}

fn codex_boilerplate(text: &str) -> bool {
    text.starts_with("# AGENTS.md")
        || text.starts_with("<environment_context>")
        || text.contains("<INSTRUCTIONS>")
}

// ---------------------------------------------------------------------------
// Pi: ~/.pi/agent/sessions/**/*.jsonl.

fn pi_root() -> Option<PathBuf> {
    env_path("SESSIONTREE_PI_ROOT")
        .or_else(|| home_dir().map(|home| home.join(".pi").join("agent").join("sessions")))
}

fn pi_timestamp(value: &Value) -> i64 {
    let Some(ts) = value.get("timestamp") else {
        return 0;
    };
    if let Some(number) = number(Some(ts)) {
        return number;
    }
    ts.as_str().and_then(rfc3339_millis).unwrap_or(0)
}

fn pi_user_text(value: &Value) -> String {
    let Some(content) = value.pointer("/message/content") else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    let Some(parts) = content.as_array() else {
        return String::new();
    };
    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .map(|part| part.get("text").and_then(Value::as_str).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pi_title(text: &str) -> String {
    let mut title = text.trim();
    if let Some(end) = title.rfind("</skill>") {
        title = title[end + 8..].trim();
    }
    title
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(title)
        .trim()
        .chars()
        .take(160)
        .collect()
}

fn pi_usage(value: &Value) -> (u64, u64, u64, u64, f64) {
    let Some(usage) = value.pointer("/message/usage") else {
        return (0, 0, 0, 0, 0.0);
    };
    let input = number(usage.get("input")).unwrap_or(0).max(0) as u64;
    let output = (number(usage.get("output")).unwrap_or(0)
        + number(usage.get("reasoning")).unwrap_or(0))
    .max(0) as u64;
    let cache_read = number(usage.get("cacheRead")).unwrap_or(0).max(0) as u64;
    let cache_write = number(usage.get("cacheWrite")).unwrap_or(0).max(0) as u64;
    let cost = usage
        .get("cost")
        .and_then(|cost| cost.get("total").or(Some(cost)))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    (input, output, cache_read, cache_write, cost)
}

fn parse_pi_file(path: &Path, fallback_ts: i64) -> Result<Option<Session>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("session");
    let mut id = stem.to_owned();
    let mut project = String::new();
    let mut title = String::new();
    let mut provider = "pi".to_owned();
    let mut model = String::new();
    let mut messages = 0_u64;
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut cache_read = 0_u64;
    let mut cache_write = 0_u64;
    let mut cost = 0.0;
    let mut first_ts = 0_i64;
    let mut last_ts = 0_i64;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session") => {
                if let Some(found) = value.get("id").and_then(Value::as_str) {
                    id = found.to_owned();
                }
                if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
                    project = cwd.to_owned();
                }
                let ts = pi_timestamp(&value);
                if ts > 0 {
                    first_ts = earliest(first_ts, ts);
                    last_ts = last_ts.max(ts);
                }
            }
            Some("model_change") => {
                if let Some(found) = value.get("provider").and_then(Value::as_str) {
                    provider = found.to_owned();
                }
                if let Some(found) = value.get("modelId").and_then(Value::as_str) {
                    model = found.to_owned();
                }
            }
            Some("message") => {
                let role = value
                    .pointer("/message/role")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if role != "user" && role != "assistant" {
                    continue;
                }
                let ts = pi_timestamp(&value);
                if ts > 0 {
                    first_ts = earliest(first_ts, ts);
                    last_ts = last_ts.max(ts);
                }
                messages += 1;
                if role == "user" && title.is_empty() {
                    title = pi_title(&pi_user_text(&value));
                }
                if role == "assistant" {
                    if let Some(found) = value.pointer("/message/provider").and_then(Value::as_str)
                    {
                        provider = found.to_owned();
                    }
                    if let Some(found) = value.pointer("/message/model").and_then(Value::as_str) {
                        model = found.to_owned();
                    }
                    let (turn_input, turn_output, turn_read, turn_write, turn_cost) =
                        pi_usage(&value);
                    input += turn_input;
                    output += turn_output;
                    cache_read += turn_read;
                    cache_write += turn_write;
                    cost += turn_cost;
                }
            }
            _ => {}
        }
    }
    if messages == 0 {
        return Ok(None);
    }
    if cost == 0.0 && (input > 0 || output > 0) {
        cost = estimate(&provider, &model, input, output, cache_read, cache_write);
    }
    if title.is_empty() {
        title = id.clone();
    }
    Ok(Some(Session {
        key: format!("pi:{id}"),
        tool: "Pi".into(),
        project,
        title,
        model,
        messages,
        input,
        output,
        cache_read,
        cache_write,
        cost,
        first_ts: if first_ts == 0 { fallback_ts } else { first_ts },
        last_ts: if last_ts == 0 { fallback_ts } else { last_ts },
        file: path.display().to_string(),
    }))
}

// ---------------------------------------------------------------------------
// DeepSeek Harness: ~/.dsh/sessions/<project>/<session>/session.vN.jsonl.zstd.

fn dsh_root() -> Option<PathBuf> {
    env_path("SESSIONTREE_DSH_ROOT").or_else(|| {
        env_path("DSH_HOME")
            .map(|home| home.join("sessions"))
            .or_else(|| home_dir().map(|home| home.join(".dsh").join("sessions")))
    })
}

fn dsh_version(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix("session")?.strip_suffix(".jsonl.zstd")?;
    match rest {
        "" => Some(0),
        version => version.strip_prefix(".v")?.parse().ok(),
    }
}

fn dsh_files(root: &Path, errors: &mut Vec<String>) -> Vec<PathBuf> {
    let mut best: BTreeMap<PathBuf, (u32, PathBuf)> = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                push_error(errors, format!("{}: {error}", directory.display()));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(version) = dsh_version(&path) {
                let newer = best
                    .get(&directory)
                    .is_none_or(|(current, _)| version > *current);
                if newer {
                    best.insert(directory.clone(), (version, path));
                }
            }
        }
    }
    best.into_values().map(|(_, path)| path).collect()
}

fn dsh_title(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}

fn dsh_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let data = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if path.extension().and_then(|ext| ext.to_str()) != Some("zstd") {
        return Ok(data);
    }
    let mut input = data.as_slice();
    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    let mut output = Vec::new();
    while !input.is_empty() {
        match decoder.init(&mut input) {
            Ok(()) => {}
            Err(ruzstd::decoding::errors::FrameDecoderError::ReadFrameHeaderError(
                ruzstd::decoding::errors::ReadFrameHeaderError::SkipFrame { length, .. },
            )) => {
                input = input
                    .get(length as usize..)
                    .ok_or_else(|| format!("{}: truncated skippable frame", path.display()))?;
                continue;
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
        loop {
            decoder
                .decode_blocks(&mut input, ruzstd::decoding::BlockDecodingStrategy::All)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            loop {
                let mut chunk = [0_u8; 64 * 1024];
                let read = std::io::Read::read(&mut decoder, &mut chunk)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                if read == 0 {
                    break;
                }
                output.extend_from_slice(&chunk[..read]);
            }
            if decoder.is_finished() {
                break;
            }
        }
    }
    Ok(output)
}

fn parse_dsh_file(path: &Path, fallback_ts: i64) -> Result<Option<Session>, String> {
    let data = dsh_bytes(path)?;
    let mut id = path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("session")
        .to_owned();
    let mut project = String::new();
    let mut title = String::new();
    let mut provider = "deepseek".to_owned();
    let mut model = String::new();
    let mut messages = 0_u64;
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut cache_read = 0_u64;
    let mut first_ts = 0_i64;
    let mut last_ts = 0_i64;

    for raw in data.split(|byte| *byte == b'\n') {
        let Ok(line) = std::str::from_utf8(raw) else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let wanted = line.contains("\"type\":\"session\"")
            || line.contains("\"type\":\"user/message\"")
            || line.contains("\"type\":\"assistant/message\"")
            || line.contains("\"type\":\"session/title\"")
            || line.contains("\"type\":\"turn/end\"");
        if !wanted {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ts = number(value.get("time")).unwrap_or(0);
        if ts > 0 {
            first_ts = earliest(first_ts, ts);
            last_ts = last_ts.max(ts);
        }
        match value.get("type").and_then(Value::as_str) {
            Some("session") => {
                if let Some(found) = value.get("id").and_then(Value::as_str) {
                    id = found.to_owned();
                }
                if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
                    project = cwd.to_owned();
                }
                let created = number(value.get("createdAt")).unwrap_or(0);
                if created > 0 {
                    first_ts = earliest(first_ts, created);
                    last_ts = last_ts.max(created);
                }
            }
            Some("session/title") => {
                if let Some(found) = value.pointer("/data/title").and_then(Value::as_str) {
                    title = found.to_owned();
                }
            }
            Some("user/message") => {
                messages += 1;
                if title.is_empty()
                    && let Some(content) = value.pointer("/data/content")
                {
                    title = dsh_title(&text_value(content));
                }
            }
            Some("assistant/message") => {
                messages += 1;
                if let Some(source) = value.pointer("/data/message/source") {
                    if let Some(found) = source.get("provider").and_then(Value::as_str) {
                        provider = found.to_owned();
                    }
                    if let Some(found) = source.get("model").and_then(Value::as_str) {
                        model = found.to_owned();
                    }
                }
                if let Some(usage) = value.pointer("/data/usage") {
                    input += number(usage.get("inputTokens")).unwrap_or(0).max(0) as u64;
                    output += number(usage.get("outputTokens")).unwrap_or(0).max(0) as u64;
                    cache_read += number(usage.get("cacheReadTokens")).unwrap_or(0).max(0) as u64;
                }
            }
            _ => {}
        }
    }
    if messages == 0 {
        return Ok(None);
    }
    let cost = estimate(&provider, &model, input, output, cache_read, 0);
    if title.is_empty() {
        title = id.clone();
    }
    Ok(Some(Session {
        key: format!("dsh:{id}"),
        tool: "DeepSeek".into(),
        project,
        title,
        model,
        messages,
        input,
        output,
        cache_read,
        cache_write: 0,
        cost,
        first_ts: if first_ts == 0 { fallback_ts } else { first_ts },
        last_ts: if last_ts == 0 { fallback_ts } else { last_ts },
        file: path.display().to_string(),
    }))
}

// ---------------------------------------------------------------------------
// OpenCode: ~/.local/share/opencode/opencode.db.

fn opencode_db() -> Option<PathBuf> {
    if let Some(path) = env_path("SESSIONTREE_OPENCODE_DB") {
        return path.is_file().then_some(path);
    }
    let mut candidates = Vec::new();
    if let Some(data) = env_path("XDG_DATA_HOME") {
        candidates.push(data.join("opencode").join("opencode.db"));
    }
    if let Some(data) = env_path("LOCALAPPDATA") {
        candidates.push(data.join("opencode").join("opencode.db"));
    }
    if let Some(data) = env_path("APPDATA") {
        candidates.push(data.join("opencode").join("opencode.db"));
    }
    if let Some(home) = home_dir() {
        candidates.push(
            home.join(".local")
                .join("share")
                .join("opencode")
                .join("opencode.db"),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn parse_opencode(path: &Path) -> Result<Vec<Session>, String> {
    parse_opencode_tool(path, "OpenCode", "opencode")
}

fn parse_mimo(path: &Path) -> Result<Vec<Session>, String> {
    parse_opencode_tool(path, "MiMo", "mimo")
}

fn parse_zcode_db(path: &Path) -> Result<Vec<Session>, String> {
    parse_opencode_tool(path, "ZCode", "zcode")
}

fn table_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
}

fn table_columns(connection: &Connection, table: &str) -> BTreeSet<String> {
    let mut columns = BTreeSet::new();
    let Ok(mut statement) =
        connection.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
    else {
        return columns;
    };
    if let Ok(rows) = statement.query_map([], |row| row.get::<_, String>(0)) {
        for column in rows.flatten() {
            columns.insert(column);
        }
    }
    columns
}

fn row_text(row: &rusqlite::Row<'_>, index: usize) -> String {
    row.get::<_, Option<String>>(index)
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn row_f64(row: &rusqlite::Row<'_>, index: usize) -> f64 {
    match row.get::<_, rusqlite::types::Value>(index) {
        Ok(rusqlite::types::Value::Integer(number)) => number as f64,
        Ok(rusqlite::types::Value::Real(number)) => number,
        _ => 0.0,
    }
}

#[derive(Clone, Copy, Default)]
struct TurnTotals {
    messages: u64,
    input: u64,
    output: u64,
    reasoning: u64,
    cache_read: u64,
    cache_write: u64,
    cost: f64,
}

fn parse_opencode_tool(path: &Path, tool: &str, prefix: &str) -> Result<Vec<Session>, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let session_table = if table_exists(&connection, "session") {
        "session"
    } else if table_exists(&connection, "session_v2") {
        "session_v2"
    } else {
        return Ok(Vec::new());
    };
    let columns = table_columns(&connection, session_table);
    let pick = |name: &str, fallback: &str| {
        if columns.contains(name) {
            name.to_owned()
        } else {
            fallback.to_owned()
        }
    };
    let sql = format!(
        "SELECT {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {} FROM {}",
        pick("id", "''"),
        pick("title", "''"),
        pick("directory", "''"),
        pick("time_created", "0"),
        pick("time_updated", "0"),
        pick("cost", "0"),
        pick("tokens_input", "0"),
        pick("tokens_output", "0"),
        pick("tokens_reasoning", "0"),
        pick("tokens_cache_read", "0"),
        pick("tokens_cache_write", "0"),
        pick("model", "NULL"),
        session_table,
    );

    let mut counts = BTreeMap::new();
    if table_exists(&connection, "message")
        && let Ok(mut statement) =
            connection.prepare("SELECT session_id, COUNT(*) FROM message GROUP BY session_id")
        && let Ok(rows) = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
    {
        for row in rows.flatten() {
            counts.insert(row.0, row.1.max(0) as u64);
        }
    }
    let mut turns: BTreeMap<String, TurnTotals> = BTreeMap::new();
    let mut models: BTreeMap<String, (String, String)> = BTreeMap::new();
    if table_exists(&connection, "session_message") {
        if let Ok(mut statement) = connection.prepare(
            "SELECT session_id, COUNT(*),
                    SUM(COALESCE(json_extract(data,'$.tokens.input'),json_extract(data,'$.usage.inputTokens'),json_extract(data,'$.usage.input_tokens'),0)),
                    SUM(COALESCE(json_extract(data,'$.tokens.output'),json_extract(data,'$.usage.outputTokens'),json_extract(data,'$.usage.output_tokens'),0)),
                    SUM(COALESCE(json_extract(data,'$.tokens.reasoning'),json_extract(data,'$.usage.reasoningTokens'),json_extract(data,'$.usage.reasoning_tokens'),0)),
                    SUM(COALESCE(json_extract(data,'$.tokens.cache.read'),json_extract(data,'$.usage.cachedInputTokens'),json_extract(data,'$.usage.cache_read_input_tokens'),0)),
                    SUM(COALESCE(json_extract(data,'$.tokens.cache.write'),json_extract(data,'$.usage.cacheWriteTokens'),json_extract(data,'$.usage.cache_creation_input_tokens'),0)),
                    SUM(COALESCE(json_extract(data,'$.cost'),0))
             FROM session_message WHERE type IN ('user','assistant') GROUP BY session_id",
        ) && let Ok(rows) = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row_f64(row, 1).max(0.0) as u64,
                row_f64(row, 2).max(0.0) as u64,
                row_f64(row, 3).max(0.0) as u64,
                row_f64(row, 4).max(0.0) as u64,
                row_f64(row, 5).max(0.0) as u64,
                row_f64(row, 6).max(0.0) as u64,
                row_f64(row, 7).max(0.0),
            ))
        }) {
            for row in rows.flatten() {
                turns.insert(
                    row.0,
                    TurnTotals {
                        messages: row.1,
                        input: row.2,
                        output: row.3,
                        reasoning: row.4,
                        cache_read: row.5,
                        cache_write: row.6,
                        cost: row.7,
                    },
                );
            }
        }
        if let Ok(mut statement) = connection.prepare(
            "SELECT session_id, COALESCE(json_extract(data,'$.modelID'),
                    json_extract(data,'$.model.modelID'), json_extract(data,'$.model.id'),
                    CASE WHEN json_type(data,'$.model')='text' THEN json_extract(data,'$.model') END),
                    COALESCE(json_extract(data,'$.providerID'), json_extract(data,'$.provider'))
             FROM session_message WHERE type='assistant' ORDER BY time_created",
        ) && let Ok(rows) = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row_text(row, 1),
                row_text(row, 2),
            ))
        }) {
            for (id, model, provider) in rows.flatten() {
                if !model.is_empty() {
                    models.insert(id, (model, provider));
                }
            }
        }
    }
    if table_exists(&connection, "message")
        && !table_columns(&connection, "message").contains("role")
    {
        if let Ok(mut statement) = connection.prepare(
            "SELECT session_id, COUNT(*),
                    SUM(COALESCE(json_extract(data,'$.tokens.input'),0)),
                    SUM(COALESCE(json_extract(data,'$.tokens.output'),0)),
                    SUM(COALESCE(json_extract(data,'$.tokens.reasoning'),0)),
                    SUM(COALESCE(json_extract(data,'$.tokens.cache.read'),0)),
                    SUM(COALESCE(json_extract(data,'$.tokens.cache.write'),0)),
                    SUM(COALESCE(json_extract(data,'$.cost'),0))
             FROM message WHERE json_extract(data,'$.role') IN ('user','assistant')
             GROUP BY session_id",
        ) && let Ok(rows) = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row_f64(row, 1).max(0.0) as u64,
                row_f64(row, 2).max(0.0) as u64,
                row_f64(row, 3).max(0.0) as u64,
                row_f64(row, 4).max(0.0) as u64,
                row_f64(row, 5).max(0.0) as u64,
                row_f64(row, 6).max(0.0) as u64,
                row_f64(row, 7).max(0.0),
            ))
        }) {
            for row in rows.flatten() {
                turns.insert(
                    row.0,
                    TurnTotals {
                        messages: row.1,
                        input: row.2,
                        output: row.3,
                        reasoning: row.4,
                        cache_read: row.5,
                        cache_write: row.6,
                        cost: row.7,
                    },
                );
            }
        }
        if let Ok(mut statement) = connection.prepare(
            "SELECT session_id, COALESCE(json_extract(data,'$.modelID'),
                    json_extract(data,'$.model.modelID'), json_extract(data,'$.model.id')),
                    COALESCE(json_extract(data,'$.providerID'), json_extract(data,'$.provider'))
             FROM message WHERE json_extract(data,'$.role')='assistant' ORDER BY time_created",
        ) && let Ok(rows) = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row_text(row, 1), row_text(row, 2)))
        }) {
            for (id, model, provider) in rows.flatten() {
                if !model.is_empty() {
                    models.insert(id, (model, provider));
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row_text(row, 0),
                row_text(row, 1),
                row_text(row, 2),
                row_f64(row, 3) as i64,
                row_f64(row, 4) as i64,
                row_f64(row, 5),
                row_f64(row, 6).max(0.0) as u64,
                row_f64(row, 7).max(0.0) as u64,
                row_f64(row, 8).max(0.0) as u64,
                row_f64(row, 9).max(0.0) as u64,
                row_f64(row, 10).max(0.0) as u64,
                row.get::<_, Option<String>>(11)?,
            ))
        })
        .map_err(|error| format!("{}: {error}", path.display()))?;
    for row in rows.flatten() {
        let (
            id,
            title,
            directory,
            created,
            updated,
            stored_cost,
            mut input,
            mut output,
            mut reasoning,
            mut cache_read,
            mut cache_write,
            model,
        ) = row;
        let turn = turns.get(&id).copied();
        if input + output + reasoning + cache_read + cache_write == 0
            && let Some(turn) = turn
        {
            input = turn.input;
            output = turn.output;
            reasoning = turn.reasoning;
            cache_read = turn.cache_read;
            cache_write = turn.cache_write;
        }
        let messages = turn
            .map(|turn| turn.messages)
            .filter(|count| *count > 0)
            .or_else(|| counts.get(&id).copied())
            .unwrap_or(0);
        let from_message = models.get(&id).cloned().unwrap_or_default();
        let model = model.filter(|model| !model.is_empty());
        let (model_id, provider) = match model {
            Some(model) => parse_opencode_model(Some(&model)),
            None if !from_message.0.is_empty() => (
                from_message.0,
                if from_message.1.is_empty() {
                    "opencode".into()
                } else {
                    from_message.1
                },
            ),
            None => (String::new(), "opencode".into()),
        };
        let mut cost = stored_cost.max(0.0);
        if cost == 0.0
            && let Some(turn) = turn
            && turn.cost > 0.0
        {
            cost = turn.cost;
        }
        if cost == 0.0 {
            cost = price_for(&provider, &model_id)
                .map(|price| {
                    input as f64 * price.0
                        + (output + reasoning) as f64 * price.1
                        + cache_read as f64 * price.2
                        + cache_write as f64 * price.3
                })
                .unwrap_or(0.0)
                / 1_000_000.0;
        }
        out.push(Session {
            key: format!("{prefix}:{id}"),
            tool: tool.to_owned(),
            project: directory,
            title: if title.trim().is_empty() {
                "(untitled session)".into()
            } else {
                title
            },
            model: model_id,
            messages,
            input,
            output: output + reasoning,
            cache_read,
            cache_write,
            cost,
            first_ts: created,
            last_ts: updated.max(created),
            file: path.display().to_string(),
        });
    }
    Ok(out)
}

fn parse_opencode_model(raw: Option<&str>) -> (String, String) {
    let Some(raw) = raw else {
        return (String::new(), "opencode".into());
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return (raw.to_owned(), "opencode".into());
    };
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let provider = value
        .get("providerID")
        .and_then(Value::as_str)
        .unwrap_or("opencode")
        .to_owned();
    (id, provider)
}

// ---------------------------------------------------------------------------
// Cursor: state.vscdb (composerHeaders + cursorDiskKV).

fn cursor_db() -> Option<PathBuf> {
    if let Some(path) = env_path("SESSIONTREE_CURSOR_DB") {
        return path.is_file().then_some(path);
    }
    let mut candidates = Vec::new();
    if let Some(home) = home_dir() {
        candidates
            .push(home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb"));
        candidates.push(home.join(".config/Cursor/User/globalStorage/state.vscdb"));
    }
    if let Some(data) = env_path("APPDATA") {
        candidates.push(data.join("Cursor/User/globalStorage/state.vscdb"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn cursor_value(connection: &Connection, key: &str) -> Option<Value> {
    connection
        .query_row(
            "SELECT value FROM cursorDiskKV WHERE key = ?1",
            [key],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
}

fn cursor_merge(base: &mut Value, extra: &Value) {
    let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) else {
        return;
    };
    for (key, value) in extra {
        let replace = base.get(key).map(Value::is_null).unwrap_or(true);
        if replace && !value.is_null() {
            base.insert(key.clone(), value.clone());
        }
    }
}

fn cursor_bubbles(connection: &Connection, composer_id: &str) -> Vec<Value> {
    let Some(composer) = cursor_value(connection, &format!("composerData:{composer_id}")) else {
        return Vec::new();
    };
    let conversation = composer.get("conversation").and_then(Value::as_array);
    if let Some(items) = conversation.filter(|items| !items.is_empty()) {
        let mut bubbles = Vec::new();
        for item in items {
            let mut bubble = item.clone();
            if let Some(id) = item.get("bubbleId").and_then(Value::as_str)
                && let Some(extra) =
                    cursor_value(connection, &format!("bubbleId:{composer_id}:{id}"))
            {
                cursor_merge(&mut bubble, &extra);
            }
            bubbles.push(bubble);
        }
        return bubbles;
    }
    let Some(headers) = composer
        .get("fullConversationHeadersOnly")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    headers
        .iter()
        .filter_map(|header| {
            let id = header.get("bubbleId").and_then(Value::as_str)?;
            cursor_value(connection, &format!("bubbleId:{composer_id}:{id}"))
        })
        .collect()
}

fn cursor_text(value: &Value) -> String {
    value
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn cursor_bubble_model(value: &Value) -> Option<String> {
    value
        .get("modelInfo")
        .and_then(|model| model.get("modelName"))
        .or_else(|| value.get("modelName"))
        .and_then(Value::as_str)
        .filter(|model| *model != "default")
        .map(ToOwned::to_owned)
}

fn cursor_project(header: &Value) -> String {
    header
        .pointer("/workspaceIdentifier/uri/fsPath")
        .or_else(|| header.pointer("/draftTarget/environment/uri/fsPath"))
        .or_else(|| header.pointer("/trackedGitRepos/0/repoPath"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn cursor_title(header: &Value, bubbles: &[Value]) -> String {
    if let Some(title) = header
        .get("name")
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty())
        .or_else(|| {
            header
                .get("subtitle")
                .and_then(Value::as_str)
                .filter(|title| !title.is_empty())
        })
    {
        return title.chars().take(160).collect();
    }
    bubbles
        .iter()
        .find(|bubble| bubble.get("type").and_then(Value::as_i64) == Some(1))
        .map(cursor_text)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(160).collect())
        .unwrap_or_default()
}

fn cursor_rows(connection: &Connection) -> Vec<(String, i64, i64, Value)> {
    let Ok(mut statement) = connection.prepare(
        "SELECT composerId, createdAt, lastUpdatedAt, value
         FROM composerHeaders ORDER BY createdAt ASC",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        let value: String = row.get(3)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            serde_json::from_str::<Value>(&value).unwrap_or(Value::Null),
        ))
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

fn cursor_session_stats(bubbles: &[Value]) -> (u64, u64, u64, Option<String>) {
    let mut messages = 0_u64;
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut model = None;
    for bubble in bubbles {
        let kind = bubble.get("type").and_then(Value::as_i64);
        if kind != Some(1) && kind != Some(2) {
            continue;
        }
        messages += 1;
        if kind == Some(2) {
            if let Some(found) = cursor_bubble_model(bubble) {
                model = Some(found);
            }
            if let Some(tokens) = bubble.get("tokenCount") {
                input += number(tokens.get("inputTokens")).unwrap_or(0).max(0) as u64;
                output += number(tokens.get("outputTokens")).unwrap_or(0).max(0) as u64;
            }
        }
    }
    (messages, input, output, model)
}

fn parse_cursor(path: &Path) -> Result<Vec<Session>, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let mut out = Vec::new();
    for (id, created, updated, header) in cursor_rows(&connection) {
        let bubbles = cursor_bubbles(&connection, &id);
        let (messages, input, output, model) = cursor_session_stats(&bubbles);
        let project = cursor_project(&header);
        let title = cursor_title(&header, &bubbles);
        out.push(Session {
            key: format!("cursor:{id}"),
            tool: "Cursor".into(),
            project,
            title: if title.is_empty() {
                "(untitled session)".into()
            } else {
                title
            },
            model: model.unwrap_or_else(|| "cursor".into()),
            messages,
            input,
            output,
            cache_read: 0,
            cache_write: 0,
            cost: 0.0,
            first_ts: created,
            last_ts: updated.max(created),
            file: path.display().to_string(),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Copilot Chat: session-store.db (sessions + turns).

fn copilot_db() -> Option<PathBuf> {
    if let Some(path) = env_path("SESSIONTREE_COPILOT_DB") {
        return path.is_file().then_some(path);
    }
    let mut candidates = Vec::new();
    if let Some(home) = home_dir() {
        for edition in ["Code", "Code - Insiders"] {
            candidates.push(
                home.join("Library/Application Support")
                    .join(edition)
                    .join("User/globalStorage/github.copilot-chat/session-store.db"),
            );
            candidates.push(
                home.join(".config")
                    .join(edition)
                    .join("User/globalStorage/github.copilot-chat/session-store.db"),
            );
        }
    }
    if let Some(data) = env_path("APPDATA") {
        for edition in ["Code", "Code - Insiders"] {
            candidates.push(
                data.join(edition)
                    .join("User/globalStorage/github.copilot-chat/session-store.db"),
            );
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn copilot_time(text: &str) -> i64 {
    rfc3339_millis(text).unwrap_or(0)
}

fn copilot_rows(
    connection: &Connection,
) -> Vec<(String, String, String, String, String, i64, i64)> {
    let Ok(mut statement) = connection.prepare(
        "SELECT id, COALESCE(cwd, ''), COALESCE(repository, ''), COALESCE(summary, ''),
                COALESCE(agent_name, ''), COALESCE(created_at, ''), COALESCE(updated_at, '')
         FROM sessions ORDER BY created_at ASC",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        let created: String = row.get(5)?;
        let updated: String = row.get(6)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            copilot_time(&created),
            copilot_time(&updated),
        ))
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

fn copilot_turns(connection: &Connection, session_id: &str) -> Vec<(i64, String, String, i64)> {
    let Ok(mut statement) = connection.prepare(
        "SELECT turn_index, COALESCE(user_message, ''), COALESCE(assistant_response, ''),
                COALESCE(timestamp, '')
         FROM turns WHERE session_id = ?1 ORDER BY turn_index ASC",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([session_id], |row| {
        let ts: String = row.get(3)?;
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            copilot_time(&ts),
        ))
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

fn copilot_title(summary: &str, agent: &str, turns: &[(i64, String, String, i64)]) -> String {
    if !summary.trim().is_empty() {
        return summary.chars().take(160).collect();
    }
    turns
        .iter()
        .find(|(_, user, _, _)| !user.trim().is_empty())
        .map(|(_, user, _, _)| user.chars().take(160).collect())
        .or_else(|| (!agent.is_empty()).then(|| agent.to_owned()))
        .unwrap_or_default()
}

fn parse_copilot(path: &Path) -> Result<Vec<Session>, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let mut out = Vec::new();
    for (id, cwd, repository, summary, agent, created, updated) in copilot_rows(&connection) {
        let turns = copilot_turns(&connection, &id);
        let title = copilot_title(&summary, &agent, &turns);
        out.push(Session {
            key: format!("copilot:{id}"),
            tool: "Copilot".into(),
            project: if cwd.is_empty() { repository } else { cwd },
            title: if title.is_empty() {
                "(untitled session)".into()
            } else {
                title
            },
            model: "copilot".into(),
            messages: turns.len() as u64,
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            cost: 0.0,
            first_ts: created,
            last_ts: updated.max(created),
            file: path.display().to_string(),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// ZCode: ~/.zcode/projects/**/*.jsonl, ~/.zcode/v2/sessions/**/*.json,
// and the CLI's OpenCode-schema database at ~/.zcode/cli/db/db.sqlite.

fn zcode_root() -> Option<PathBuf> {
    env_path("SESSIONTREE_ZCODE_ROOT")
        .or_else(|| home_dir().map(|home| home.join(".zcode").join("projects")))
}

fn zcode_legacy_root() -> Option<PathBuf> {
    env_path("SESSIONTREE_ZCODE_LEGACY_ROOT")
        .or_else(|| home_dir().map(|home| home.join(".zcode").join("v2").join("sessions")))
}

fn zcode_db() -> Option<PathBuf> {
    env_path("SESSIONTREE_ZCODE_DB").or_else(|| {
        home_dir()
            .map(|home| home.join(".zcode").join("cli").join("db").join("db.sqlite"))
            .filter(|path| path.is_file())
    })
}

fn walk_files(
    root: &Path,
    errors: &mut Vec<String>,
    keep: impl Fn(&Path) -> bool + Copy,
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                push_error(errors, format!("{}: {error}", directory.display()));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if keep(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn zcode_transcript_files(errors: &mut Vec<String>) -> Vec<PathBuf> {
    let Some(root) = zcode_root().filter(|root| root.is_dir()) else {
        return Vec::new();
    };
    walk_files(&root, errors, |path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        name.ends_with(".jsonl")
            && !name.ends_with(".checkpoints.jsonl")
            && !name.ends_with(".prompts.jsonl")
            && !name.ends_with(".meta.json")
    })
}

fn zcode_legacy_files(errors: &mut Vec<String>) -> Vec<PathBuf> {
    let Some(root) = zcode_legacy_root().filter(|root| root.is_dir()) else {
        return Vec::new();
    };
    walk_files(&root, errors, |path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        name.ends_with(".json") && !name.ends_with(".deleted.json")
    })
}

fn decode_project_slug(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for ch in name.chars() {
        out.push(if ch == '-' { '/' } else { ch });
    }
    if !out.starts_with('/') {
        out.insert(0, '/');
    }
    out
}

fn timestamp_millis(value: Option<&Value>) -> i64 {
    let Some(value) = value else {
        return 0;
    };
    let number =
        number(Some(value)).or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()));
    if let Some(number) = number {
        return if number > 0 && number < 100_000_000_000 {
            number * 1000
        } else {
            number
        };
    }
    value.as_str().and_then(rfc3339_millis).unwrap_or(0)
}

fn model_from_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(model)) => model.clone(),
        Some(Value::Object(fields)) => ["id", "modelID", "model", "name"]
            .iter()
            .find_map(|key| fields.get(*key).and_then(Value::as_str))
            .unwrap_or("")
            .to_owned(),
        _ => String::new(),
    }
}

fn usage_tokens(usage: Option<&Value>) -> (u64, u64, u64, u64, f64) {
    let Some(usage) = usage else {
        return (0, 0, 0, 0, 0.0);
    };
    let token = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| number(usage.get(*key)))
            .unwrap_or(0)
            .max(0) as u64
    };
    let input = token(&["input_tokens", "inputTokens", "prompt_tokens", "input"]);
    let output = token(&[
        "output_tokens",
        "outputTokens",
        "completion_tokens",
        "output",
    ]);
    let cache_read = token(&[
        "cache_read_input_tokens",
        "cacheReadTokens",
        "cached_tokens",
        "cache_read",
    ]);
    let cache_write = token(&[
        "cache_creation_input_tokens",
        "cacheWriteTokens",
        "cache_write",
    ]);
    let cost = usage
        .get("cost")
        .and_then(|cost| cost.get("total").or(Some(cost)))
        .and_then(Value::as_f64)
        .unwrap_or_else(|| {
            usage
                .get("total_cost")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
        });
    (input, output, cache_read, cache_write, cost)
}

fn provider_for_model(model: &str) -> &'static str {
    let model = model.to_ascii_lowercase();
    if model.contains("deepseek") {
        "deepseek"
    } else if model.contains("mimo") || model.contains("xiaomi") {
        "xiaomi"
    } else if model.contains("qwen") {
        "qwen"
    } else if model.contains("kimi") || model.contains("moonshot") {
        "moonshot"
    } else if model.contains("claude") {
        "anthropic"
    } else {
        "zai"
    }
}

fn zcode_title(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}

fn parse_zcode_file(path: &Path, fallback_ts: i64) -> Result<Option<Session>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("session")
        .to_owned();
    let mut project = path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .map(decode_project_slug)
        .unwrap_or_default();
    let mut title = String::new();
    let mut model = String::new();
    let mut messages = 0_u64;
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut cache_read = 0_u64;
    let mut cache_write = 0_u64;
    let mut stored_cost = 0.0;
    let mut first_ts = 0_i64;
    let mut last_ts = 0_i64;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let role = value.get("role").and_then(Value::as_str).unwrap_or("");
        if role != "user" && role != "assistant" {
            continue;
        }
        if let Some(found) = value
            .get("sessionId")
            .or_else(|| value.get("session_id"))
            .and_then(Value::as_str)
        {
            id = found.to_owned();
        }
        if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
            project = cwd.to_owned();
        }
        let ts = timestamp_millis(
            value
                .get("timestamp")
                .or_else(|| value.get("time"))
                .or_else(|| value.get("createdAt")),
        );
        if ts > 0 {
            first_ts = earliest(first_ts, ts);
            last_ts = last_ts.max(ts);
        }
        messages += 1;
        let content = value.get("content").map(text_value).unwrap_or_default();
        if role == "user" && title.is_empty() && !content.trim().is_empty() {
            title = zcode_title(&content);
        }
        if let Some(found) = value
            .get("model")
            .or_else(|| value.get("modelId"))
            .or_else(|| value.get("modelID"))
        {
            let found = model_from_value(Some(found));
            if !found.is_empty() {
                model = found;
            }
        }
        if role == "assistant" {
            let (turn_input, turn_output, turn_read, turn_write, turn_cost) =
                usage_tokens(value.get("usage"));
            input += turn_input;
            output += turn_output;
            cache_read += turn_read;
            cache_write += turn_write;
            stored_cost += turn_cost;
        }
    }
    if messages == 0 {
        return Ok(None);
    }
    let provider = provider_for_model(&model);
    let cost = if stored_cost > 0.0 {
        stored_cost
    } else {
        estimate(provider, &model, input, output, cache_read, cache_write)
    };
    if title.is_empty() {
        title = id.clone();
    }
    Ok(Some(Session {
        key: format!("zcode:{id}"),
        tool: "ZCode".into(),
        project,
        title,
        model,
        messages,
        input,
        output,
        cache_read,
        cache_write,
        cost,
        first_ts: if first_ts == 0 { fallback_ts } else { first_ts },
        last_ts: if last_ts == 0 { fallback_ts } else { last_ts },
        file: path.display().to_string(),
    }))
}

fn parse_zcode_legacy_file(path: &Path, fallback_ts: i64) -> Result<Option<Session>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Ok(None);
    };
    let meta = value.get("meta").unwrap_or(&Value::Null);
    let id = ["acpSessionId", "taskId"]
        .iter()
        .find_map(|key| meta.get(*key).and_then(Value::as_str))
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("session")
        })
        .to_owned();
    let project = meta
        .get("workspacePath")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut title = meta
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut messages = 0_u64;
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut cache_read = 0_u64;
    let mut cache_write = 0_u64;
    let mut stored_cost = 0.0;
    let mut model = String::new();
    let mut first_ts = timestamp_millis(meta.get("createdAt").or_else(|| meta.get("created_at")));
    let mut last_ts = timestamp_millis(meta.get("updatedAt").or_else(|| meta.get("updated_at")));

    if let Some(items) = value.get("messages").and_then(Value::as_array) {
        for item in items {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("");
            if role != "user" && role != "assistant" {
                continue;
            }
            messages += 1;
            let ts = timestamp_millis(
                item.get("timestamp")
                    .or_else(|| item.get("createdAt"))
                    .or_else(|| item.get("time")),
            );
            if ts > 0 {
                first_ts = earliest(first_ts, ts);
                last_ts = last_ts.max(ts);
            }
            let content = item.get("content").map(text_value).unwrap_or_default();
            if role == "user" && title.is_empty() && !content.trim().is_empty() {
                title = zcode_title(&content);
            }
            if let Some(found) = item
                .get("model")
                .or_else(|| item.get("modelId"))
                .or_else(|| item.get("modelID"))
            {
                let found = model_from_value(Some(found));
                if !found.is_empty() {
                    model = found;
                }
            }
            if role == "assistant" {
                let (turn_input, turn_output, turn_read, turn_write, turn_cost) =
                    usage_tokens(item.get("usage"));
                input += turn_input;
                output += turn_output;
                cache_read += turn_read;
                cache_write += turn_write;
                stored_cost += turn_cost;
            }
        }
    }
    if messages == 0 {
        return Ok(None);
    }
    let provider = provider_for_model(&model);
    let cost = if stored_cost > 0.0 {
        stored_cost
    } else {
        estimate(provider, &model, input, output, cache_read, cache_write)
    };
    if title.is_empty() {
        title = id.clone();
    }
    Ok(Some(Session {
        key: format!("zcode:{id}"),
        tool: "ZCode".into(),
        project,
        title,
        model,
        messages,
        input,
        output,
        cache_read,
        cache_write,
        cost,
        first_ts: if first_ts == 0 { fallback_ts } else { first_ts },
        last_ts: if last_ts == 0 { fallback_ts } else { last_ts },
        file: path.display().to_string(),
    }))
}

// ---------------------------------------------------------------------------
// MiMo Code: ${MIMOCODE_HOME:-${XDG_DATA_HOME:-~/.local/share}/mimocode}/mimocode.db.

fn mimocode_db() -> Option<PathBuf> {
    if let Some(path) = env_path("SESSIONTREE_MIMO_DB").or_else(|| env_path("MIMOCODE_DB")) {
        return path.is_file().then_some(path);
    }
    let mut candidates = Vec::new();
    if let Some(home) = env_path("MIMOCODE_HOME") {
        candidates.push(home.join("mimocode.db"));
    }
    if let Some(data) = env_path("XDG_DATA_HOME") {
        candidates.push(data.join("mimocode").join("mimocode.db"));
    }
    if let Some(data) = env_path("LOCALAPPDATA") {
        candidates.push(data.join("mimocode").join("mimocode.db"));
    }
    if let Some(data) = env_path("APPDATA") {
        candidates.push(data.join("mimocode").join("mimocode.db"));
    }
    if let Some(home) = home_dir() {
        candidates.push(
            home.join(".local")
                .join("share")
                .join("mimocode")
                .join("mimocode.db"),
        );
        candidates.push(
            home.join("Library")
                .join("Application Support")
                .join("mimocode")
                .join("mimocode.db"),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

// ---------------------------------------------------------------------------
// Antigravity (agy): ~/.gemini/antigravity*/brain/<session>/.system_generated/logs/transcript.jsonl.

fn antigravity_roots() -> Vec<PathBuf> {
    if let Some(root) = env_path("SESSIONTREE_ANTIGRAVITY_ROOT") {
        return vec![root];
    }
    let Some(gemini) = home_dir().map(|home| home.join(".gemini")) else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    if let Ok(entries) = fs::read_dir(gemini) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() && name.starts_with("antigravity") {
                roots.push(path);
            }
        }
    }
    roots.sort();
    roots
}

fn antigravity_files(errors: &mut Vec<String>) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    for root in antigravity_roots() {
        let brain = root.join("brain");
        let entries = match fs::read_dir(&brain) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let transcript = entry
                .path()
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            if transcript.is_file() {
                files.insert(transcript);
            }
        }
    }
    let _ = errors;
    files.into_iter().collect()
}

fn clean_tagged(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        out.push_str(&rest[..start]);
        let after = &rest[start + open.len()..];
        if let Some(end) = after.find(&close) {
            rest = &after[end + close.len()..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

fn antigravity_text(text: &str) -> String {
    let text = clean_tagged(text, "ADDITIONAL_METADATA");
    let text = clean_tagged(&text, "USER_SETTINGS_CHANGE");
    text.replace("<USER_REQUEST>", "\n")
        .replace("</USER_REQUEST>", "\n")
        .trim()
        .to_owned()
}

fn antigravity_uri_path(uri: &str) -> String {
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    path.replace("%20", " ")
}

fn antigravity_project(root: &Path, id: &str) -> String {
    let metadata = root.join("cache").join("conversation_metadata.json");
    if let Ok(text) = fs::read_to_string(&metadata)
        && let Ok(value) = serde_json::from_str::<Value>(&text)
        && let Some(conversations) = value.get("conversations").and_then(Value::as_object)
    {
        for (key, conversation) in conversations {
            if !key.starts_with(id) && !id.starts_with(key.as_str()) {
                continue;
            }
            if let Some(uris) = conversation
                .pointer("/summary/WorkspaceURIs")
                .and_then(Value::as_array)
                && let Some(uri) = uris.first().and_then(Value::as_str)
            {
                return antigravity_uri_path(uri);
            }
        }
    }
    let last = root.join("cache").join("last_conversations.json");
    if let Ok(text) = fs::read_to_string(&last)
        && let Ok(value) = serde_json::from_str::<Value>(&text)
        && let Some(map) = value.as_object()
    {
        for (workspace, conversation) in map {
            if let Some(conversation) = conversation.as_str()
                && (conversation.starts_with(id) || id.starts_with(conversation))
            {
                return workspace.clone();
            }
        }
    }
    String::new()
}

fn parse_antigravity_file(path: &Path, fallback_ts: i64) -> Result<Option<Session>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let id = path
        .ancestors()
        .find(|part| part.file_name().and_then(|name| name.to_str()) == Some("brain"))
        .and_then(|brain| brain.parent())
        .and_then(|root| {
            path.strip_prefix(root).ok().and_then(|rest| {
                rest.components()
                    .nth(1)
                    .map(|part| part.as_os_str().to_string_lossy().to_string())
            })
        })
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("session")
                .to_owned()
        });
    let root = path
        .ancestors()
        .find(|part| part.file_name().and_then(|name| name.to_str()) == Some("brain"))
        .and_then(|brain| brain.parent())
        .map(Path::to_path_buf);
    let project = root
        .as_deref()
        .map(|root| antigravity_project(root, &id))
        .unwrap_or_default();
    let mut title = String::new();
    let mut messages = 0_u64;
    let mut first_ts = 0_i64;
    let mut last_ts = 0_i64;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let source = value.get("source").and_then(Value::as_str).unwrap_or("");
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let role = match source {
            "USER_EXPLICIT" => "user",
            "MODEL" if kind.is_empty() || kind == "PLANNER_RESPONSE" => "assistant",
            _ => continue,
        };
        let text = value.get("content").and_then(Value::as_str).unwrap_or("");
        let text = if role == "user" {
            antigravity_text(text)
        } else {
            text.trim().to_owned()
        };
        if text.is_empty() {
            continue;
        }
        messages += 1;
        let ts = value
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(rfc3339_millis)
            .unwrap_or(0);
        if ts > 0 {
            first_ts = earliest(first_ts, ts);
            last_ts = last_ts.max(ts);
        }
        if role == "user" && title.is_empty() {
            title = zcode_title(&text);
        }
    }
    if messages == 0 {
        return Ok(None);
    }
    if title.is_empty() {
        title = id.clone();
    }
    Ok(Some(Session {
        key: format!("antigravity:{id}"),
        tool: "Antigravity".into(),
        project,
        title,
        model: "antigravity".into(),
        messages,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cost: 0.0,
        first_ts: if first_ts == 0 { fallback_ts } else { first_ts },
        last_ts: if last_ts == 0 { fallback_ts } else { last_ts },
        file: path.display().to_string(),
    }))
}

// ---------------------------------------------------------------------------
// Kiro: ~/.kiro/sessions/cli/<id>.jsonl with <id>.json, and
// ~/.kiro/sessions/<workspace>/sess_<uuid>/messages.jsonl with session.json.

fn kiro_root() -> Option<PathBuf> {
    env_path("SESSIONTREE_KIRO_ROOT")
        .or_else(|| home_dir().map(|home| home.join(".kiro").join("sessions")))
}

fn kiro_cli_files(errors: &mut Vec<String>) -> Vec<PathBuf> {
    let Some(root) = kiro_root()
        .map(|root| root.join("cli"))
        .filter(|root| root.is_dir())
    else {
        return Vec::new();
    };
    walk_files(&root, errors, |path| {
        path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
    })
}

fn kiro_ide_files(errors: &mut Vec<String>) -> Vec<PathBuf> {
    let Some(root) = kiro_root().filter(|root| root.is_dir()) else {
        return Vec::new();
    };
    walk_files(&root, errors, |path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name != "messages.jsonl" {
            return false;
        }
        path.parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("sess_"))
    })
}

fn kiro_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    let Some(parts) = content.as_array() else {
        return String::new();
    };
    parts
        .iter()
        .filter_map(|part| {
            let kind = part.get("kind").and_then(Value::as_str).unwrap_or("text");
            if !kind.is_empty() && kind != "text" {
                return None;
            }
            part.get("data").and_then(Value::as_str)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_kiro_cli_file(path: &Path, fallback_ts: i64) -> Result<Option<Session>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("session");
    let mut id = stem.to_owned();
    let mut project = String::new();
    let mut model = String::new();
    let header_path = path.with_extension("json");
    if let Ok(text) = fs::read_to_string(&header_path)
        && let Ok(value) = serde_json::from_str::<Value>(&text)
    {
        if let Some(found) = value.get("session_id").and_then(Value::as_str) {
            id = found.to_owned();
        }
        if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
            project = cwd.to_owned();
        }
        model = model_from_value(value.get("model").or_else(|| value.get("modelId")));
    }
    let mut title = String::new();
    let mut messages = 0_u64;
    let mut first_ts = 0_i64;
    let mut last_ts = 0_i64;
    let mut last_assistant = String::new();

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let kind = value.get("kind").and_then(Value::as_str).unwrap_or("");
        let Some(data) = value.get("data") else {
            continue;
        };
        let text = kiro_text(data.get("content"));
        if text.trim().is_empty() {
            continue;
        }
        let ts = timestamp_millis(data.pointer("/meta/timestamp"));
        if ts > 0 {
            first_ts = earliest(first_ts, ts);
            last_ts = last_ts.max(ts);
        }
        match kind {
            "Prompt" => {
                messages += 1;
                last_assistant.clear();
                if title.is_empty() {
                    title = zcode_title(&text);
                }
            }
            "AssistantMessage" => {
                let message_id = data.get("message_id").and_then(Value::as_str).unwrap_or("");
                if !message_id.is_empty() && message_id == last_assistant {
                    continue;
                }
                last_assistant = message_id.to_owned();
                messages += 1;
            }
            _ => {}
        }
    }
    if messages == 0 {
        return Ok(None);
    }
    if title.is_empty() {
        title = id.clone();
    }
    Ok(Some(Session {
        key: format!("kiro:{id}"),
        tool: "Kiro".into(),
        project,
        title,
        model,
        messages,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cost: 0.0,
        first_ts: if first_ts == 0 { fallback_ts } else { first_ts },
        last_ts: if last_ts == 0 { fallback_ts } else { last_ts },
        file: path.display().to_string(),
    }))
}

fn kiro_ide_line(value: &Value) -> (String, String) {
    let payload = value.get("payload").unwrap_or(value);
    let kind = payload
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| payload.get("role").and_then(Value::as_str))
        .unwrap_or("");
    if payload
        .get("operationType")
        .and_then(Value::as_str)
        .is_some_and(|operation| operation == "Reasoning")
    {
        return (String::new(), String::new());
    }
    let text = kiro_text(payload.get("content"));
    let role = match kind {
        "user" | "human" | "prompt" => "user",
        "assistant" | "bot" | "response" => "assistant",
        _ => "",
    };
    (role.to_owned(), text)
}

fn parse_kiro_ide_file(path: &Path, fallback_ts: i64) -> Result<Option<Session>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let directory = path.parent().unwrap_or(path);
    let mut id = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session")
        .to_owned();
    let mut project = String::new();
    let mut model = String::new();
    let mut first_ts = 0_i64;
    if let Ok(text) = fs::read_to_string(directory.join("session.json"))
        && let Ok(value) = serde_json::from_str::<Value>(&text)
    {
        if let Some(found) = value.get("id").and_then(Value::as_str) {
            id = found.to_owned();
        }
        if let Some(workspace) = value
            .get("workspacePaths")
            .and_then(Value::as_array)
            .and_then(|paths| paths.first())
            .and_then(Value::as_str)
        {
            project = workspace.to_owned();
        }
        model = model_from_value(value.get("model").or_else(|| value.get("modelId")));
        first_ts = timestamp_millis(value.get("createdAt"));
    }
    let mut title = String::new();
    let mut messages = 0_u64;
    let mut last_ts = first_ts;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let (role, text) = kiro_ide_line(&value);
        if role.is_empty() || text.trim().is_empty() {
            continue;
        }
        messages += 1;
        let ts = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(rfc3339_millis)
            .unwrap_or(0);
        if ts > 0 {
            first_ts = earliest(first_ts, ts);
            last_ts = last_ts.max(ts);
        }
        if role == "user" && title.is_empty() {
            title = zcode_title(&text);
        }
    }
    if messages == 0 {
        return Ok(None);
    }
    if title.is_empty() {
        title = id.clone();
    }
    Ok(Some(Session {
        key: format!("kiro:{id}"),
        tool: "Kiro".into(),
        project,
        title,
        model,
        messages,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cost: 0.0,
        first_ts: if first_ts == 0 { fallback_ts } else { first_ts },
        last_ts: if last_ts == 0 { fallback_ts } else { last_ts },
        file: path.display().to_string(),
    }))
}

fn dedupe_sessions(sessions: Vec<Session>) -> Vec<Session> {
    let mut out: Vec<Session> = Vec::new();
    let mut zcode: BTreeMap<String, usize> = BTreeMap::new();
    for session in sessions {
        if !session.key.starts_with("zcode:") {
            out.push(session);
            continue;
        }
        match zcode.get(&session.key).copied() {
            Some(index) => {
                let existing = &mut out[index];
                let score = session.input + session.output + session.messages;
                let current = existing.input + existing.output + existing.messages;
                if score > current || (score == current && session.cost > existing.cost) {
                    *existing = session;
                }
            }
            None => {
                zcode.insert(session.key.clone(), out.len());
                out.push(session);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Pricing, adapted from aishark's built-in table (USD per million tokens).

type Price = (f64, f64, f64, f64);

fn prefix(model: &str, prefix: &str) -> bool {
    model.starts_with(prefix)
}

fn price_for(provider: &str, model: &str) -> Option<Price> {
    let model = model.to_ascii_lowercase();
    match provider {
        "anthropic" => {
            if prefix(&model, "claude-opus") || prefix(&model, "claude-3-opus") {
                Some((15.0, 75.0, 1.5, 18.75))
            } else if prefix(&model, "claude-sonnet") || prefix(&model, "claude-3-7") {
                Some((3.0, 15.0, 0.3, 3.75))
            } else if prefix(&model, "claude-haiku") || prefix(&model, "claude-3-5-haiku") {
                Some((0.8, 4.0, 0.08, 1.0))
            } else if prefix(&model, "claude-3-5-sonnet") || prefix(&model, "claude-3-sonnet") {
                Some((3.0, 15.0, 0.3, 3.75))
            } else if prefix(&model, "claude-3-haiku") {
                Some((0.25, 1.25, 0.03, 0.3))
            } else {
                None
            }
        }
        "openai" => {
            if prefix(&model, "gpt-4o-mini") {
                Some((0.15, 0.6, 0.075, 0.0))
            } else if prefix(&model, "gpt-4o") {
                Some((2.5, 10.0, 1.25, 0.0))
            } else if prefix(&model, "gpt-4.1-mini") {
                Some((0.4, 1.6, 0.1, 0.0))
            } else if prefix(&model, "gpt-4.1-nano") {
                Some((0.1, 0.4, 0.025, 0.0))
            } else if prefix(&model, "gpt-4.1") {
                Some((2.0, 8.0, 0.5, 0.0))
            } else if prefix(&model, "gpt-5-mini") {
                Some((0.25, 2.0, 0.025, 0.0))
            } else if prefix(&model, "gpt-5-nano") {
                Some((0.05, 0.4, 0.005, 0.0))
            } else if prefix(&model, "gpt-5") {
                Some((1.25, 10.0, 0.125, 0.0))
            } else if prefix(&model, "o4-mini") {
                Some((1.1, 4.4, 0.275, 0.0))
            } else if prefix(&model, "o3-mini") {
                Some((1.1, 4.4, 0.55, 0.0))
            } else if prefix(&model, "o3") {
                Some((2.0, 8.0, 0.5, 0.0))
            } else {
                None
            }
        }
        "gemini" => {
            if prefix(&model, "gemini-2.5-pro") {
                Some((1.25, 10.0, 0.31, 0.0))
            } else if prefix(&model, "gemini-2.5-flash") {
                Some((0.3, 2.5, 0.075, 0.0))
            } else if prefix(&model, "gemini-2.0-flash") {
                Some((0.1, 0.4, 0.025, 0.0))
            } else {
                None
            }
        }
        "deepseek" | "deepseek-official" => deepseek_price(&model),
        "zai" | "zhipu" | "zcode" => zai_price(&model),
        "xiaomi" | "mimo" => mimo_price(&model),
        "qwen" => qwen_price(&model),
        "moonshot" | "kimi" => kimi_price(&model),
        _ => None,
    }
    .or_else(|| {
        if model.contains("free") {
            return None;
        }
        let model = model.rsplit('/').next().unwrap_or(&model);
        if prefix(model, "deepseek") {
            deepseek_price(model)
        } else if prefix(model, "glm") {
            zai_price(model)
        } else if prefix(model, "mimo") {
            mimo_price(model)
        } else if prefix(model, "qwen") {
            qwen_price(model)
        } else if prefix(model, "kimi") {
            kimi_price(model)
        } else {
            None
        }
    })
}

fn zai_price(model: &str) -> Option<Price> {
    if prefix(model, "glm-4.5-air") {
        Some((0.2, 1.1, 0.03, 0.0))
    } else if prefix(model, "glm-4.5v") {
        Some((0.6, 1.8, 0.11, 0.0))
    } else if prefix(model, "glm-4.7-flash") {
        Some((0.07, 0.4, 0.0, 0.0))
    } else if prefix(model, "glm-5.3-flash") {
        Some((0.15, 0.5, 0.03, 0.0))
    } else if prefix(model, "glm-5-turbo") {
        Some((1.2, 4.0, 0.24, 0.0))
    } else if prefix(model, "glm-5.3") {
        Some((1.4, 4.4, 0.14, 0.0))
    } else if prefix(model, "glm-5.2") || prefix(model, "glm-5.1") {
        Some((1.4, 4.4, 0.26, 0.0))
    } else if prefix(model, "glm-5") {
        Some((1.0, 3.2, 0.2, 0.0))
    } else if prefix(model, "glm-4.5") || prefix(model, "glm-4.6") || prefix(model, "glm-4.7") {
        Some((0.6, 2.2, 0.11, 0.0))
    } else {
        None
    }
}

fn mimo_price(model: &str) -> Option<Price> {
    if prefix(model, "mimo-v2.6-pro-ultraspeed") {
        Some((4.35, 8.7, 0.036, 0.0))
    } else if prefix(model, "mimo-v2.6-pro") || prefix(model, "mimo-v2.5-pro-ultraspeed") {
        Some((0.435, 0.87, 0.0036, 0.0))
    } else if prefix(model, "mimo-v2.5-pro") {
        Some((0.3045, 0.609, 0.0028, 0.0))
    } else if prefix(model, "mimo") {
        Some((0.14, 0.28, 0.0028, 0.0))
    } else {
        None
    }
}

fn qwen_price(model: &str) -> Option<Price> {
    if prefix(model, "qwen3-coder") {
        Some((0.3, 1.0, 0.1, 0.0))
    } else if prefix(model, "qwen3.8-max") || prefix(model, "qwen3.7-max") {
        Some((2.0, 6.0, 0.25, 0.0))
    } else if prefix(model, "qwen3.7-flash") {
        Some((0.03, 0.13, 0.006, 0.0))
    } else if prefix(model, "qwen3.8-flash") {
        Some((0.15, 0.47, 0.016, 0.0))
    } else {
        None
    }
}

fn kimi_price(model: &str) -> Option<Price> {
    if prefix(model, "kimi-k3") {
        Some((3.0, 15.0, 0.3, 0.0))
    } else if prefix(model, "kimi-k2.7-code-highspeed") {
        Some((1.9, 8.0, 0.38, 0.0))
    } else if prefix(model, "kimi-k2.7-code") {
        Some((0.95, 4.0, 0.19, 0.0))
    } else if prefix(model, "kimi-k2.6") {
        Some((0.95, 4.0, 0.16, 0.0))
    } else if prefix(model, "kimi-k2.5") {
        Some((0.6, 3.0, 0.1, 0.0))
    } else if prefix(model, "kimi-k2") || prefix(model, "kimi-for-coding") {
        Some((0.6, 2.5, 0.15, 0.0))
    } else {
        None
    }
}

fn deepseek_price(model: &str) -> Option<Price> {
    if prefix(model, "deepseek-v4-pro") {
        Some((1.32, 3.96, 0.044, 0.0))
    } else if prefix(model, "deepseek-v4-flash") {
        Some((0.14, 0.28, 0.028, 0.0))
    } else if prefix(model, "deepseek-v4.1") || prefix(model, "deepseek-flash") {
        Some((0.3, 1.2, 0.006, 0.0))
    } else {
        Some((0.27, 1.1, 0.07, 0.0))
    }
}

// ---------------------------------------------------------------------------
// Cache and shared helpers.

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Cache {
    version: u32,
    files: BTreeMap<String, CachedSession>,
    opencode: Option<CachedDb>,
    cursor: Option<CachedDb>,
    copilot: Option<CachedDb>,
    zcode: Option<CachedDb>,
    mimo: Option<CachedDb>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CachedSession {
    mtime: i64,
    size: u64,
    session: Session,
}

#[derive(Clone, Serialize, Deserialize)]
struct CachedDb {
    mtime: i64,
    size: u64,
    sessions: Vec<Session>,
}

impl Cache {
    fn load() -> Self {
        let Some(path) = cache_path() else {
            return Self::default();
        };
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let cache: Self = serde_json::from_str(&text).unwrap_or_default();
        if cache.version == SCAN_VERSION {
            cache
        } else {
            Self::default()
        }
    }
    fn save(&self) {
        let Some(path) = cache_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let cache = Self {
            version: SCAN_VERSION,
            files: self.files.clone(),
            opencode: self.opencode.clone(),
            cursor: self.cursor.clone(),
            copilot: self.copilot.clone(),
            zcode: self.zcode.clone(),
            mimo: self.mimo.clone(),
        };
        if let Ok(text) = serde_json::to_string(&cache) {
            let _ = fs::write(path, text);
        }
    }
}

fn cache_path() -> Option<PathBuf> {
    env_path("SESSIONTREE_CACHE").or_else(|| Some(app_dir()?.join("session-index.json")))
}

fn app_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let dir = home_dir().map(|home| home.join("Library/Application Support/Sessiontree"));
    #[cfg(windows)]
    let dir = env_path("LOCALAPPDATA")
        .or_else(|| env_path("APPDATA"))
        .map(|dir| dir.join("Sessiontree"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let dir = env_path("XDG_DATA_HOME")
        .or_else(|| home_dir().map(|home| home.join(".local/share")))
        .map(|dir| dir.join("sessiontree"));
    dir
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env_path("USERPROFILE").or_else(|| env_path("HOME"))
    }
    #[cfg(not(windows))]
    {
        env_path("HOME")
    }
}

fn modified_millis(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as i64)
        })
}

fn number(value: Option<&Value>) -> Option<i64> {
    value.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().map(|n| n as i64))
            .or_else(|| value.as_f64().map(|n| n as i64))
    })
}

fn earliest(current: i64, candidate: i64) -> i64 {
    if candidate <= 0 {
        current
    } else if current <= 0 {
        candidate
    } else {
        current.min(candidate)
    }
}

fn rfc3339_millis(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp_millis())
}

fn text_value(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    if let Some(parts) = value.as_array() {
        return parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.as_str())
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

fn estimate(
    provider: &str,
    model: &str,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
) -> f64 {
    price_for(provider, model)
        .map(|price| {
            (input as f64 * price.0
                + output as f64 * price.1
                + cache_read as f64 * price.2
                + cache_write as f64 * price.3)
                / 1_000_000.0
        })
        .unwrap_or(0.0)
}

fn cached_db(
    slot: &mut Option<CachedDb>,
    path: Option<PathBuf>,
    parse: fn(&Path) -> Result<Vec<Session>, String>,
    errors: &mut Vec<String>,
) -> (Vec<Session>, u64) {
    let Some(path) = path else {
        return (Vec::new(), 0);
    };
    let Ok(metadata) = fs::metadata(&path) else {
        return (Vec::new(), 0);
    };
    let mtime = modified_millis(&metadata);
    let size = metadata.len();
    if let Some(hit) = slot
        .as_ref()
        .filter(|hit| hit.mtime == mtime && hit.size == size)
    {
        return (hit.sessions.clone(), hit.sessions.len() as u64);
    }
    match parse(&path) {
        Ok(found) => {
            *slot = Some(CachedDb {
                mtime,
                size,
                sessions: found.clone(),
            });
            (found, 0)
        }
        Err(error) => {
            push_error(errors, error);
            (Vec::new(), 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sessiontree-test-{}-{}",
            std::process::id(),
            modified_millis(&fs::metadata(".").unwrap())
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn reads_a_claude_transcript() {
        let lines = [
            serde_json::json!({"type":"user","sessionId":"s1","cwd":"/tmp/project","timestamp":"2026-01-02T03:04:05Z","message":{"role":"user","content":"fix the tests"}}),
            serde_json::json!({"type":"assistant","sessionId":"s1","cwd":"/tmp/project","timestamp":"2026-01-02T03:04:06Z","message":{"model":"claude-sonnet-4-20250514","usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":3,"cache_creation_input_tokens":4},"content":[{"type":"text","text":"done"}]}}),
        ]
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let path = temp_file("claude.jsonl", &lines);
        let session = parse_claude_file(&path, 0).unwrap().expect("a session");
        assert_eq!(session.key, "claude:s1");
        assert_eq!(session.project, "/tmp/project");
        assert_eq!(session.title, "fix the tests");
        assert_eq!(session.model, "claude-sonnet-4-20250514");
        assert_eq!(session.messages, 2);
        assert_eq!(session.input, 10);
        assert_eq!(session.output, 20);
        assert_eq!(session.cache_read, 3);
        assert_eq!(session.cache_write, 4);
        assert!(session.cost > 0.0);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_a_codex_rollout() {
        let lines = [
            serde_json::json!({"type":"session_meta","timestamp":"2026-01-02T03:04:05Z","payload":{"id":"c1","cwd":"/tmp/codex"}}),
            serde_json::json!({"type":"turn_context","timestamp":"2026-01-02T03:04:06Z","payload":{"model":"gpt-5-codex","cwd":"/tmp/codex"}}),
            serde_json::json!({"type":"response_item","timestamp":"2026-01-02T03:04:07Z","payload":{"type":"message","role":"user","content":[{"type":"text","text":"build it"}]}}),
            serde_json::json!({"type":"response_item","timestamp":"2026-01-02T03:04:08Z","payload":{"type":"message","role":"assistant","content":[{"type":"text","text":"done"}]}}),
            serde_json::json!({"type":"event_msg","timestamp":"2026-01-02T03:04:09Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":20,"reasoning_output_tokens":5,"cached_input_tokens":40}}}}),
        ]
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let path = temp_file("codex.jsonl", &lines);
        let session = parse_codex_file(&path, 0).unwrap().expect("a session");
        assert_eq!(session.key, "codex:c1");
        assert_eq!(session.tool, "Codex");
        assert_eq!(session.project, "/tmp/codex");
        assert_eq!(session.title, "build it");
        assert_eq!(session.model, "gpt-5-codex");
        assert_eq!(session.messages, 2);
        assert_eq!(session.input, 100);
        assert_eq!(session.output, 25);
        assert_eq!(session.cache_read, 40);
        assert!(session.cost > 0.0);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_a_pi_session() {
        let lines = [
            serde_json::json!({"type":"session","timestamp":"2026-01-02T03:04:05Z","id":"p1","cwd":"/tmp/pi"}),
            serde_json::json!({"type":"model_change","timestamp":"2026-01-02T03:04:06Z","provider":"anthropic","modelId":"claude-sonnet-4-20250514"}),
            serde_json::json!({"type":"message","timestamp":"2026-01-02T03:04:07Z","message":{"role":"user","content":[{"type":"text","text":"hello pi"}]}}),
            serde_json::json!({"type":"message","timestamp":"2026-01-02T03:04:08Z","message":{"role":"assistant","provider":"anthropic","model":"claude-sonnet-4-20250514","usage":{"input":10,"output":20,"cacheRead":3,"cacheWrite":4,"cost":{"total":0.001}},"content":[{"type":"text","text":"hi"}]}}),
        ]
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let path = temp_file("pi.jsonl", &lines);
        let session = parse_pi_file(&path, 0).unwrap().expect("a session");
        assert_eq!(session.key, "pi:p1");
        assert_eq!(session.tool, "Pi");
        assert_eq!(session.project, "/tmp/pi");
        assert_eq!(session.title, "hello pi");
        assert_eq!(session.messages, 2);
        assert_eq!(session.input, 10);
        assert_eq!(session.output, 20);
        assert_eq!(session.cache_read, 3);
        assert!((session.cost - 0.001).abs() < 1e-9);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_a_cursor_database() {
        let path = temp_file("state.vscdb", "");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE composerHeaders (
                        composerId TEXT, createdAt INTEGER, lastUpdatedAt INTEGER,
                        isSubagent INTEGER, value TEXT
                     );
                     CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value TEXT);",
                )
                .unwrap();
            let header = serde_json::json!({
                "name":"Cursor fixture",
                "workspaceIdentifier":{"uri":{"fsPath":"/tmp/cursor-project"}}
            });
            connection
                .execute(
                    "INSERT INTO composerHeaders VALUES(?1,?2,?3,?4,?5)",
                    rusqlite::params![
                        "c1",
                        1_700_000_000_000_i64,
                        1_700_000_100_000_i64,
                        0_i64,
                        header.to_string()
                    ],
                )
                .unwrap();
            let composer = serde_json::json!({"conversation":[
                {"type":1,"text":"hello","bubbleId":"b1"},
                {"type":2,"text":"world","bubbleId":"b2",
                 "modelInfo":{"modelName":"claude-sonnet-4"},
                 "tokenCount":{"inputTokens":7,"outputTokens":9}}
            ]});
            connection
                .execute(
                    "INSERT INTO cursorDiskKV VALUES(?1,?2)",
                    rusqlite::params!["composerData:c1", composer.to_string()],
                )
                .unwrap();
        }
        let sessions = parse_cursor(&path).unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.key, "cursor:c1");
        assert_eq!(session.tool, "Cursor");
        assert_eq!(session.project, "/tmp/cursor-project");
        assert_eq!(session.title, "Cursor fixture");
        assert_eq!(session.messages, 2);
        assert_eq!(session.input, 7);
        assert_eq!(session.output, 9);
        assert_eq!(session.model, "claude-sonnet-4");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_a_copilot_database() {
        let path = temp_file("session-store.db", "");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE sessions (
                        id TEXT, cwd TEXT, repository TEXT, summary TEXT,
                        agent_name TEXT, created_at TEXT, updated_at TEXT
                     );
                     CREATE TABLE turns (
                        session_id TEXT, turn_index INTEGER, user_message TEXT,
                        assistant_response TEXT, timestamp TEXT
                     );",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO sessions VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    rusqlite::params![
                        "cp1",
                        "/tmp/copilot-project",
                        "",
                        "Copilot fixture",
                        "copilot",
                        "2026-01-02T03:04:05Z",
                        "2026-01-02T03:05:05Z"
                    ],
                )
                .unwrap();
            for (turn, user, assistant) in [(0_i64, "hello", "hi"), (1, "again", "done")] {
                connection
                    .execute(
                        "INSERT INTO turns VALUES(?1,?2,?3,?4,?5)",
                        rusqlite::params!["cp1", turn, user, assistant, "2026-01-02T03:04:06Z"],
                    )
                    .unwrap();
            }
        }
        let sessions = parse_copilot(&path).unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.key, "copilot:cp1");
        assert_eq!(session.tool, "Copilot");
        assert_eq!(session.project, "/tmp/copilot-project");
        assert_eq!(session.title, "Copilot fixture");
        assert_eq!(session.messages, 2);
        assert!(session.first_ts > 0);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_a_deepseek_harness_transcript() {
        let dir = std::env::temp_dir().join(format!("sessiontree-test-dsh-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.v4.jsonl");
        let lines = [
            serde_json::json!({"type":"session","version":4,"id":"d1","createdAt":1789827533179_i64,"cwd":"/tmp/dsh-project"}),
            serde_json::json!({"type":"user/message","seq":8,"time":1789827588662_i64,"data":{"content":[{"type":"text","text":"fix the parser"}]}}),
            serde_json::json!({"type":"assistant/message","seq":14,"time":1789827592406_i64,"data":{"message":{"role":"assistant","source":{"kind":"model","provider":"deepseek-official","model":"deepseek-v4-flash"}},"usage":{"inputTokens":29983,"outputTokens":461,"totalTokens":30444,"cacheReadTokens":0,"reasoningTokens":269}}}),
            serde_json::json!({"type":"tool/result","seq":16,"time":1789827592713_i64,"data":{"content":[{"type":"text","text":"ignored"}]}}),
            serde_json::json!({"type":"session/title","seq":13,"time":1789827588667_i64,"data":{"title":"Parser repair"}}),
        ]
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();
        let session = parse_dsh_file(&path, 0).unwrap().expect("a session");
        assert_eq!(session.key, "dsh:d1");
        assert_eq!(session.tool, "DeepSeek");
        assert_eq!(session.project, "/tmp/dsh-project");
        assert_eq!(session.title, "Parser repair");
        assert_eq!(session.model, "deepseek-v4-flash");
        assert_eq!(session.messages, 2);
        assert_eq!(session.input, 29983);
        assert_eq!(session.output, 461);
        assert!(session.first_ts > 0);
        assert!(session.cost > 0.0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn decompresses_a_deepseek_harness_log() {
        const FIXTURE: &[u8] = &[
            0x28, 0xb5, 0x2f, 0xfd, 0x24, 0xa8, 0xdd, 0x03, 0x00, 0x22, 0x88, 0x19, 0x1b, 0x70,
            0x6b, 0x73, 0xd0, 0x26, 0xd2, 0x5f, 0x64, 0x6a, 0xbb, 0xa9, 0x89, 0x32, 0x53, 0x4a,
            0xf8, 0xcf, 0x03, 0x26, 0x6c, 0x02, 0x33, 0x63, 0x55, 0xf0, 0xcf, 0x04, 0xc7, 0x7b,
            0x85, 0x67, 0xb5, 0xca, 0x00, 0x5c, 0x92, 0xfc, 0xe0, 0x44, 0x2f, 0xe3, 0x4b, 0x48,
            0x6f, 0xfb, 0x36, 0x97, 0xdc, 0xa4, 0xd1, 0x22, 0x65, 0x0c, 0xa4, 0xcf, 0xd6, 0x22,
            0xae, 0x46, 0xdf, 0x80, 0x54, 0xcf, 0xce, 0x72, 0x4d, 0x1c, 0xcf, 0xcc, 0x51, 0xd8,
            0x9e, 0x45, 0x32, 0x22, 0xe9, 0x1d, 0xa8, 0x2f, 0xfa, 0x92, 0x8c, 0x46, 0x18, 0xa7,
            0xd5, 0x32, 0x0a, 0x72, 0xb9, 0xc2, 0xe8, 0xe6, 0x96, 0xea, 0xd5, 0xe9, 0x2b, 0x8d,
            0x75, 0x13, 0x06, 0x00, 0x2a, 0xa3, 0xec, 0x55, 0x0d, 0x52, 0xaf, 0x4d, 0x42, 0x49,
            0x79, 0x46, 0xd6, 0xab, 0x88, 0x01, 0x43, 0xcc, 0x03, 0xb0,
        ];
        let dir =
            std::env::temp_dir().join(format!("sessiontree-test-dsh-zstd-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.v4.jsonl.zstd");
        fs::write(&path, FIXTURE).unwrap();
        let session = parse_dsh_file(&path, 0).unwrap().expect("a session");
        assert_eq!(session.key, "dsh:z1");
        assert_eq!(session.project, "/tmp/z");
        assert_eq!(session.title, "hello zstd");
        assert_eq!(session.messages, 1);

        let single = dsh_bytes(&path).unwrap();
        let doubled = dir.join("session.v3.jsonl.zstd");
        fs::write(&doubled, [FIXTURE, FIXTURE].concat()).unwrap();
        let twice = dsh_bytes(&doubled).unwrap();
        assert_eq!(twice, [single.clone(), single].concat());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn picks_the_newest_deepseek_session_version() {
        let root = std::env::temp_dir().join(format!(
            "sessiontree-test-dsh-versions-{}",
            std::process::id()
        ));
        let session = root.join("--tmp-project--").join("abc");
        fs::create_dir_all(&session).unwrap();
        for name in [
            "session.jsonl.zstd",
            "session.v2.jsonl.zstd",
            "session.v4.jsonl.zstd",
        ] {
            fs::write(session.join(name), b"").unwrap();
        }
        let files = dsh_files(&root, &mut Vec::new());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("session.v4.jsonl.zstd"));
        let _ = fs::remove_dir_all(root);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sessiontree-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_a_zcode_transcript() {
        let dir = temp_dir("zcode").join("-tmp-zcode");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session-1.jsonl");
        let lines = [
            serde_json::json!({"role":"user","sessionId":"s1","timestamp":1789827588662_i64,"content":"fix the parser"}),
            serde_json::json!({"role":"assistant","sessionId":"s1","timestamp":1789827592406_i64,"model":"glm-4.6","content":"done","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10}}),
        ]
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();
        let session = parse_zcode_file(&path, 0).unwrap().expect("a session");
        assert_eq!(session.key, "zcode:s1");
        assert_eq!(session.tool, "ZCode");
        assert_eq!(session.project, "/tmp/zcode");
        assert_eq!(session.title, "fix the parser");
        assert_eq!(session.model, "glm-4.6");
        assert_eq!(session.messages, 2);
        assert_eq!(session.input, 100);
        assert_eq!(session.output, 50);
        assert_eq!(session.cache_read, 10);
        assert!(session.cost > 0.0);
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn reads_a_zcode_legacy_snapshot() {
        let dir = temp_dir("zcode-legacy");
        let path = dir.join("task-1.json");
        let document = serde_json::json!({
            "meta": {
                "acpSessionId": "legacy-1",
                "workspacePath": "/tmp/legacy",
                "title": "Legacy task",
                "createdAt": 1789827588000_i64,
                "updatedAt": 1789827592000_i64
            },
            "messages": [
                {"role":"user","content":"hello","timestamp":1789827588000_i64},
                {"role":"assistant","content":"hi","timestamp":1789827592000_i64,"model":"glm-4.7","usage":{"inputTokens":10,"outputTokens":5}}
            ]
        });
        fs::write(&path, document.to_string()).unwrap();
        let session = parse_zcode_legacy_file(&path, 0)
            .unwrap()
            .expect("a session");
        assert_eq!(session.key, "zcode:legacy-1");
        assert_eq!(session.project, "/tmp/legacy");
        assert_eq!(session.title, "Legacy task");
        assert_eq!(session.messages, 2);
        assert_eq!(session.input, 10);
        assert_eq!(session.output, 5);
        assert!(session.cost > 0.0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_a_mimo_database() {
        let dir = temp_dir("mimo-db");
        let path = dir.join("mimocode.db");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE session (
                        id TEXT, title TEXT, directory TEXT,
                        time_created INTEGER, time_updated INTEGER, cost REAL,
                        tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER,
                        tokens_cache_read INTEGER, tokens_cache_write INTEGER, model TEXT
                     );
                     CREATE TABLE message (session_id TEXT);",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO session VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    rusqlite::params![
                        "m1",
                        "MiMo session",
                        "/tmp/mimo",
                        1_700_000_000_000_i64,
                        1_700_000_100_000_i64,
                        0.0,
                        1000_i64,
                        200_i64,
                        50_i64,
                        100_i64,
                        0_i64,
                        "{\"id\":\"mimo-v2.6-flash\",\"providerID\":\"xiaomi\"}"
                    ],
                )
                .unwrap();
            for _ in 0..3 {
                connection
                    .execute("INSERT INTO message VALUES('m1')", [])
                    .unwrap();
            }
        }
        let sessions = parse_mimo(&path).unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.key, "mimo:m1");
        assert_eq!(session.tool, "MiMo");
        assert_eq!(session.project, "/tmp/mimo");
        assert_eq!(session.model, "mimo-v2.6-flash");
        assert_eq!(session.messages, 3);
        assert_eq!(session.input, 1000);
        assert_eq!(session.output, 250);
        assert!(session.cost > 0.0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_a_mimo_v2_database() {
        let dir = temp_dir("mimo-v2-db");
        let path = dir.join("mimocode.db");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE session_v2 (
                        id TEXT, title TEXT, directory TEXT,
                        time_created INTEGER, time_updated INTEGER
                     );
                     CREATE TABLE session_message (
                        session_id TEXT, type TEXT, time_created INTEGER, data TEXT
                     );",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO session_v2 VALUES(?1,?2,?3,?4,?5)",
                    rusqlite::params![
                        "v2",
                        "MiMo v2",
                        "/tmp/mimo-v2",
                        1_700_000_000_000_i64,
                        1_700_000_100_000_i64
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO session_message VALUES(?1,'user',?2,?3)",
                    rusqlite::params!["v2", 1_700_000_000_000_i64, "{\"content\":\"hello\"}"],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO session_message VALUES(?1,'assistant',?2,?3)",
                    rusqlite::params![
                        "v2",
                        1_700_000_050_000_i64,
                        "{\"modelID\":\"mimo-v2.6-flash\",\"tokens\":{\"input\":100,\"output\":20,\"reasoning\":5}}"
                    ],
                )
                .unwrap();
        }
        let sessions = parse_mimo(&path).unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.key, "mimo:v2");
        assert_eq!(session.messages, 2);
        assert_eq!(session.input, 100);
        assert_eq!(session.output, 25);
        assert_eq!(session.model, "mimo-v2.6-flash");
        assert!(session.cost > 0.0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_an_antigravity_transcript() {
        let root = temp_dir("antigravity");
        let logs = root
            .join("brain")
            .join("sess-a")
            .join(".system_generated")
            .join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::write(
            root.join("cache").join("conversation_metadata.json"),
            serde_json::json!({
                "conversations": {
                    "sess-a": {"summary": {"WorkspaceURIs": ["file:///work/api"]}}
                }
            })
            .to_string(),
        )
        .unwrap();
        let path = logs.join("transcript.jsonl");
        let lines = [
            serde_json::json!({"source":"USER_EXPLICIT","created_at":"2026-07-17T09:00:00Z","content":"<USER_REQUEST>Check the build.<ADDITIONAL_METADATA>{\"cwd\":\"/work/api\"}</ADDITIONAL_METADATA></USER_REQUEST>"}),
            serde_json::json!({"source":"MODEL","type":"PLANNER_RESPONSE","created_at":"2026-07-17T09:00:05Z","content":"Building now."}),
            serde_json::json!({"source":"MODEL","type":"RUN_COMMAND","created_at":"2026-07-17T09:00:06Z","content":"Task Description: cargo build"}),
        ]
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();
        let session = parse_antigravity_file(&path, 0)
            .unwrap()
            .expect("a session");
        assert_eq!(session.key, "antigravity:sess-a");
        assert_eq!(session.tool, "Antigravity");
        assert_eq!(session.project, "/work/api");
        assert_eq!(session.title, "Check the build.");
        assert_eq!(session.messages, 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_a_kiro_cli_transcript() {
        let root = temp_dir("kiro-cli");
        let cli = root.join("cli");
        fs::create_dir_all(&cli).unwrap();
        fs::write(
            cli.join("k1.json"),
            serde_json::json!({"session_id":"k1","cwd":"/tmp/kiro","model":"claude-sonnet-4"})
                .to_string(),
        )
        .unwrap();
        let path = cli.join("k1.jsonl");
        let lines = [
            serde_json::json!({"kind":"Prompt","data":{"content":[{"kind":"text","data":"fix it"}],"meta":{"timestamp":1789827588}}}),
            serde_json::json!({"kind":"AssistantMessage","data":{"message_id":"a1","content":[{"kind":"text","data":"done"}],"meta":{"timestamp":1789827589}}}),
            serde_json::json!({"kind":"AssistantMessage","data":{"message_id":"a1","content":[{"kind":"text","data":" and tested"}],"meta":{"timestamp":1789827590}}}),
        ]
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();
        let session = parse_kiro_cli_file(&path, 0).unwrap().expect("a session");
        assert_eq!(session.key, "kiro:k1");
        assert_eq!(session.tool, "Kiro");
        assert_eq!(session.project, "/tmp/kiro");
        assert_eq!(session.title, "fix it");
        assert_eq!(session.model, "claude-sonnet-4");
        assert_eq!(session.messages, 2);
        assert!(session.first_ts > 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_a_kiro_ide_transcript() {
        let root = temp_dir("kiro-ide");
        let dir = root.join("ws").join("sess_1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("session.json"),
            serde_json::json!({"id":"sess_1","workspacePaths":["/tmp/kiro-ide"],"modelId":"claude-sonnet-4","createdAt":"2026-07-17T09:00:00Z"}).to_string(),
        )
        .unwrap();
        let path = dir.join("messages.jsonl");
        let lines = [
            serde_json::json!({"timestamp":"2026-07-17T09:00:00Z","payload":{"type":"user","content":"add tests"}}),
            serde_json::json!({"timestamp":"2026-07-17T09:00:05Z","payload":{"type":"assistant","content":"added"}}),
            serde_json::json!({"timestamp":"2026-07-17T09:00:06Z","payload":{"type":"usage_summary"}}),
        ]
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();
        let session = parse_kiro_ide_file(&path, 0).unwrap().expect("a session");
        assert_eq!(session.key, "kiro:sess_1");
        assert_eq!(session.project, "/tmp/kiro-ide");
        assert_eq!(session.title, "add tests");
        assert_eq!(session.model, "claude-sonnet-4");
        assert_eq!(session.messages, 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dedupes_sessions_by_key() {
        let base = Session {
            key: "zcode:x".into(),
            messages: 1,
            ..Session::default()
        };
        let richer = Session {
            key: "zcode:x".into(),
            messages: 4,
            input: 100,
            ..Session::default()
        };
        let sessions = dedupe_sessions(vec![base, richer]);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].messages, 4);
    }

    #[test]
    fn unknown_models_get_no_invented_price() {
        assert!(price_for("opencode", "deepseek-v4.1-flash-free").is_none());
        assert!(price_for("anthropic", "claude-sonnet-4").is_some());
        assert!(price_for("anthropic", "deepseek-v4-flash").is_some());
        assert!(price_for("deepseek-official", "deepseek-flash").is_some());
    }
}
