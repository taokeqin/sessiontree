//! Application state. Reading transcripts never runs on the UI thread.
use crate::{
    capture,
    model::*,
    scan,
    share::{self, ShareOptions},
};
use nsh_core::{prelude::*, storage::Storage};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(crate) struct State {
    pub owner: Owner,
    pub raw: Signal<Snapshot>,
    pub snapshot: Signal<Snapshot>,
    pub scope: Signal<String>,
    pub selection: Signal<Node>,
    pub metric: Signal<Metric>,
    pub levels: Signal<u8>,
    pub busy: Signal<bool>,
    pub message: Signal<String>,
    pub progress: Signal<Option<Progress>>,
    pub hovered: Signal<Vec<Node>>,
    pub in_map: Signal<bool>,
    pub generation: Signal<u64>,
    pub seconds: Signal<u64>,
    pub share_open: Signal<bool>,
    pub share_agents: Signal<bool>,
    pub share_projects: Signal<bool>,
    pub share_sessions: Signal<bool>,
    pub share_numbers: Signal<bool>,
    pub share_busy: Signal<bool>,
    pub share_status: Signal<String>,
    pub map_frame: Signal<(f32, f32)>,
    pub query: Signal<String>,
    pub project_query: Signal<String>,
    pub compare_open: Signal<bool>,
    pub range: Signal<TimeRange>,
    pub custom_open: Signal<bool>,
    pub custom_start: Signal<String>,
    pub custom_end: Signal<String>,
    pub custom_error: Signal<String>,
}

const RANGE_KEY: &str = "sessiontree.range";

