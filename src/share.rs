//! Renders the current treemap as a shareable PNG, with optional redaction.

use crate::model::*;
use nsh_core::{
    canvas::{
        CanvasTextAlign, CanvasTextBaseline, LineCap, LineJoin,
        render::{ImageFormat, RenderOptions},
    },
    element::Painter,
    geometry::{Bounds, point, px, size},
    prelude::*,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(crate) const WIDTH: f32 = 1200.0;

const BG: u32 = 0x0b0b0d;
const MAP: u32 = 0x050506;
const LINE: u32 = 0x26262c;
const INK: u32 = 0xededf0;
const MUTED: u32 = 0x85858f;
const GREEN: u32 = 0x65f193;

const MONO: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else {
    "monospace"
};

#[derive(Clone, Copy, Default)]
pub(crate) struct ShareOptions {
    pub mask_agents: bool,
    pub mask_projects: bool,
    pub mask_sessions: bool,
    pub mask_numbers: bool,
}

#[derive(Default)]
struct Labels {
    agents: BTreeMap<String, String>,
    projects: BTreeMap<String, String>,
    sessions: BTreeMap<String, String>,
}

fn labels(root: &Node) -> Labels {
    let mut labels = Labels::default();
    for (agent_index, agent) in root.children.iter().enumerate() {
        labels
            .agents
            .insert(agent.tool.clone(), format!("Agent {}", agent_index + 1));
        for (project_index, project) in agent.children.iter().enumerate() {
            labels.projects.insert(
                project.path.clone(),
                format!("Project {}", project_index + 1),
            );
            for (session_index, session) in project.children.iter().enumerate() {
                labels.sessions.insert(
                    session.path.clone(),
                    format!("Session {}", session_index + 1),
                );
            }
        }
    }
    labels
}

fn node_label(node: &Node, options: ShareOptions, labels: &Labels) -> String {
    match node.kind.as_str() {
        "agent" if options.mask_agents => labels
            .agents
            .get(&node.tool)
            .cloned()
            .unwrap_or_else(|| "Agent".into()),
        "project" if options.mask_projects => labels
            .projects
            .get(&node.path)
            .cloned()
            .unwrap_or_else(|| "Project".into()),
        "session" if options.mask_sessions => labels
            .sessions
            .get(&node.path)
            .cloned()
            .unwrap_or_else(|| "Session".into()),
        _ if node.kind == "root" => "All sessions".into(),
        _ => node.name.clone(),
    }
}

fn text_width(value: &str, font_size: f32) -> f32 {
    value
        .chars()
        .map(|ch| if ch.is_ascii() { 0.6 } else { 1.0 } * font_size)
        .sum()
}

fn fit(value: &str, width: f32, font_size: f32) -> String {
    if text_width(value, font_size) <= width {
        return value.to_owned();
    }
    let mut out = String::new();
    let mut used = 0.0;
    for ch in value.chars() {
        let advance = if ch.is_ascii() { 0.6 } else { 1.0 } * font_size;
        if used + advance > width - font_size * 0.7 {
            break;
        }
        used += advance;
        out.push(ch);
    }
    out.push('…');
    out
}

fn area(x: f32, y: f32, width: f32, height: f32) -> Bounds<Points> {
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

fn rounded(
    painter: Painter,
    bounds: Bounds<Points>,
    radius: f32,
    fill: Rgba,
    stroke: Option<Rgba>,
) -> Painter {
    let painter = painter
        .begin_path()
        .round_rect(bounds, px(radius))
        .fill_color(fill)
        .fill();
    match stroke {
        Some(stroke) => painter
            .begin_path()
            .round_rect(bounds, px(radius))
            .stroke_color(stroke)
            .line_width(1.0)
            .stroke(),
        None => painter,
    }
}

#[derive(Clone, Copy)]
struct TextStyle {
    size: f32,
    weight: u16,
    color: Rgba,
    align: CanvasTextAlign,
}

fn style(size: f32, weight: u16, color: u32) -> TextStyle {
    TextStyle {
        size,
        weight,
        color: rgb(color),
        align: CanvasTextAlign::Left,
    }
}

fn style_right(size: f32, weight: u16, color: u32) -> TextStyle {
    TextStyle {
        align: CanvasTextAlign::Right,
        ..style(size, weight, color)
    }
}

fn text(painter: Painter, value: &str, x: f32, y: f32, style: TextStyle) -> Painter {
    painter
        .font(&format!("{} {}px {MONO}", style.weight, style.size))
        .text_align(style.align)
        .text_baseline(CanvasTextBaseline::Top)
        .fill_color(style.color)
        .fill_text(value, x, y)
}

fn draw_mark(painter: Painter, x: f32, y: f32, scale: f32) -> Painter {
    let x_at = |value: f32| x + value * scale;
    let y_at = |value: f32| y + value * scale;
    painter
        .stroke_color(rgb(GREEN))
        .line_width(1.8 * scale)
        .line_cap(LineCap::Round)
        .line_join(LineJoin::Round)
        .begin_path()
        .move_to(x_at(12.), y_at(3.))
        .line_to(x_at(12.), y_at(9.))
        .move_to(x_at(12.), y_at(9.))
        .line_to(x_at(6.), y_at(13.))
        .line_to(x_at(6.), y_at(20.))
        .move_to(x_at(12.), y_at(9.))
        .line_to(x_at(18.), y_at(13.))
        .line_to(x_at(18.), y_at(20.))
        .stroke()
        .begin_path()
        .arc(x_at(12.), y_at(9.), 1.6 * scale, 0., std::f32::consts::TAU)
        .arc(x_at(6.), y_at(13.), 1.6 * scale, 0., std::f32::consts::TAU)
        .arc(x_at(18.), y_at(13.), 1.6 * scale, 0., std::f32::consts::TAU)
        .stroke()
}

/// Draws the poster: the same treemap the app shows for `node`, scaled to fit.
/// Returns the painter and the height of the image in points.
pub(crate) fn painter(
    node: &Node,
    root: &Node,
    metric: Metric,
    levels: u8,
    options: ShareOptions,
    frame: (f32, f32),
) -> (Painter, f32) {
    let labels = labels(root);
    let mut p = Painter::new()
        .fill_color(rgb(BG))
        .fill_rect(0.0, 0.0, WIDTH, 10_000.0)
        .fill_color(rgba(0x65f19333))
        .fill_rect(0.0, 0.0, WIDTH, 3.0);

    p = draw_mark(p, 52.0, 42.0, 1.5);
    p = text(p, "sessiontree", 98.0, 46.0, style(22.0, 600, INK));
    p = text(
        p,
        "sessiontree.app",
        WIDTH - 52.0,
        50.0,
        style_right(11.0, 400, MUTED),
    );
    p = p
        .fill_color(rgb(LINE))
        .fill_rect(52.0, 92.0, WIDTH - 104.0, 1.0);

    let title = node_label(node, options, &labels);
    p = text(
        p,
        &fit(&title, WIDTH - 104.0, 30.0),
        52.0,
        114.0,
        style(30.0, 600, INK),
    );

    let summary = if options.mask_numbers {
        format!(
            "Session activity  ·  area = {}",
            metric.label().to_lowercase()
        )
    } else {
        format!(
            "{} sessions  ·  {} tokens  ·  {}  ·  area = {}",
            count(node.sessions),
            compact(node.tokens()),
            money(node.cost),
            metric.label().to_lowercase()
        )
    };
    p = text(p, &summary, 52.0, 156.0, style(12.0, 400, MUTED));

    // The map keeps the on-screen frame's aspect, so its tiles are the ones
    // the window shows, not a re-flow at a different shape.
    let map_aspect = (frame.0.max(1.0)) / (frame.1.max(1.0));
    let mut map_w = WIDTH - 104.0;
    let mut map_h = map_w / map_aspect;
    if map_h > 700.0 {
        map_h = 700.0;
        map_w = map_h * map_aspect;
    }
    if map_h < 240.0 {
        map_h = 240.0;
        map_w = (map_h * map_aspect).min(WIDTH - 104.0);
        map_h = map_w / map_aspect;
    }
    let map_x = (WIDTH - map_w) / 2.0;
    let map_y = 194.0;
    let map_scale = map_w / frame.0.max(1.0);

    p = rounded(p, area(map_x, map_y, map_w, map_h), 8.0, rgb(MAP), None);
    let levels = if node.kind == "root" || node.kind == "agent" {
        levels.max(2)
    } else {
        1
    };
    let tiles = mosaic(
        &bounded_children(node, metric),
        frame.0.max(1.0),
        frame.1.max(1.0),
        metric,
        levels,
    );
    p = p.save().translate(map_x, map_y).scale(map_scale, map_scale);
    if tiles.is_empty() {
        p = text(
            p,
            "No sessions to share yet.",
            frame.0 / 2.0,
            frame.1 / 2.0 - 8.0,
            TextStyle {
                align: CanvasTextAlign::Center,
                ..style(16.0, 400, MUTED)
            },
        );
    }
    for tile in &tiles {
        let width = tile.width - 2.0;
        let height = tile.height - 2.0;
        if width < 2.0 || height < 2.0 {
            continue;
        }
        let x = tile.x + 1.0;
        let y = tile.y + 1.0;
        let color = tile_color(&tile.node, tile.depth);
        p = rounded(
            p,
            area(x, y, width, height),
            3.0,
            rgba((color << 8) | 35),
            Some(rgba((color << 8) | 97)),
        );
        if width > 70.0 && height > 26.0 {
            let label = node_label(&tile.node, options, &labels);
            p = text(
                p,
                &fit(&label, width - 10.0, 11.0),
                x + 5.0,
                y + 5.0,
                style(11.0, 600, INK),
            );
        }
        if width > 90.0 && height > 70.0 && (tile.node.is_leaf() || tile.depth >= levels) {
            let value = if options.mask_numbers {
                "•••".to_owned()
            } else {
                metric.value(&tile.node)
            };
            let value_size = if width > 200.0 { 22.0 } else { 13.0 };
            p = text(p, &value, x + 5.0, y + 23.0, style(value_size, 400, INK));
        }
    }
    p = p.restore();

    let entries: Vec<&std::sync::Arc<Node>> = node.children.iter().take(10).collect();
    let legend_top = map_y + map_h + 30.0;
    let mut footer_y = map_y + map_h + 42.0;
    if !entries.is_empty() {
        let heading = match node.kind.as_str() {
            "root" => "AGENTS",
            "agent" => "PROJECTS",
            _ => "SESSIONS",
        };
        p = text(
            p,
            &format!("{heading}  ·  BY {}", metric.label().to_uppercase()),
            52.0,
            legend_top,
            style(9.0, 600, MUTED),
        );
        for (index, child) in entries.iter().enumerate() {
            let column = index % 2;
            let row = index / 2;
            let x = 52.0 + column as f32 * 556.0;
            let y = legend_top + 24.0 + row as f32 * 25.0;
            p = rounded(
                p,
                area(x, y + 5.0, 8.0, 8.0),
                4.0,
                rgb(agent_color(&child.tool)),
                None,
            );
            p = text(
                p,
                &fit(&node_label(child, options, &labels), 400.0, 11.0),
                x + 16.0,
                y + 2.0,
                style(11.0, 400, INK),
            );
            let value = if options.mask_numbers {
                "•••".to_owned()
            } else {
                metric.value(child)
            };
            p = text(p, &value, x + 520.0, y + 2.0, style_right(11.0, 400, MUTED));
        }
        let rows = entries.len().div_ceil(2) as f32;
        footer_y = legend_top + 24.0 + rows * 25.0 + 12.0;
    }
    p = p
        .fill_color(rgb(LINE))
        .fill_rect(52.0, footer_y, WIDTH - 104.0, 1.0);
    p = text(
        p,
        "Local-first session history  ·  nothing leaves this machine",
        52.0,
        footer_y + 14.0,
        style(10.0, 400, MUTED),
    );
    p = text(
        p,
        "generated with sessiontree",
        WIDTH - 52.0,
        footer_y + 14.0,
        style_right(10.0, 400, MUTED),
    );

    (p, footer_y + 40.0)
}

pub(crate) fn redacted(root: &Node, options: ShareOptions) -> Node {
    if !(options.mask_agents || options.mask_projects || options.mask_sessions) {
        return root.clone();
    }
    let mut root = root.clone();
    let hide_files = options.mask_projects || options.mask_sessions;
    for (agent_index, agent) in root.children.iter_mut().enumerate() {
        let agent_label = format!("Agent {}", agent_index + 1);
        let mut agent_node = (**agent).clone();
        if options.mask_agents {
            agent_node.name = agent_label.clone();
            agent_node.tool = agent_label.clone();
            agent_node.title.clear();
        }
        for (project_index, project) in agent_node.children.iter_mut().enumerate() {
            let mut project_node = (**project).clone();
            if options.mask_projects {
                let label = format!("Project {}", project_index + 1);
                project_node.name = label.clone();
                project_node.title = label;
            }
            if options.mask_agents {
                project_node.tool = agent_label.clone();
            }
            if hide_files {
                project_node.file.clear();
            }
            for (session_index, session) in project_node.children.iter_mut().enumerate() {
                let mut session_node = (**session).clone();
                if options.mask_sessions {
                    let label = format!("Session {}", session_index + 1);
                    session_node.name = label.clone();
                    session_node.title = label;
                }
                if options.mask_agents {
                    session_node.tool = agent_label.clone();
                }
                if hide_files {
                    session_node.file.clear();
                }
                *session = Arc::new(session_node);
            }
            *project = Arc::new(project_node);
        }
        *agent = Arc::new(agent_node);
    }
    root
}

pub(crate) fn render(
    painter: Painter,
    height: f32,
) -> nsh_core::element::Answer<Result<Vec<u8>, nsh_core::element::JsError>> {
    let mut options = RenderOptions::default();
    options.scale = Some(2.0);
    options.format = ImageFormat::Png;
    painter.render(size(px(WIDTH), px(height)), options)
}

pub(crate) fn data_uri(png: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(22 + png.len().div_ceil(3) * 4);
    out.push_str("data:image/png;base64,");
    for chunk in png.chunks(3) {
        let first = chunk[0] as u32;
        let second = chunk.get(1).copied().unwrap_or(0) as u32;
        let third = chunk.get(2).copied().unwrap_or(0) as u32;
        let value = (first << 16) | (second << 8) | third;
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn encodes_a_png_data_uri() {
        assert_eq!(data_uri(b"hi"), "data:image/png;base64,aGk=");
        assert_eq!(data_uri(b"a"), "data:image/png;base64,YQ==");
        assert_eq!(data_uri(b"ab"), "data:image/png;base64,YWI=");
    }

    #[test]
    fn redacts_names_tools_and_files_but_keeps_paths() {
        let session = Arc::new(Node {
            path: "agent:Claude Code|project:/work/x|session:s1".into(),
            name: "Fix the login".into(),
            title: "Fix the login".into(),
            kind: "session".into(),
            tool: "Claude Code".into(),
            file: "/Users/someone/.claude/s1.jsonl".into(),
            ..Node::default()
        });
        let project = Arc::new(Node {
            path: "agent:Claude Code|project:/work/x".into(),
            name: "x".into(),
            title: "/work/x".into(),
            kind: "project".into(),
            tool: "Claude Code".into(),
            file: "/Users/someone/.claude".into(),
            children: vec![session],
            ..Node::default()
        });
        let agent = Arc::new(Node {
            path: "agent:Claude Code".into(),
            name: "Claude Code".into(),
            kind: "agent".into(),
            tool: "Claude Code".into(),
            children: vec![project],
            ..Node::default()
        });
        let root = Node {
            path: "root".into(),
            name: "All sessions".into(),
            kind: "root".into(),
            children: vec![agent],
            ..Node::default()
        };
        let masked = ShareOptions {
            mask_agents: true,
            mask_projects: true,
            mask_sessions: true,
            mask_numbers: false,
        };
        let redacted = redacted(&root, masked);
        let agent = &redacted.children[0];
        let project = &agent.children[0];
        let session = &project.children[0];
        assert_eq!(agent.name, "Agent 1");
        assert_eq!(agent.tool, "Agent 1");
        assert_eq!(project.name, "Project 1");
        assert_eq!(project.title, "Project 1");
        assert_eq!(session.name, "Session 1");
        assert_eq!(session.title, "Session 1");
        assert_eq!(session.file, "");
        assert_eq!(session.path, "agent:Claude Code|project:/work/x|session:s1");
    }

    #[test]
    fn masks_each_kind_of_name() {
        let session = Arc::new(Node {
            path: "agent:Claude Code|project:/work|x|session:s1".into(),
            name: "Fix the login".into(),
            kind: "session".into(),
            tool: "Claude Code".into(),
            ..Node::default()
        });
        let project = Arc::new(Node {
            path: "agent:Claude Code|project:/work|x".into(),
            name: "x".into(),
            kind: "project".into(),
            tool: "Claude Code".into(),
            children: vec![session],
            ..Node::default()
        });
        let agent = Arc::new(Node {
            path: "agent:Claude Code".into(),
            name: "Claude Code".into(),
            kind: "agent".into(),
            tool: "Claude Code".into(),
            children: vec![project],
            ..Node::default()
        });
        let root = Node {
            path: "root".into(),
            name: "All sessions".into(),
            kind: "root".into(),
            children: vec![agent],
            ..Node::default()
        };
        let labels = labels(&root);
        let masked = ShareOptions {
            mask_agents: true,
            mask_projects: true,
            mask_sessions: true,
            mask_numbers: true,
        };
        assert_eq!(node_label(&root, masked, &labels), "All sessions");
        let agent = &root.children[0];
        let project = &agent.children[0];
        let session = &project.children[0];
        assert_eq!(node_label(agent, masked, &labels), "Agent 1");
        assert_eq!(node_label(project, masked, &labels), "Project 1");
        assert_eq!(node_label(session, masked, &labels), "Session 1");
        assert_eq!(
            node_label(agent, ShareOptions::default(), &labels),
            "Claude Code"
        );
        assert_eq!(node_label(project, ShareOptions::default(), &labels), "x");
        assert_eq!(
            node_label(session, ShareOptions::default(), &labels),
            "Fix the login"
        );
    }
}
