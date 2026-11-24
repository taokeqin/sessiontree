//! Sessions, aggregates, and the platform-independent treemap geometry.
use chrono::{Datelike, Local, NaiveDate, TimeZone, Timelike};
use nsh_core::t;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// One conversation read from an agent's own on-disk history.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Session {
    pub key: String,
    pub tool: String,
    pub project: String,
    pub title: String,
    pub model: String,
    pub messages: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: f64,
    pub first_ts: i64,
    pub last_ts: i64,
    pub file: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Node {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub tool: String,
    pub model: String,
    pub title: String,
    pub file: String,
    pub sessions: u64,
    pub messages: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: f64,
    pub first_ts: i64,
    pub last_ts: i64,
    pub children: Vec<Arc<Node>>,
}

impl Node {
    pub fn tokens(&self) -> u64 {
        self.input + self.output
    }
    pub fn cache(&self) -> u64 {
        self.cache_read + self.cache_write
    }
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Metric {
    #[default]
    Tokens,
    Cost,
    Messages,
}

impl Metric {
    pub fn weight(self, node: &Node) -> f64 {
        (match self {
            Self::Tokens => node.tokens() as f64,
            Self::Cost => node.cost,
            Self::Messages => node.messages as f64,
        })
        .max(1e-6)
    }
    pub fn label(self) -> String {
        match self {
            Self::Tokens => t!("metric.tokens"),
            Self::Cost => t!("metric.cost"),
            Self::Messages => t!("metric.messages"),
        }
    }
    pub fn hint(self) -> String {
        match self {
            Self::Tokens => t!("metric.tokens_hint"),
            Self::Cost => t!("metric.cost_hint"),
            Self::Messages => t!("metric.messages_hint"),
        }
    }
    pub fn amount(self, node: &Node) -> f64 {
        match self {
            Self::Tokens => node.tokens() as f64,
            Self::Cost => node.cost,
            Self::Messages => node.messages as f64,
        }
    }
    pub fn value(self, node: &Node) -> String {
        match self {
            Self::Tokens => compact(node.tokens()),
            Self::Cost => cost_text(node),
            Self::Messages => count(node.messages),
        }
    }
}

/// The period the treemap and its totals cover, by a session's last activity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TimeRange {
    #[default]
    All,
    Last1,
    Last7,
    Last30,
    Last90,
    Custom {
        start: i64,
        end: i64,
    },
}

impl TimeRange {
    pub fn label(self) -> String {
        match self {
            Self::All => t!("range.all"),
            Self::Last1 => t!("range.1d"),
            Self::Last7 => t!("range.7d"),
            Self::Last30 => t!("range.30d"),
            Self::Last90 => t!("range.90d"),
            Self::Custom { .. } => t!("range.custom"),
        }
    }

    pub fn title(self) -> String {
        match self {
            Self::All => t!("range.title.all"),
            Self::Last1 => t!("range.title.1d"),
            Self::Last7 => t!("range.title.7d"),
            Self::Last30 => t!("range.title.30d"),
            Self::Last90 => t!("range.title.90d"),
            Self::Custom { .. } => t!("range.title.custom"),
        }
    }

    pub fn is_all(self) -> bool {
        matches!(self, Self::All)
    }

    pub fn contains(self, last_ts: i64, now: i64) -> bool {
        match self {
            Self::All => true,
            Self::Last1 => last_ts >= local_day_start(now, 0),
            Self::Last7 => last_ts >= local_day_start(now, 6),
            Self::Last30 => last_ts >= local_day_start(now, 29),
            Self::Last90 => last_ts >= local_day_start(now, 89),
            Self::Custom { start, end } => last_ts >= start && last_ts <= end,
        }
    }

    pub fn chart_days(self) -> usize {
        match self {
            Self::All => 30,
            Self::Last1 => 1,
            Self::Last7 => 7,
            Self::Last30 => 30,
            Self::Last90 => 90,
            Self::Custom { start, end } => {
                (((end.saturating_sub(start)) / 86_400_000) + 1).clamp(1, 90) as usize
            }
        }
    }