pub(crate) fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn local_date(ts: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts)
        .map(|time| time.with_timezone(&chrono::Local))
        .map(|time| time.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

impl State {
    pub fn new() -> Self {
        Self {
            owner: Owner::current(),
            raw: signal(Snapshot::default()),
            snapshot: signal(Snapshot::default()),
            scope: signal("root".into()),
            selection: signal(Node::default()),
            metric: signal(Metric::Tokens),
            levels: signal(3),
            busy: signal(false),
            message: signal("Looking for local agent sessions…".into()),
            progress: signal(None),
            hovered: signal(Vec::new()),
            in_map: signal(false),
            generation: signal(0),
            seconds: signal(0),
            share_open: signal(false),
            share_agents: signal(false),
            share_projects: signal(false),
            share_sessions: signal(false),
            share_numbers: signal(false),
            share_busy: signal(false),
            share_status: signal("Captures this window. Nothing leaves this machine.".into()),
            map_frame: signal((700.0, 520.0)),
            query: signal(String::new()),
            project_query: signal(String::new()),
            compare_open: signal(false),
            range: signal(
                Storage::get_item(RANGE_KEY)
                    .and_then(|value| TimeRange::from_storage(&value))
                    .unwrap_or_default(),
            ),
            custom_open: signal(false),
            custom_start: signal(String::new()),
            custom_end: signal(String::new()),
            custom_error: signal(String::new()),
        }
    }

    pub fn current(self) -> Node {
        let snapshot = self.snapshot.get();
        let scope = self.scope.get();
        find(&snapshot.root, &scope)
            .cloned()
            .unwrap_or(snapshot.root)
    }

    pub fn scan(self) {
        if self.busy.get_untracked() {
            return;
        }
        let run = self.generation.get_untracked() + 1;
        batch(|| {
            self.generation.set(run);
            self.busy.set(true);
            self.seconds.set(0);
            self.message
                .set("Indexing Claude Code and OpenCode history…".into());
            self.progress.set(Some(Progress::default()));
        });
        self.owner.run(move || {
            spawn(async move {
                let started = Instant::now();
                while self.busy.get_untracked() && self.generation.get_untracked() == run {
                    timer(Duration::from_millis(160)).await;
                    if self.generation.get_untracked() != run || !self.busy.get_untracked() {
                        break;
                    }
                    batch(|| {
                        self.seconds.set(started.elapsed().as_secs());
                        self.progress.set(Some(scan::progress()));
                    });
                }
            })
        });
        self.owner.run(move || {
            spawn(async move {
                let result = blocking(scan::scan)
                    .await
                    .map_err(|error| error.to_string());
                if self.generation.get_untracked() != run {
                    return;
                }
                batch(|| {
                    match result {
                        Ok(snapshot) => {
                            let root = snapshot.root.clone();
                            self.message.set(format!(
                                "Updated in {} ms · {} cached files",
                                snapshot.scanned_ms,
                                count(snapshot.cache_hits),
                            ));
                            self.raw.set(snapshot);
                            let filtered = self.filtered();
                            self.snapshot.set(filtered);
                            self.scope.set(root.path.clone());
                            self.selection.set(root);
                            self.hovered.set(Vec::new());
                        }
                        Err(error) => self.message.set(error),
                    }
                    self.busy.set(false);
                    self.progress.set(None);
                });
            })
        });
    }

    fn filtered(self) -> Snapshot {
        let raw = self.raw.get_untracked();
        raw.filtered(self.range.get_untracked(), now_millis())
    }

    /// Changes the period and rebuilds the visible tree without rescanning.
    pub fn set_range(self, range: TimeRange) {
        if self.range.get_untracked() == range {
            return;
        }
        self.range.set(range);
        let _ = Storage::set_item(RANGE_KEY, range.storage_value());
        let snapshot = self.filtered();
        let scope = self.scope.get_untracked();
        let selection = self.selection.get_untracked();
        let root = snapshot.root.clone();
        let scope_node = find(&root, &scope).cloned().unwrap_or_else(|| root.clone());
        let selection_node = find(&root, &selection.path)
            .cloned()
            .unwrap_or_else(|| scope_node.clone());
        batch(|| {
            self.snapshot.set(snapshot);
            self.scope.set(scope_node.path);
            self.selection.set(selection_node);
            self.hovered.set(Vec::new());
        });
    }

    pub fn open_custom(self) {
        let range = self.range.get_untracked();
        let now = now_millis();
        let (start, end) = match range {
            TimeRange::Custom { start, end } => (start, end),
            TimeRange::Last7 => (now - 6 * 86_400_000, now),
            TimeRange::Last90 => (now - 89 * 86_400_000, now),
            _ => (now - 29 * 86_400_000, now),
        };
        batch(|| {
            self.custom_start.set(local_date(start));
            self.custom_end.set(local_date(end));
            self.custom_error.set(String::new());
            self.custom_open.set(true);
        });
    }

    pub fn apply_custom(self) {
        match parse_custom_range(
            &self.custom_start.get_untracked(),
            &self.custom_end.get_untracked(),
        ) {
            Ok(range) => {
                self.custom_open.set(false);
                self.set_range(range);
            }
            Err(error) => self.custom_error.set(error),
        }
    }

    pub fn choose(self, node: Node) {
        if node.kind == "session" {
            self.selection.set(node);
            return;
        }
        if self.selection.get_untracked().path == node.path {
            self.enter(node);
        } else {
            if node.kind == "project" && self.selection.get_untracked().path != node.path {
                self.project_query.set(String::new());
            }
            self.selection.set(node);
        }
    }

    pub fn enter(self, node: Node) {
        if node.kind == "session" {
            self.selection.set(node);
            return;
        }
        batch(|| {
            self.scope.set(node.path.clone());
            self.selection.set(node);
            self.hovered.set(Vec::new());
        });
    }

    pub fn navigate(self, path: String) {
        if let Some(node) = find(&self.snapshot.get_untracked().root, &path).cloned() {
            self.enter(node);
        }
    }

    pub fn up(self) {
        let trail = ancestors(
            &self.snapshot.get_untracked().root,
            &self.scope.get_untracked(),
        );
        if trail.len() > 1 {
            let parent = trail[trail.len() - 2].clone();
            self.enter(parent);
        }
    }

    /// Esc: step back to the scope first, then one level up.
    pub fn close(self) {
        let current = self.current();
        if self.selection.get_untracked().path != current.path {
            self.selection.set(current);
        } else {
            self.up();
        }
    }

    pub fn share_options(self) -> ShareOptions {
        ShareOptions {
            mask_agents: self.share_agents.get_untracked(),
            mask_projects: self.share_projects.get_untracked(),
            mask_sessions: self.share_sessions.get_untracked(),
            mask_numbers: self.share_numbers.get_untracked(),
        }
    }

    pub fn copy_share(self) {
        if self.share_busy.get_untracked() {
            return;
        }
        let snapshot = self.snapshot.get_untracked();
        let root = snapshot.root.clone();
        let node = self.current();
        let selection = self.selection.get_untracked();
        let metric = self.metric.get_untracked();
        let levels = self.levels.get_untracked();
        let frame = self.map_frame.get_untracked();
        let options = self.share_options();
        let masked = options.mask_agents || options.mask_projects || options.mask_sessions;
        let redacted_root = share::redacted(&root, options);
        let redacted_selection = find(&redacted_root, &selection.path)
            .cloned()
            .unwrap_or_else(|| redacted_root.clone());
        let redacted_snapshot = Snapshot {
            root: redacted_root,
            ..snapshot.clone()
        };
        let original = snapshot;
        batch(|| {
            self.share_busy.set(true);
            self.share_status.set("Capturing the window…".into());
            self.message.set("Updated just now".into());
            self.share_open.set(false);
            if masked {
                self.snapshot.set(redacted_snapshot);
                self.selection.set(redacted_selection);
            }
            set_hide_numbers(options.mask_numbers);
            self.hovered.set(Vec::new());
        });
        self.owner.run(move || {
            spawn(async move {
                // Let the sheet dismiss and the redacted UI draw before the
                // window is read back.
                timer(Duration::from_millis(450)).await;
                let captured = run_on_main_thread(capture::window_png).await;
                let mut error = String::new();
                let mut captured_window = false;
                let mut png = match captured {
                    Ok(Ok(bytes)) => {
                        captured_window = true;
                        Some(bytes)
                    }
                    Ok(Err(reason)) => {
                        error = reason;
                        None
                    }
                    Err(reason) => {
                        error = reason.to_string();
                        None
                    }
                };
                batch(|| {
                    set_hide_numbers(false);
                    if masked {
                        self.snapshot.set(original.clone());
                        self.selection.set(selection);
                    }
                });
                if png.is_none() {
                    let (painter, height) =
                        share::painter(&node, &original.root, metric, levels, options, frame);
                    match share::render(painter, height).await {
                        Ok(bytes) => png = Some(bytes),
                        Err(reason) => {
                            error = format!("{error}; {reason:?}");
                        }
                    }
                }
                batch(|| {
                    match png {
                        Some(bytes) => {
                            set_clipboard_image(share::data_uri(&bytes));
                            let size = if bytes.len() >= 1_048_576 {
                                format!("{:.1} MB", bytes.len() as f64 / 1_048_576.0)
                            } else {
                                format!("{} KB", (bytes.len() / 1024).max(1))
                            };
                            if captured_window {
                                self.message
                                    .set(format!("Copied a {size} screenshot to the clipboard."));
                            } else {
                                self.message.set(format!(
                                    "Copied a {size} share image (window capture is unavailable here)."
                                ));
                            }
                        }
                        None => {
                            self.message
                                .set(format!("Could not capture the window: {error}"));
                        }
                    }
                    self.share_status
                        .set("Ready to copy. Nothing leaves this machine.".into());
                    self.share_busy.set(false);
                });
            })
        });
    }

    pub fn hover(self, node: Node, inside: bool) {
        self.hovered.update(|nodes| {
            nodes.retain(|n| n.path != node.path);
            if inside {
                nodes.push(node);
            }
        });
    }
}