    pub fn chart_end(self, now: i64) -> i64 {
        match self {
            Self::Custom { end, .. } => end,
            _ => now,
        }
    }

    pub fn storage_value(self) -> String {
        match self {
            Self::All => "all".into(),
            Self::Last1 => "1d".into(),
            Self::Last7 => "7d".into(),
            Self::Last30 => "30d".into(),
            Self::Last90 => "90d".into(),
            Self::Custom { start, end } => format!("custom:{start}:{end}"),
        }
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "1d" => Some(Self::Last1),
            "7d" => Some(Self::Last7),
            "30d" => Some(Self::Last30),
            "90d" => Some(Self::Last90),
            _ => {
                let rest = value.strip_prefix("custom:")?;
                let (start, end) = rest.split_once(':')?;
                let start = start.parse().ok()?;
                let end = end.parse().ok()?;
                (start > 0 && end >= start).then_some(Self::Custom { start, end })
            }
        }
    }
}

/// The local midnight `days_back` days before `now`.
fn local_day_start(now: i64, days_back: i64) -> i64 {
    let date = chrono::DateTime::from_timestamp_millis(now)
        .map(|time| time.with_timezone(&Local).date_naive())
        .unwrap_or_else(|| Local::now().date_naive());
    date.checked_sub_signed(chrono::Duration::days(days_back))
        .unwrap_or(date)
        .and_hms_opt(0, 0, 0)
        .and_then(|time| Local.from_local_datetime(&time).single())
        .map(|time| time.timestamp_millis())
        .unwrap_or(now)
}

/// Parses two local `YYYY-MM-DD` dates into an inclusive custom range.
pub(crate) fn parse_custom_range(start: &str, end: &str) -> Result<TimeRange, String> {
    let parse = |value: &str, label_key: &str| {
        NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
            .map_err(|_| t!("range.error.format", label = t!(label_key)))
    };
    let first = parse(start, "range.error.start")?;
    let last = parse(end, "range.error.end")?;
    let (first, last) = if first <= last {
        (first, last)
    } else {
        (last, first)
    };
    let start = first
        .and_hms_opt(0, 0, 0)
        .and_then(|time| Local.from_local_datetime(&time).single())
        .ok_or_else(|| t!("range.error.start_invalid"))?
        .timestamp_millis();
    let end = last
        .and_hms_milli_opt(23, 59, 59, 999)
        .and_then(|time| Local.from_local_datetime(&time).single())
        .ok_or_else(|| t!("range.error.end_invalid"))?
        .timestamp_millis();
    Ok(TimeRange::Custom { start, end })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Snapshot {
    pub root: Node,
    pub errors: Vec<String>,
    pub scanned_ms: u64,
    pub cache_hits: u64,
}

impl Snapshot {
    /// The same tree with sessions whose last activity is outside `range`
    /// removed, and every parent's totals recomputed.
    pub fn filtered(&self, range: TimeRange, now: i64) -> Snapshot {
        if range.is_all() {
            return self.clone();
        }
        let root = filter_node(&self.root, range, now).unwrap_or_else(|| Node {
            path: "root".into(),
            name: "All sessions".into(),
            kind: "root".into(),
            ..Default::default()
        });
        Snapshot {
            root,
            errors: self.errors.clone(),
            scanned_ms: self.scanned_ms,
            cache_hits: self.cache_hits,
        }
    }
}

fn filter_node(node: &Node, range: TimeRange, now: i64) -> Option<Node> {
    if node.kind == "session" {
        return range.contains(node.last_ts, now).then(|| node.clone());
    }
    let children: Vec<Arc<Node>> = node
        .children
        .iter()
        .filter_map(|child| filter_node(child, range, now).map(Arc::new))
        .collect();
    if children.is_empty() && node.kind != "root" {
        return None;
    }
    let mut parent = node.clone();
    parent.children = children;
    refresh_parent(&mut parent);
    Some(parent)
}

fn refresh_parent(node: &mut Node) {
    if node.kind == "session" {
        return;
    }
    node.sessions = 0;
    node.messages = 0;
    node.input = 0;
    node.output = 0;
    node.cache_read = 0;
    node.cache_write = 0;
    node.cost = 0.0;
    node.first_ts = 0;
    node.last_ts = 0;
    node.model.clear();
    for child in &node.children {
        node.sessions += child.sessions;
        node.messages += child.messages;
        node.input += child.input;
        node.output += child.output;
        node.cache_read += child.cache_read;
        node.cache_write += child.cache_write;
        node.cost += child.cost;
        node.first_ts = earliest(node.first_ts, child.first_ts);
        node.last_ts = node.last_ts.max(child.last_ts);
        if node.model.is_empty() && !child.model.is_empty() {
            node.model = child.model.clone();
        }
    }
}

/// One value per local day, oldest first, summing the sessions under `node`
/// by their last activity. `end` is the day the last bucket covers.
pub(crate) fn daily_series(node: &Node, metric: Metric, days: usize, end: i64) -> Vec<f64> {
    let days = days.max(1);
    let mut series = vec![0.0; days];
    let end_date = chrono::DateTime::from_timestamp_millis(end)
        .map(|time| time.with_timezone(&Local).date_naive())
        .unwrap_or_else(|| Local::now().date_naive());
    let start = end_date
        .checked_sub_signed(chrono::Duration::days(days as i64 - 1))
        .unwrap_or(end_date);
    let start_day = start.num_days_from_ce();
    fn walk(node: &Node, metric: Metric, start_day: i32, days: usize, series: &mut [f64]) {
        if node.kind == "session" {
            let Some(date) = chrono::DateTime::from_timestamp_millis(node.last_ts)
                .map(|time| time.with_timezone(&Local).date_naive())
            else {
                return;
            };
            let index = date.num_days_from_ce() - start_day;
            if index >= 0 && (index as usize) < days {
                series[index as usize] += metric.amount(node);
            }
            return;
        }
        for child in &node.children {
            walk(child, metric, start_day, days, series);
        }
    }
    walk(node, metric, start_day, days, &mut series);
    series
}

/// One value per local hour of `end`'s day, midnight first. A single-day
/// range uses this instead of one useless daily bar.
pub(crate) fn hourly_series(node: &Node, metric: Metric, end: i64) -> Vec<f64> {
    let mut series = vec![0.0; 24];
    let day = chrono::DateTime::from_timestamp_millis(end)
        .map(|time| time.with_timezone(&Local).date_naive())
        .unwrap_or_else(|| Local::now().date_naive());
    let start = day
        .and_hms_opt(0, 0, 0)
        .and_then(|time| Local.from_local_datetime(&time).single())
        .map(|time| time.timestamp_millis())
        .unwrap_or(end);
    let next = day.succ_opt().unwrap_or(day);
    let stop = next
        .and_hms_opt(0, 0, 0)
        .and_then(|time| Local.from_local_datetime(&time).single())
        .map(|time| time.timestamp_millis())
        .unwrap_or(start + 86_400_000);
    fn walk(node: &Node, metric: Metric, start: i64, stop: i64, series: &mut [f64]) {
        if node.kind == "session" {
            if node.last_ts >= start
                && node.last_ts < stop
                && let Some(hour) = chrono::DateTime::from_timestamp_millis(node.last_ts)
                    .map(|time| time.with_timezone(&Local).hour())
            {
                series[hour as usize] += metric.amount(node);
            }
            return;
        }
        for child in &node.children {
            walk(child, metric, start, stop, series);
        }
    }
    walk(node, metric, start, stop, &mut series);
    series
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Progress {
    pub done: u64,
    pub total: u64,
    pub sessions: u64,
    pub phase: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tile {
    pub node: Arc<Node>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub depth: u8,
}

const MASK_COLORS: [u32; 9] = [
    0xe08b55, 0x63d2a4, 0x7aa7e8, 0xb48ce0, 0x6fc2cf, 0xe0c765, 0x5b8def, 0xf0a35e, 0x9bd86a,
];

pub(crate) fn agent_color(tool: &str) -> u32 {
    if let Some(index) = tool
        .strip_prefix("Agent ")
        .and_then(|number| number.parse::<usize>().ok())
    {
        return MASK_COLORS[(index - 1) % MASK_COLORS.len()];
    }
    match tool {
        "Claude Code" => 0xe08b55,
        "OpenCode" => 0x63d2a4,
        "Codex" => 0x7aa7e8,
        "Cursor" => 0xb48ce0,
        "Copilot" => 0x6fc2cf,
        "Pi" => 0xe0c765,
        "DeepSeek" => 0x5b8def,
        "ZCode" => 0x8a7cf0,
        "MiMo" => 0xf0a35e,
        "Antigravity" => 0x9bd86a,
        "Kiro" => 0xff8a65,
        _ => 0x8d93a1,
    }
}

/// The agent's hue identifies the tool; depth and name only tint it.
pub(crate) fn tile_color(node: &Node, depth: u8) -> u32 {
    let base = agent_color(&node.tool);
    let hash = node
        .name
        .encode_utf16()
        .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32));
    let jitter = (hash.unsigned_abs() % 13) as i32 - 6;
    let lift = (depth.saturating_sub(1) as i32) * 5 + jitter;
    let channel =
        |shift: u32| ((((base >> shift) & 255) as i32 + lift).clamp(0, 255) as u32) << shift;
    channel(16) | channel(8) | channel(0)
}

/// Balanced subdivision preserves area and bounds the work to the supplied view.
/// Keep the map readable and the element count bounded on very large projects.
pub(crate) const MAP_BUDGET: usize = 240;

/// The children the map and the contents list should show. A project with
/// thousands of sessions contributes its largest ones; the parent tile still
/// carries the project's full weight, so the outer map remains truthful while
/// the inner view stays readable.
pub(crate) fn bounded_children(node: &Node, metric: Metric) -> Vec<Arc<Node>> {
    if node.children.len() <= MAP_BUDGET {
        return node.children.clone();
    }
    let mut children = node.children.clone();
    children.sort_by(|a, b| {
        metric
            .weight(b)
            .partial_cmp(&metric.weight(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    children.truncate(MAP_BUDGET);
    children
}

pub(crate) fn mosaic(
    nodes: &[Arc<Node>],
    width: f32,
    height: f32,
    metric: Metric,
    levels: u8,
) -> Vec<Tile> {
    fn ordered(nodes: &[Arc<Node>], metric: Metric) -> Vec<&Arc<Node>> {
        let mut nodes: Vec<_> = nodes.iter().filter(|n| metric.weight(n) > 0.0).collect();
        nodes.sort_by(|a, b| {
            metric
                .weight(b)
                .partial_cmp(&metric.weight(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        nodes
    }
    fn split(
        nodes: &[&Arc<Node>],
        rect: [f32; 4],
        metric: Metric,
        levels: u8,
        depth: u8,
        out: &mut Vec<Tile>,
    ) {
        let [x, y, width, height] = rect;
        if nodes.is_empty() || width < 2.0 || height < 2.0 {
            return;
        }
        if nodes.len() == 1 {
            let node = nodes[0];
            out.push(Tile {
                node: Arc::clone(node),
                x,
                y,
                width,
                height,
                depth,
            });
            if depth < levels && !node.children.is_empty() && width > 90.0 && height > 65.0 {
                split(
                    &ordered(&bounded_children(node, metric), metric),
                    [
                        x + 3.0,
                        y + 25.0,
                        (width - 6.0).max(1.0),
                        (height - 28.0).max(1.0),
                    ],
                    metric,
                    levels,
                    depth + 1,
                    out,
                );
            }
            return;
        }
        let total: f64 = nodes.iter().map(|n| metric.weight(n)).sum();
        let mut sum = 0.0;
        let mut at = 0;
        while at < nodes.len() - 1 && sum < total / 2.0 {
            sum += metric.weight(nodes[at]);
            at += 1;
        }
        let ratio = (sum / total) as f32;
        let (first, second) = if width >= height {
            (
                [x, y, width * ratio, height],
                [x + width * ratio, y, width * (1.0 - ratio), height],
            )
        } else {
            (
                [x, y, width, height * ratio],
                [x, y + height * ratio, width, height * (1.0 - ratio)],
            )
        };
        split(&nodes[..at], first, metric, levels, depth, out);
        split(&nodes[at..], second, metric, levels, depth, out);
    }
    let mut result = Vec::new();
    split(
        &ordered(nodes, metric),
        [0.0, 0.0, width, height],
        metric,
        levels,
        1,
        &mut result,
    );
    result
}

pub(crate) fn build_tree(sessions: Vec<Session>, errors: Vec<String>, scanned_ms: u64) -> Snapshot {
    let mut by_tool: BTreeMap<String, Vec<Session>> = BTreeMap::new();
    for session in sessions {
        by_tool
            .entry(session.tool.clone())
            .or_default()
            .push(session);
    }
    let mut agents = Vec::new();
    for (tool, sessions) in by_tool {
        let mut by_project: BTreeMap<String, Vec<Session>> = BTreeMap::new();
        for session in sessions {
            by_project
                .entry(session.project.clone())
                .or_default()
                .push(session);
        }
        let mut projects = Vec::new();
        for (project, sessions) in by_project {
            let project_path = format!("agent:{}|project:{}", tool, project);
            let mut node = Node {
                path: project_path.clone(),
                name: if project.is_empty() {
                    "(unknown project)".into()
                } else {
                    basename(&project)
                },
                kind: "project".into(),
                tool: tool.clone(),
                title: project.clone(),
                ..Default::default()
            };
            for session in sessions {
                let mut leaf = Node {
                    path: format!("{}|session:{}", project_path, session.key),
                    name: display_title(&session.title, &session.key),
                    kind: "session".into(),
                    tool: session.tool.clone(),
                    model: session.model.clone(),
                    title: session.title.clone(),
                    file: session.file.clone(),
                    ..Default::default()
                };
                absorb(&mut leaf, &session);
                leaf.sessions = 1;
                absorb(&mut node, &session);
                node.children.push(Arc::new(leaf));
            }
            node.children.sort_by(|a, b| {
                b.tokens()
                    .cmp(&a.tokens())
                    .then_with(|| b.last_ts.cmp(&a.last_ts))
            });
            node.sessions = node.children.len() as u64;
            projects.push(node);
        }
        let mut node = Node {
            path: format!("agent:{}", tool),
            name: tool.clone(),
            kind: "agent".into(),
            tool: tool.clone(),
            ..Default::default()
        };
        for project in projects {
            node.sessions += project.sessions;
            node.messages += project.messages;
            node.input += project.input;
            node.output += project.output;
            node.cache_read += project.cache_read;
            node.cache_write += project.cache_write;
            node.cost += project.cost;
            node.first_ts = earliest(node.first_ts, project.first_ts);
            node.last_ts = node.last_ts.max(project.last_ts);
            node.children.push(Arc::new(project));
        }
        node.children.sort_by(|a, b| {
            b.tokens()
                .cmp(&a.tokens())
                .then_with(|| b.last_ts.cmp(&a.last_ts))
        });
        agents.push(node);
    }
    agents.sort_by(|a, b| {
        b.tokens()
            .cmp(&a.tokens())
            .then_with(|| b.last_ts.cmp(&a.last_ts))
    });
    let mut root = Node {
        path: "root".into(),
        name: "All sessions".into(),
        kind: "root".into(),
        ..Default::default()
    };
    for agent in agents {
        root.sessions += agent.sessions;
        root.messages += agent.messages;
        root.input += agent.input;
        root.output += agent.output;
        root.cache_read += agent.cache_read;
        root.cache_write += agent.cache_write;
        root.cost += agent.cost;
        root.first_ts = earliest(root.first_ts, agent.first_ts);
        root.last_ts = root.last_ts.max(agent.last_ts);
        root.children.push(Arc::new(agent));
    }
    Snapshot {
        root,
        errors,
        scanned_ms,
        cache_hits: 0,
    }
}

fn absorb(node: &mut Node, session: &Session) {
    node.messages += session.messages;
    node.input += session.input;
    node.output += session.output;
    node.cache_read += session.cache_read;
    node.cache_write += session.cache_write;
    node.cost += session.cost;
    node.first_ts = earliest(node.first_ts, session.first_ts);
    node.last_ts = node.last_ts.max(session.last_ts);
    if node.model.is_empty() && !session.model.is_empty() {
        node.model = session.model.clone();
    }
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

fn display_title(title: &str, key: &str) -> String {
    let text = title.trim();
    if text.is_empty() {
        key.rsplit(':')
            .next()
            .unwrap_or("untitled session")
            .to_owned()
    } else {
        text.chars().take(70).collect()
    }
}

pub(crate) fn find<'a>(node: &'a Node, path: &str) -> Option<&'a Node> {
    if node.path == path {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, path))
}

pub(crate) fn ancestors(node: &Node, path: &str) -> Vec<Node> {
    fn walk(node: &Node, path: &str, trail: &mut Vec<Node>) -> bool {
        trail.push(node.clone());
        if node.path == path {
            return true;
        }
        for child in &node.children {
            if walk(child, path, trail) {
                return true;
            }
        }
        trail.pop();
        false
    }
    let mut trail = Vec::new();
    if walk(node, path, &mut trail) {
        trail
    } else {
        Vec::new()
    }
}

pub(crate) fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    trimmed
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(trimmed)
        .to_owned()
}

static HIDE_NUMBERS: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_hide_numbers(hide: bool) {
    HIDE_NUMBERS.store(hide, Ordering::Relaxed);
}

pub(crate) fn numbers_hidden() -> bool {
    HIDE_NUMBERS.load(Ordering::Relaxed)
}

pub(crate) fn count(value: u64) -> String {
    if numbers_hidden() {
        return "•••".into();
    }
    let digits = value.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    result
}

pub(crate) fn compact(value: u64) -> String {
    if numbers_hidden() {
        return "•••".into();
    }
    let value = value as f64;
    let (scaled, unit) = if value >= 1_000_000_000.0 {
        (value / 1_000_000_000.0, "B")
    } else if value >= 1_000_000.0 {
        (value / 1_000_000.0, "M")
    } else if value >= 1_000.0 {
        (value / 1_000.0, "K")
    } else {
        (value, "")
    };
    if unit.is_empty() {
        format!("{value:.0}")
    } else if scaled >= 100.0 {
        format!("{scaled:.0}{unit}")
    } else {
        format!("{scaled:.1}{unit}")
    }
}

pub(crate) fn money(value: f64) -> String {
    if numbers_hidden() {
        return "•••".into();
    }
    if value <= 0.0 {
        "$0.00".into()
    } else if value < 0.01 {
        "<$0.01".into()
    } else if value < 1.0 {
        format!("${value:.3}")
    } else {
        format!("${value:.2}")
    }
}

/// A node's cost cell. Cursor exposes tokens but not its credit and billing
/// math, so a zero there means unknown, not free.
pub(crate) fn cost_text(node: &Node) -> String {
    if !numbers_hidden() && node.cost <= 0.0 && node.tool == "Cursor" {
        "—".into()
    } else {
        money(node.cost)
    }
}

fn date(ts: i64) -> String {
    if ts <= 0 {
        return "—".into();
    }
    chrono::DateTime::from_timestamp_millis(ts)
        .map(|time| time.with_timezone(&chrono::Local))
        .map(|time| time.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "—".into())
}

pub(crate) fn stamp(ts: i64) -> String {
    if ts <= 0 {
        return "—".into();
    }
    chrono::DateTime::from_timestamp_millis(ts)
        .map(|time| time.with_timezone(&chrono::Local))
        .map(|time| time.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".into())
}

pub(crate) fn span(first: i64, last: i64) -> String {
    if first <= 0 || last <= 0 {
        return "—".into();
    }
    let start = date(first);
    let end = date(last);
    if start == end {
        start
    } else {
        format!("{start} → {end}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, tokens: u64, messages: u64) -> Node {
        Node {
            name: name.into(),
            path: name.into(),
            kind: "session".into(),
            messages,
            input: tokens,
            ..Default::default()
        }
    }

    #[test]
    fn subdivision_preserves_area_and_follows_metric() {
        let nodes = vec![Arc::new(node("a", 9, 1)), Arc::new(node("b", 1, 9))];
        let tokens = mosaic(&nodes, 100.0, 80.0, Metric::Tokens, 1);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].node.name, "a");
        let area: f32 = tokens.iter().map(|t| t.width * t.height).sum();
        assert!((area - 8000.0).abs() < 0.01);
        let messages = mosaic(&nodes, 100.0, 80.0, Metric::Messages, 1);
        assert_eq!(messages[0].node.name, "b");
    }

    #[test]
    fn tree_groups_agent_then_project_then_session() {
        let sessions = vec![
            Session {
                key: "one".into(),
                tool: "Claude Code".into(),
                project: "/tmp/a".into(),
                title: "first".into(),
                input: 10,
                ..Default::default()
            },
            Session {
                key: "two".into(),
                tool: "OpenCode".into(),
                project: "/tmp/a".into(),
                title: "second".into(),
                output: 30,
                ..Default::default()
            },
        ];
        let snapshot = build_tree(sessions, Vec::new(), 0);
        assert_eq!(snapshot.root.sessions, 2);
        assert_eq!(snapshot.root.children.len(), 2);
        let agent = find(&snapshot.root, "agent:Claude Code").expect("agent node");
        assert_eq!(agent.sessions, 1);
        let project = find(&snapshot.root, "agent:Claude Code|project:/tmp/a");
        assert_eq!(project.expect("project node").name, "a");
        assert_eq!(ancestors(&snapshot.root, agent.path.as_str()).len(), 2);
    }

    #[test]
    fn large_projects_are_capped_without_losing_the_parent_weight() {
        let children: Vec<Arc<Node>> = (0..MAP_BUDGET + 30)
            .map(|index| Arc::new(node(&format!("s{index}"), index as u64 + 1, 1)))
            .collect();
        let parent = Node {
            path: "project".into(),
            kind: "project".into(),
            input: (0..MAP_BUDGET + 30).map(|n| n as u64 + 1).sum(),
            sessions: (MAP_BUDGET + 30) as u64,
            children,
            ..Default::default()
        };
        let bounded = bounded_children(&parent, Metric::Tokens);
        assert_eq!(bounded.len(), MAP_BUDGET);
        assert!(bounded.iter().all(|child| child.name != "s0"));
        let tiles = mosaic(&[Arc::new(parent)], 900.0, 700.0, Metric::Tokens, 3);
        assert!(tiles.len() <= MAP_BUDGET + 1);
    }

    fn at(year: i32, month: u32, day: u32, hour: u32) -> i64 {
        chrono::Local
            .with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    fn session(key: &str, last_ts: i64, input: u64) -> Session {
        Session {
            key: key.into(),
            tool: "Claude Code".into(),
            project: "/tmp/a".into(),
            title: key.into(),
            input,
            last_ts,
            first_ts: last_ts,
            messages: 1,
            ..Default::default()
        }
    }

    #[test]
    fn a_time_range_filters_sessions_and_rebuilds_parent_totals() {
        let now = at(2026, 11, 21, 12);
        let snapshot = build_tree(
            vec![
                session("today", now - 86_400_000, 10),
                session("ten-days", now - 10 * 86_400_000, 20),
                session("forty-days", now - 40 * 86_400_000, 40),
            ],
            Vec::new(),
            0,
        );
        let week = snapshot.filtered(TimeRange::Last7, now);
        assert_eq!(week.root.sessions, 1);
        assert_eq!(week.root.input, 10);
        let month = snapshot.filtered(TimeRange::Last30, now);
        assert_eq!(month.root.sessions, 2);
        assert_eq!(month.root.input, 30);
        assert_eq!(month.root.children.len(), 1);
        assert_eq!(month.root.children[0].sessions, 2);
        assert_eq!(snapshot.filtered(TimeRange::All, now).root.sessions, 3);
    }

    #[test]
    fn cursor_cost_is_unknown_not_free() {
        let mut node = Node {
            tool: "Cursor".into(),
            ..Default::default()
        };
        assert_eq!(cost_text(&node), "—");
        node.cost = 1.5;
        assert_eq!(cost_text(&node), "$1.50");
        let other = Node {
            tool: "Claude Code".into(),
            ..Default::default()
        };
        assert_eq!(cost_text(&other), "$0.00");
    }

    #[test]
    fn a_one_day_range_keeps_today_in_local_time() {
        let now = at(2026, 11, 21, 12);
        let snapshot = build_tree(
            vec![
                session("morning", at(2026, 11, 21, 1), 10),
                session("late-yesterday", at(2026, 11, 20, 23), 20),
            ],
            Vec::new(),
            0,
        );
        let day = snapshot.filtered(TimeRange::Last1, now);
        assert_eq!(day.root.sessions, 1);
        assert_eq!(day.root.input, 10);
        assert_eq!(TimeRange::from_storage("1d"), Some(TimeRange::Last1));
    }

    #[test]
    fn a_custom_range_is_inclusive_by_local_day() {
        let now = at(2026, 11, 21, 12);
        let snapshot = build_tree(
            vec![
                session("in", at(2026, 11, 7, 9), 10),
                session("after", at(2026, 11, 8, 9), 20),
            ],
            Vec::new(),
            0,
        );
        let range = parse_custom_range("2026-11-01", "2026-11-07").unwrap();
        let filtered = snapshot.filtered(range, now);
        assert_eq!(filtered.root.sessions, 1);
        assert_eq!(filtered.root.input, 10);
        assert_eq!(TimeRange::from_storage(&range.storage_value()), Some(range));
    }

    #[test]
    fn a_daily_series_buckets_sessions_by_last_activity() {
        let now = at(2026, 11, 21, 12);
        let snapshot = build_tree(
            vec![
                session("yesterday", now - 86_400_000, 10),
                session("two-days", now - 2 * 86_400_000, 20),
                session("ten-days", now - 10 * 86_400_000, 40),
            ],
            Vec::new(),
            0,
        );
        let series = daily_series(&snapshot.root, Metric::Tokens, 7, now);
        assert_eq!(series.len(), 7);
        assert_eq!(series[4], 20.0);
        assert_eq!(series[5], 10.0);
        assert_eq!(series.iter().sum::<f64>(), 30.0);
    }

    #[test]
    fn an_hourly_series_buckets_one_day_by_last_activity() {
        let now = at(2026, 11, 21, 12);
        let snapshot = build_tree(
            vec![
                session("nine", at(2026, 11, 21, 9), 10),
                session("ten", at(2026, 11, 21, 10), 20),
                session("yesterday", at(2026, 11, 20, 23), 40),
            ],
            Vec::new(),
            0,
        );
        let series = hourly_series(&snapshot.root, Metric::Tokens, now);
        assert_eq!(series.len(), 24);
        assert_eq!(series[9], 10.0);
        assert_eq!(series[10], 20.0);
        assert_eq!(series.iter().sum::<f64>(), 30.0);
    }

    #[test]
    fn cost_metric_keeps_zero_cost_sessions_tiny() {
        let nodes = vec![
            Arc::new(node("free", 100, 1)),
            Arc::new(node("paid", 100, 1)),
        ];
        let tiles = mosaic(&nodes, 100.0, 80.0, Metric::Cost, 1);
        assert_eq!(tiles.len(), 2);
        assert!(
            tiles
                .iter()
                .all(|tile| tile.width > 0.0 && tile.height > 0.0)
        );
    }
}
