//! Native Rust components for the session treemap.
use crate::{model::*, state::State};
use nsh_base::{dialog, nav_page, nav_stack};
use nsh_core::{
    AccessibilityRole,
    canvas::{LineCap, LineJoin},
    keymap::When,
    prelude::*,
};
use std::sync::Arc;

const MONO: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else {
    "monospace"
};
const BG: u32 = 0x070708;
const PANEL: u32 = 0x101012;
const RAISED: u32 = 0x18181b;
const LINE: u32 = 0x29292d;
const INK: u32 = 0xededf0;
const MUTED: u32 = 0x85858f;
const GREEN: u32 = 0x65f193;
const AMBER: u32 = 0xe9b64d;

fn caption(value: impl IntoReactive<SharedString>, size: f32, color: u32) -> Text {
    text(value)
        .font_family(MONO)
        .text_size(px(size))
        .text_color(rgb(color))
}

fn row() -> View {
    view().flex_row().items_center().gap(px(8.))
}

fn button(
    label: impl IntoReactive<SharedString>,
    disabled: impl IntoReactive<bool>,
    action: impl Fn() + 'static,
) -> View {
    control(label, None, disabled, action)
}

fn control(
    label: impl IntoReactive<SharedString>,
    glyph: Option<&'static str>,
    disabled: impl IntoReactive<bool>,
    action: impl Fn() + 'static,
) -> View {
    let disabled = disabled.into_reactive();
    let disabled = computed(move || disabled.get());
    chip(label, glyph, move || disabled.get()).on_press(move |_: &()| {
        if !disabled.get_untracked() {
            action()
        }
    })
}

fn chip(
    label: impl IntoReactive<SharedString>,
    glyph: Option<&'static str>,
    disabled: impl IntoReactive<bool>,
) -> View {
    let disabled = disabled.into_reactive();
    let disabled = computed(move || disabled.get());
    let label = label.into_reactive();
    let label = computed(move || label.get());
    view()
        .role(AccessibilityRole::Button)
        .label(move || label.get())
        .disabled(move || disabled.get())
        .window_drag(false)
        .map(|v| {
            if glyph.is_some() {
                v.w(px(32.))
                    .h(px(32.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(rgb(0x1f1f24)))
                    .active(|s| s.bg(rgb(0x2a2a30)))
            } else {
                v.px(px(13.))
                    .py(px(8.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(rgb(0x36363d))
                    .bg(rgb(RAISED))
                    .hover(|s| s.bg(rgb(0x222227)))
            }
        })
        .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0_f32 })
        .items_center()
        .justify_center()
        .child(
            when(move || glyph.is_some(), move || icon(glyph.unwrap())).otherwise(move || {
                caption(move || label.get(), 11., INK)
                    .font_weight(FontWeight::SEMIBOLD)
                    .number_of_lines(1)
            }),
        )
}

fn icon(glyph: &str) -> impl IntoElement {
    let path = match glyph {
        "⟳" => "M20 11A8.1 8.1 0 0 0 4.5 9M4 5v4h4M4 13a8.1 8.1 0 0 0 15.5 2m.5 4v-4h-4",
        "↑" => "M12 19V5M5 12l7-7 7 7",
        "‹" => "M15 6l-6 6 6 6",
        "↗" => "M7 17L17 7M9 7h8v8",
        _ => "M4 20h4L18.5 9.5a2.828 2.828 0 1 0-4-4L4 16v4M13.5 6.5l4 4",
    };
    canvas(
        Painter::new()
            .scale(16. / 24., 16. / 24.)
            .stroke_color(rgb(if glyph == "⟳" { GREEN } else { 0xc4c4cc }))
            .line_width(1.75)
            .line_cap(LineCap::Round)
            .line_join(LineJoin::Round)
            .stroke_svg(path),
    )
    .w(px(16.))
    .h(px(16.))
}

fn app_mark() -> impl IntoElement {
    canvas(
        Painter::new()
            .stroke_color(rgb(GREEN))
            .line_width(1.8)
            .line_cap(LineCap::Round)
            .line_join(LineJoin::Round)
            .begin_path()
            .move_to(12., 3.)
            .line_to(12., 9.)
            .move_to(12., 9.)
            .line_to(6., 13.)
            .line_to(6., 20.)
            .move_to(12., 9.)
            .line_to(18., 13.)
            .line_to(18., 20.)
            .stroke()
            .begin_path()
            .arc(12., 9., 1.6, 0., std::f32::consts::TAU)
            .arc(6., 13., 1.6, 0., std::f32::consts::TAU)
            .arc(18., 13., 1.6, 0., std::f32::consts::TAU)
            .stroke(),
    )
    .w(px(24.))
    .h(px(24.))
}

fn meter(fraction: impl Fn() -> f32 + 'static, height: f32, color: u32) -> impl IntoElement {
    view()
        .h(px(height))
        .rounded(px(2.))
        .bg(rgb(0x28282d))
        .overflow_hidden()
        .child(
            view()
                .h(px(height))
                .rounded(px(2.))
                .bg(rgb(color))
                .w(move || relative(fraction().clamp(0., 1.))),
        )
}

fn stat(label: &'static str, value: impl IntoReactive<SharedString>) -> impl IntoElement {
    view()
        .flex_1()
        .gap(px(4.))
        .child(caption(label, 9., MUTED))
        .child(caption(value, 13., INK).number_of_lines(1))
}

type Back = (&'static str, Box<dyn Fn()>);

fn page_header(
    title: impl IntoReactive<SharedString>,
    back: Option<Back>,
    disabled: impl IntoReactive<bool>,
) -> impl IntoElement {
    let disabled = disabled.into_reactive();
    let disabled = computed(move || disabled.get());
    row()
        .h(px(44.))
        .px(px(10.))
        .gap(px(6.))
        .border_b_1()
        .border_color(rgb(LINE))
        .bg(rgb(PANEL))
        .when_some(back, |v, (label, back)| {
            v.child(
                row()
                    .gap(px(2.))
                    .pl(px(2.))
                    .pr(px(8.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(rgb(0x1d1d21)))
                    .role(AccessibilityRole::Button)
                    .label(label)
                    .disabled(move || disabled.get())
                    .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0_f32 })
                    .on_press(move |_: &()| back())
                    .child(icon("‹"))
                    .child(caption("Back", 11., MUTED)),
            )
        })
        .child(
            caption(title, 12., INK)
                .font_weight(FontWeight::SEMIBOLD)
                .flex_1()
                .number_of_lines(1),
        )
}

/// Sessiontree's Rust entry point, shared by `nsh dev`, `nsh run`, and Cargo.
#[component]
pub fn App() -> impl IntoElement {
    {
        use nsh_core::desktop::{TitleBarOptions, TitleBarStyle, WindowAttributes};
        let mut title_bar = TitleBarOptions::default();
        title_bar.style = TitleBarStyle::Hidden;
        title_bar.height = Some(px(56.));
        let mut attributes = WindowAttributes::default();
        attributes.title_bar = Some(title_bar);
        nsh_core::element::desktop::current_window().set(attributes);
    }
    let s = State::new();
    let keys = KeyOptions {
        when: When::NotEditing,
        ..Default::default()
    };
    on_key_with("mod+r", keys, move |_| s.scan());
    on_key_with("alt+up", keys, move |_| s.up());
    on_key_with("enter", keys, move |_| s.enter(s.selection.get_untracked()));
    on_key_with(
        "escape",
        KeyOptions {
            when: When::Always,
            ..Default::default()
        },
        move |_| s.close(),
    );
    on_key_with("mod+shift+s", keys, move |_| s.share_open.set(true));
    for (key, delta) in [("down", 1_isize), ("up", -1)] {
        on_key_with(key, keys, move |_| {
            let list = s.current().children;
            if list.is_empty() {
                return;
            }
            let index = list
                .iter()
                .position(|n| n.path == s.selection.get_untracked().path)
                .map(|i| i as isize)
                .unwrap_or(-1);
            s.selection.set(
                list[(index + delta).clamp(0, list.len() as isize - 1) as usize]
                    .as_ref()
                    .clone(),
            );
        });
    }
    s.scan();
    view()
        .flex_1()
        .bg(rgb(BG))
        .child(toolbar(s))
        .child(
            view()
                .h(px(2.))
                .bg(rgb(BG))
                .child(view().h(px(2.)).bg(rgb(GREEN)).w(move || {
                    let percent = s.progress.get().map_or(0, |p| {
                        if p.total == 0 {
                            12
                        } else {
                            (p.done as f32 / p.total as f32 * 100.).round() as u8
                        }
                    });
                    relative(if s.busy.get() {
                        (percent.max(3) as f32 / 100.).min(1.)
                    } else {
                        0.
                    })
                })),
        )
        .child(
            view()
                .flex_1()
                .min_h(px(150.))
                .flex_row()
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.))
                        .p(px(10.))
                        .gap(px(7.))
                        .child(path_bar(s))
                        .child(legend(s))
                        .child(treemap(s)),
                )
                .child(sidebar(s)),
        )
        .child(statusbar(s))
        .child(when(move || s.share_open.get(), move || share_dialog(s)))
}

fn toolbar(s: State) -> impl IntoElement {
    row()
        .test_id("titlebar")
        .h(px(56.))
        .gap(px(10.))
        .pl(move || px((nsh_core::element::desktop::title_bar().left.0 + 8.).max(16.)))
        .pr(move || px((nsh_core::element::desktop::title_bar().right.0 + 8.).max(16.)))
        .border_b_1()
        .border_color(rgb(LINE))
        .bg(rgb(0x111113))
        .window_drag(true)
        .child(app_mark())
        .child(caption("sessiontree", 17., INK).font_weight(FontWeight::SEMIBOLD))
        .child(view().flex_1())
        .child(control(
            "Share image",
            Some("↗"),
            move || s.busy.get(),
            move || s.share_open.set(true),
        ))
        .child(control(
            "Rescan sessions",
            Some("⟳"),
            move || s.busy.get(),
            move || s.scan(),
        ))
        .child(control(
            "Up one level",
            Some("↑"),
            move || s.busy.get() || s.scope.get() == "root",
            move || s.up(),
        ))
        .child(view().w(px(1.)).h(px(20.)).mx(px(4.)).bg(rgb(LINE)))
        .child(
            row()
                .gap(px(2.))
                .p(px(3.))
                .rounded(px(8.))
                .bg(rgb(0x0b0b0d))
                .border_1()
                .border_color(rgb(LINE))
                .children(
                    [Metric::Tokens, Metric::Cost, Metric::Messages]
                        .into_iter()
                        .map(move |metric| {
                            view()
                                .role(AccessibilityRole::Tab)
                                .label(metric.label())
                                .accessibility_state(move || nsh_core::AccessibilityState {
                                    selected: s.metric.get() == metric,
                                    ..Default::default()
                                })
                                .window_drag(false)
                                .on_press(move |_: &()| s.metric.set(metric))
                                .h(px(26.))
                                .px(px(12.))
                                .items_center()
                                .justify_center()
                                .rounded(px(5.))
                                .bg(move || {
                                    rgba(if s.metric.get() == metric {
                                        0x24242aff
                                    } else {
                                        0
                                    })
                                })
                                .hover(|s| s.bg(rgb(0x1a1a1f)))
                                .active(|s| s.bg(rgb(0x2a2a30)))
                                .child(caption(metric.label(), 11., INK).text_color(move || {
                                    rgb(if s.metric.get() == metric { INK } else { MUTED })
                                }))
                        }),
                ),
        )
}

fn crumb(s: State, item: ReadSignal<(String, String)>) -> impl IntoElement {
    row()
        .min_w(px(0.))
        .flex_shrink(1.)
        .child(caption("›", 12., MUTED))
        .child(
            view()
                .p(px(4.))
                .min_w(px(0.))
                .flex_shrink(1.)
                .rounded(px(4.))
                .hover(|s| s.bg(rgb(0x18181c)))
                .role(AccessibilityRole::Button)
                .label(move || format!("Go to {}", item.get().1))
                .on_press(move |_: &()| s.navigate(item.get_untracked().0))
                .child(caption(move || item.get().1, 12., GREEN).number_of_lines(1)),
        )
}

fn path_bar(s: State) -> impl IntoElement {
    let crumbs = computed(move || {
        let root = s.snapshot.get().root;
        ancestors(&root, &s.scope.get())
            .into_iter()
            .map(|node| {
                let label = if node.kind == "root" {
                    "All sessions".to_owned()
                } else if node.kind == "project" {
                    node.title.clone()
                } else {
                    node.name.clone()
                };
                (node.path, label)
            })
            .collect::<Vec<_>>()
    });
    row()
        .h(px(28.))
        .flex_shrink(0.)
        .child(view().flex_1().min_w(px(80.)).overflow_hidden().child(
            row().gap(px(3.)).overflow_hidden().child(each(
                move || crumbs.get(),
                |item| item.0.clone(),
                move |item, _| crumb(s, item),
            )),
        ))
        .child(
            view()
                .w(px(270.))
                .h(px(28.))
                .flex_shrink(0.)
                .justify_center()
                .child(
                    caption(
                        move || {
                            let node = s.current();
                            if node.path.is_empty() {
                                String::new()
                            } else {
                                format!(
                                    "{} sessions · {} tokens · {}",
                                    count(node.sessions),
                                    compact(node.tokens()),
                                    money(node.cost)
                                )
                            }
                        },
                        11.,
                        MUTED,
                    )
                    .number_of_lines(1)
                    .text_right(),
                ),
        )
}

fn legend(s: State) -> impl IntoElement {
    let agents = computed(move || {
        let root = s.snapshot.get().root;
        if root.children.is_empty() {
            Vec::new()
        } else {
            root.children
                .iter()
                .map(|agent| (agent.name.clone(), agent_color(&agent.tool), agent.sessions))
                .collect::<Vec<_>>()
        }
    });
    scroll()
        .horizontal()
        .scrollbar(false)
        .h(px(20.))
        .flex_shrink(0.)
        .child(
            row()
                .gap(px(14.))
                .child(each(
                    move || agents.get(),
                    |agent| agent.0.clone(),
                    move |agent, _| {
                        row()
                            .gap(px(5.))
                            .child(
                                view()
                                    .w(px(8.))
                                    .h(px(8.))
                                    .rounded(px(2.))
                                    .bg(move || rgb(agent.get().1)),
                            )
                            .child(caption(
                                move || format!("{} · {}", agent.get().0, count(agent.get().2)),
                                10.,
                                MUTED,
                            ))
                    },
                ))
                .child(caption(
                    move || format!("{} · color = agent", s.metric.get().hint()),
                    10.,
                    MUTED,
                )),
        )
}

fn treemap(s: State) -> impl IntoElement {
    let frame = s.map_frame;
    let tiles = computed(move || {
        let (w, h) = frame.get();
        let node = s.current();
        let metric = s.metric.get();
        let levels = if node.kind == "root" || node.kind == "agent" {
            s.levels.get().max(2)
        } else {
            1
        };
        mosaic(&bounded_children(&node, metric), w, h, metric, levels)
    });
    view()
        .test_id("treemap")
        .flex_1()
        .overflow_hidden()
        .rounded(px(3.))
        .bg(rgb(0x050506))
        .on_layout(move |b: &Bounds<Points>| frame.set((b.size.width.0, b.size.height.0)))
        .on_mouse_enter(move |_: &MouseEvent| s.in_map.set(true))
        .on_mouse_leave(move |_: &MouseEvent| {
            s.in_map.set(false);
            s.hovered.set(Vec::new());
        })
        .child(
            view()
                .w(relative(1.))
                .h(relative(1.))
                .child(each(
                    move || tiles.get(),
                    |tile| tile.node.path.clone(),
                    move |tile, _| map_tile(s, tile),
                )),
        )
        .child(when(
            move || tiles.get().is_empty(),
            move || {
                view()
                    .absolute()
                    .inset(px(0.))
                    .items_center()
                    .justify_center()
                    .gap(px(14.))
                    .child(when(
                        move || s.busy.get(),
                        move || scan_progress(s),
                    ))
                    .child(when(
                        move || !s.busy.get(),
                        move || {
                            view()
                                .items_center()
                                .gap(px(12.))
                                .child(caption(
                                    move || {
                                        if s.current().kind == "session" {
                                            "This is a single session."
                                        } else {
                                            "No sessions here yet."
                                        }
                                    },
                                    22.,
                                    INK,
                                ))
                                .child(caption(
                                    move || {
                                        if s.current().kind == "session" {
                                            "Go back up to see the tree around it."
                                        } else {
                                            "Sessiontree reads Claude Code, Codex, Cursor, Copilot, OpenCode, Pi, DeepSeek Harness, ZCode, MiMo Code, Antigravity and Kiro history that already exists on this machine."
                                        }
                                    },
                                    12.,
                                    MUTED,
                                ))
                        },
                    ))
            },
        ))
}

fn scan_progress(s: State) -> impl IntoElement {
    let p = move || s.progress.get().unwrap_or_default();
    view()
        .w(px(430.))
        .gap(px(12.))
        .child(caption("INDEXING STATIC SESSIONS", 10., MUTED).font_weight(FontWeight::SEMIBOLD))
        .child(caption(move || p().phase, 26., INK))
        .child(meter(
            move || {
                let p = p();
                if p.total == 0 {
                    0.
                } else {
                    p.done as f32 / p.total as f32
                }
            },
            4.,
            GREEN,
        ))
        .child(
            row()
                .justify_between()
                .child(stat("FILES", move || count(p().done)))
                .child(stat("TOTAL", move || count(p().total)))
                .child(stat("SESSIONS", move || count(p().sessions)))
                .child(stat("ELAPSED", move || format!("{}s", s.seconds.get()))),
        )
        .child(caption(
            "First launch reads every transcript. Later launches reuse the cache and only re-read changed files.",
            10.,
            MUTED,
        ))
}

fn map_tile(s: State, tile: ReadSignal<Tile>) -> impl IntoElement {
    let departing = (*tile.get_untracked().node).clone();
    cleanup(move || s.hover(departing, false));
    view()
        .absolute()
        .left(move || px(tile.get().x + 1.))
        .top(move || px(tile.get().y + 1.))
        .w(move || px((tile.get().width - 2.).max(0.)))
        .h(move || px((tile.get().height - 2.).max(0.)))
        .rounded(px(3.))
        .overflow_hidden()
        .border_1()
        .border_color(move || {
            let t = tile.get();
            let alpha = if s.selection.get().path == t.node.path {
                242
            } else {
                97
            };
            rgba((tile_color(&t.node, t.depth) << 8) | alpha)
        })
        .bg(move || {
            let t = tile.get();
            rgba((tile_color(&t.node, t.depth) << 8) | 35)
        })
        .bg_linear_to_br()
        .gradient_from(move || {
            let t = tile.get();
            rgba((tile_color(&t.node, t.depth) << 8) | 18)
        })
        .gradient_to(rgba(0x0000001f))
        .p(px(5.))
        .role(AccessibilityRole::Button)
        .label(move || tile.get().node.name.clone())
        .on_press(move |_: &()| s.choose((*tile.get_untracked().node).clone()))
        .on_mouse_enter(move |_: &MouseEvent| s.hover((*tile.get_untracked().node).clone(), true))
        .on_mouse_leave(move |_: &MouseEvent| s.hover((*tile.get_untracked().node).clone(), false))
        .child(when(
            move || tile.get().width > 70. && tile.get().height > 26.,
            move || {
                row()
                    .gap(px(5.))
                    .child(
                        caption(move || tile.get().node.name.clone(), 11., INK)
                            .font_weight(FontWeight::SEMIBOLD)
                            .flex_1()
                            .number_of_lines(1),
                    )
                    .child(when(
                        move || !tile.get().node.is_leaf() && tile.get().width > 140.,
                        move || {
                            caption("↗", 13., INK).on_press(move |_: &()| {
                                s.enter((*tile.get_untracked().node).clone())
                            })
                        },
                    ))
            },
        ))
        .child(when(
            move || {
                let t = tile.get();
                t.width > 90. && t.height > 70. && (t.node.is_leaf() || t.depth >= s.levels.get())
            },
            move || {
                caption(move || s.metric.get().value(&tile.get().node), 22., INK)
                    .mt(px(5.))
                    .text_size(move || px(if tile.get().width > 200. { 22. } else { 13. }))
            },
        ))
}

const SHARE_READY: &str = "Ready to copy. Nothing leaves this machine.";

fn share_dialog(s: State) -> impl IntoElement {
    dialog()
        .label("Share screenshot")
        .animation(nsh_core::ModalAnimation::None)
        .close_on_escape(true)
        .close_on_backdrop_press(false)
        .on_cancel(move || s.share_open.set(false))
        .child(
            nsh_core::element::overlay()
                .inset_0()
                .bg(rgba(0x000000aa))
                .items_center()
                .justify_center()
                .on_press(move |_: &()| s.share_open.set(false))
                .child(
                    view()
                        .w(px(560.))
                        .p(px(22.))
                        .gap(px(14.))
                        .rounded(px(16.))
                        .bg(rgb(PANEL))
                        .border_1()
                        .border_color(rgb(LINE))
                        .on_press(move |_: &()| {})
                        .child(
                            row()
                                .justify_between()
                                .child(
                                    caption("Share screenshot", 18., INK)
                                        .font_weight(FontWeight::SEMIBOLD),
                                )
                                .child(
                                    view()
                                        .role(AccessibilityRole::Button)
                                        .label("Close share")
                                        .w(px(28.))
                                        .h(px(28.))
                                        .rounded(px(6.))
                                        .items_center()
                                        .justify_center()
                                        .hover(|v| v.bg(rgb(0x1f1f24)))
                                        .on_press(move |_: &()| s.share_open.set(false))
                                        .child(caption("✕", 14., MUTED)),
                                ),
                        )
                        .child(
                            caption(
                                "Copies this window as it looks now. Mask what you need first; the image is made and copied on this machine.",
                                11.,
                                MUTED,
                            )
                            .number_of_lines(3),
                        )
                        .child(share_toggle(
                            s,
                            "Agent names",
                            "Replace agents with Agent 1, Agent 2…",
                            s.share_agents,
                        ))
                        .child(share_toggle(
                            s,
                            "Project names",
                            "Replace projects with Project 1, Project 2…",
                            s.share_projects,
                        ))
                        .child(share_toggle(
                            s,
                            "Session titles",
                            "Hide the first prompt of every session",
                            s.share_sessions,
                        ))
                        .child(share_toggle(
                            s,
                            "Numbers",
                            "Hide counts, tokens, cost and dates",
                            s.share_numbers,
                        ))
                        .child(
                            caption(move || s.share_status.get(), 10., MUTED)
                                .text_color(move || {
                                    rgb(if s.share_busy.get() { AMBER } else { MUTED })
                                })
                                .number_of_lines(2),
                        )
                        .child(
                            row()
                                .justify_end()
                                .gap(px(8.))
                                .child(button("Cancel", false, move || {
                                    s.share_open.set(false)
                                }))
                                .child(
                                    view()
                                        .role(AccessibilityRole::Button)
                                        .label("Copy screenshot")
                                        .disabled(move || s.share_busy.get())
                                        .opacity(move || {
                                            if s.share_busy.get() { 0.6_f32 } else { 1.0_f32 }
                                        })
                                        .px(px(16.))
                                        .py(px(9.))
                                        .rounded(px(8.))
                                        .bg(move || {
                                            rgb(if s.share_busy.get() {
                                                0x2f6b45
                                            } else {
                                                GREEN
                                            })
                                        })
                                        .hover(|v| v.opacity(0.9))
                                        .active(|v| v.opacity(0.8))
                                        .on_press(move |_: &()| s.copy_share())
                                        .child(
                                            caption(
                                                move || {
                                                    if s.share_busy.get() {
                                                        "Capturing…"
                                                    } else {
                                                        "Copy screenshot"
                                                    }
                                                },
                                                12.,
                                                BG,
                                            )
                                            .font_weight(FontWeight::SEMIBOLD),
                                        ),
                                ),
                        ),
                ),
        )
}

fn share_toggle(
    s: State,
    label: &'static str,
    detail: &'static str,
    value: Signal<bool>,
) -> impl IntoElement {
    let on = move || value.get();
    view()
        .role(AccessibilityRole::Checkbox)
        .label(label)
        .accessibility_state(move || nsh_core::AccessibilityState {
            checked: Some(if on() {
                nsh_core::Checked::True
            } else {
                nsh_core::Checked::False
            }),
            ..Default::default()
        })
        .flex_row()
        .items_center()
        .gap(px(12.))
        .p(px(10.))
        .rounded(px(9.))
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(0x141418))
        .hover(|v| v.bg(rgb(0x1a1a1f)))
        .on_press(move |_: &()| {
            value.set(!value.get_untracked());
            s.share_status.set(SHARE_READY.into());
        })
        .child(
            view()
                .w(px(18.))
                .h(px(18.))
                .rounded(px(4.))
                .border_1()
                .border_color(move || {
                    rgb(if on() { GREEN } else { 0x3a3a42 })
                })
                .bg(move || {
                    if on() {
                        rgb(GREEN)
                    } else {
                        rgba(0x00000000)
                    }
                })
                .items_center()
                .justify_center()
                .child(when(on, move || {
                    caption("✓", 11., BG).font_weight(FontWeight::BOLD)
                })),
        )
        .child(
            view()
                .flex_1()
                .gap(px(2.))
                .child(caption(label, 12., INK))
                .child(caption(detail, 10., MUTED).number_of_lines(2)),
        )
}

fn sidebar(s: State) -> impl IntoElement {
    let details = computed(move || {
        let selection = s.selection.get();
        !selection.path.is_empty() && selection.path != s.scope.get()
    });
    let back = move || s.selection.set(s.current());
    view()
        .w(px(300.))
        .flex_shrink(0.)
        .bg(rgb(PANEL))
        .border_l_1()
        .border_color(rgb(LINE))
        .child(
            nav_stack()
                .flex_1()
                .child(
                    nav_page()
                        .key("contents")
                        .title(move || s.current().name)
                        .header(page_header(move || s.current().name, None, false))
                        .child(contents_page(s)),
                )
                .child(when(
                    move || details.get(),
                    move || {
                        nav_page()
                            .key("details")
                            .title(move || s.selection.get().name)
                            .on_dismiss(back)
                            .header(page_header(
                                move || s.selection.get().name,
                                Some(("Back to contents", Box::new(back))),
                                false,
                            ))
                            .child(details_page(s))
                    },
                )),
        )
        .child(sources_footer(s))
}

fn scope_summary(s: State) -> impl IntoElement {
    let node = move || s.current();
    let metric = move || s.metric.get();
    view()
        .gap(px(9.))
        .pb(px(12.))
        .border_b_1()
        .border_color(rgb(LINE))
        .child(
            caption(
                move || {
                    let node = node();
                    if node.kind == "project" && !node.title.is_empty() {
                        node.title
                    } else if node.kind == "root" {
                        "Every agent, project and session found on this machine.".into()
                    } else {
                        node.name
                    }
                },
                10.,
                MUTED,
            )
            .number_of_lines(2),
        )
        .child(caption(move || metric().value(&node()), 32., INK))
        .child(meter(
            move || {
                let root = s.snapshot.get().root;
                metric().weight(&node()) as f32 / metric().weight(&root).max(1.0) as f32
            },
            4.,
            GREEN,
        ))
        .child(
            row()
                .gap(px(12.))
                .child(stat("SESSIONS", move || count(node().sessions)))
                .child(stat("MESSAGES", move || count(node().messages))),
        )
        .child(
            row()
                .gap(px(12.))
                .child(stat("IN / OUT", move || {
                    format!("{} / {}", compact(node().input), compact(node().output))
                }))
                .child(stat("COST", move || money(node().cost))),
        )
        .child(
            row()
                .gap(px(12.))
                .child(stat("CACHE", move || compact(node().cache())))
                .child(stat("SPAN", move || {
                    if numbers_hidden() {
                        "•••".into()
                    } else {
                        span(node().first_ts, node().last_ts)
                    }
                })),
        )
        .child(caption(
            "Click a tile to select · click again to open · right side drills down",
            9.,
            MUTED,
        ))
}

fn contents_page(s: State) -> impl IntoElement {
    scroll().flex_1().child(
        view()
            .p(px(14.))
            .gap(px(9.))
            .child(scope_summary(s))
            .child(
                row()
                    .justify_between()
                    .child(caption(
                        move || {
                            let node = s.current();
                            match node.kind.as_str() {
                                "root" => "AGENTS".to_owned(),
                                "agent" => "PROJECTS".to_owned(),
                                _ => "SESSIONS".to_owned(),
                            }
                        },
                        10.,
                        MUTED,
                    ))
                    .child(caption(
                        move || {
                            let total = s.current().children.len();
                            if total > MAP_BUDGET {
                                format!("top {} of {}", MAP_BUDGET, count(total as u64))
                            } else {
                                count(total as u64)
                            }
                        },
                        10.,
                        MUTED,
                    )),
            )
            .child(each(
                move || bounded_children(&s.current(), s.metric.get()),
                |node| node.path.clone(),
                move |node, _| child_row(s, node),
            ))
            .child(when(
                move || s.current().children.is_empty(),
                move || {
                    view().py(px(18.)).items_center().gap(px(5.)).child(caption(
                        move || {
                            if s.busy.get() {
                                "Scanning…".to_owned()
                            } else {
                                "Nothing below this node.".to_owned()
                            }
                        },
                        11.,
                        MUTED,
                    ))
                },
            )),
    )
}

fn child_row(s: State, node: ReadSignal<Arc<Node>>) -> impl IntoElement {
    let departing = (*node.get_untracked()).clone();
    cleanup(move || s.hover(departing, false));
    view()
        .py(px(6.))
        .px(px(5.))
        .gap(px(4.))
        .rounded(px(4.))
        .bg(move || {
            rgba(if s.selection.get().path == node.get().path {
                0x1b2a20ff
            } else {
                0
            })
        })
        .hover(|s| s.bg(rgb(0x1a1a1e)))
        .role(AccessibilityRole::Button)
        .label(move || format!("Open {}", node.get().name))
        .on_press(move |_: &()| {
            let node = (*node.get_untracked()).clone();
            if node.kind == "session" {
                s.choose(node)
            } else {
                s.enter(node)
            }
        })
        .on_mouse_enter(move |_: &MouseEvent| s.hover((*node.get_untracked()).clone(), true))
        .on_mouse_leave(move |_: &MouseEvent| s.hover((*node.get_untracked()).clone(), false))
        .child(
            row()
                .gap(px(6.))
                .child(
                    view()
                        .w(px(3.))
                        .h(px(15.))
                        .bg(move || rgb(tile_color(&node.get(), 1))),
                )
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(2.))
                        .child(
                            caption(move || node.get().name.clone(), 11., INK)
                                .number_of_lines(1)
                                .text_color(move || {
                                    rgb(if s.selection.get().path == node.get().path {
                                        GREEN
                                    } else {
                                        INK
                                    })
                                }),
                        )
                        .child(
                            caption(
                                move || {
                                    let node = node.get();
                                    if node.kind == "session" {
                                        if node.model.is_empty() {
                                            node.title.clone()
                                        } else {
                                            node.model.clone()
                                        }
                                    } else if node.kind == "agent" {
                                        format!("{} projects", count(node.children.len() as u64))
                                    } else {
                                        format!("{} sessions", count(node.sessions))
                                    }
                                },
                                9.,
                                MUTED,
                            )
                            .number_of_lines(1),
                        ),
                )
                .child(caption(
                    move || s.metric.get().value(&node.get()),
                    10.,
                    MUTED,
                )),
        )
        .child(meter(
            move || {
                let metric = s.metric.get();
                metric.weight(&node.get()) as f32 / metric.weight(&s.current()).max(1.0) as f32
            },
            2.,
            agent_color(&node.get().tool),
        ))
}

fn details_page(s: State) -> impl IntoElement {
    let node = move || s.selection.get();
    let metric = move || s.metric.get();
    scroll().flex_1().child(
        view()
            .p(px(16.))
            .gap(px(12.))
            .child(
                row()
                    .child(
                        view()
                            .w(px(3.))
                            .h(px(19.))
                            .bg(move || rgb(tile_color(&node(), 1))),
                    )
                    .child(
                        caption(move || node().name, 20., INK)
                            .font_weight(FontWeight::SEMIBOLD)
                            .flex_1()
                            .number_of_lines(3),
                    ),
            )
            .child(caption(
                move || {
                    let node = node();
                    if node.kind == "session" {
                        if node.title.is_empty() {
                            node.file.clone()
                        } else {
                            node.title.clone()
                        }
                    } else if node.kind == "agent" {
                        format!("Agent · {} projects", count(node.children.len() as u64))
                    } else {
                        format!("Project · {}", node.title)
                    }
                },
                11.,
                MUTED,
            ))
            .child(caption(move || metric().value(&node()), 38., INK))
            .child(meter(
                move || {
                    metric().weight(&node()) as f32 / metric().weight(&s.current()).max(1.0) as f32
                },
                4.,
                GREEN,
            ))
            .child(
                row()
                    .gap(px(14.))
                    .child(stat("AGENT", move || node().tool))
                    .child(stat("SESSIONS", move || count(node().sessions))),
            )
            .child(
                row()
                    .gap(px(14.))
                    .child(stat("MESSAGES", move || count(node().messages)))
                    .child(stat("MODEL", move || {
                        let model = node().model;
                        if model.is_empty() {
                            "—".into()
                        } else {
                            model
                        }
                    })),
            )
            .child(
                row()
                    .gap(px(14.))
                    .child(stat("INPUT", move || compact(node().input)))
                    .child(stat("OUTPUT", move || compact(node().output))),
            )
            .child(
                row()
                    .gap(px(14.))
                    .child(stat("CACHE READ", move || compact(node().cache_read)))
                    .child(stat("CACHE WRITE", move || compact(node().cache_write))),
            )
            .child(
                row()
                    .gap(px(14.))
                    .child(stat("COST", move || money(node().cost)))
                    .child(stat("LAST", move || {
                    if numbers_hidden() {
                        "•••".into()
                    } else {
                        stamp(node().last_ts)
                    }
                })),
            )
            .child(caption(move || node().file, 9., MUTED).number_of_lines(3))
            .child(when(
                move || node().kind != "session",
                move || {
                    row().gap(px(6.)).child(button(
                        "Open",
                        move || s.busy.get(),
                        move || s.enter(s.selection.get_untracked()),
                    ))
                },
            )),
    )
}

fn sources_footer(s: State) -> impl IntoElement {
    let agents = computed(move || {
        s.snapshot
            .get()
            .root
            .children
            .iter()
            .map(|agent| (agent.name.clone(), agent_color(&agent.tool), agent.sessions))
            .collect::<Vec<_>>()
    });
    view()
        .p(px(14.))
        .gap(px(8.))
        .border_t_1()
        .border_color(rgb(LINE))
        .bg(rgb(0x141416))
        .child(caption("SOURCES", 9., MUTED))
        .child(each(
            move || agents.get(),
            |agent| agent.0.clone(),
            move |agent, _| {
                row()
                    .justify_between()
                    .child(
                        row()
                            .gap(px(6.))
                            .child(
                                view()
                                    .w(px(7.))
                                    .h(px(7.))
                                    .rounded(px(2.))
                                    .bg(move || rgb(agent.get().1)),
                            )
                            .child(caption(move || agent.get().0, 10., INK)),
                    )
                    .child(caption(move || count(agent.get().2), 10., MUTED))
            },
        ))
        .child(caption(move || s.message.get(), 9., MUTED).number_of_lines(3))
}

fn statusbar(s: State) -> impl IntoElement {
    row()
        .test_id("statusbar")
        .h(px(28.))
        .flex_shrink(0.)
        .gap(px(16.))
        .px(px(12.))
        .border_t_1()
        .border_color(rgb(LINE))
        .bg(rgb(PANEL))
        .child(when(
            move || s.busy.get(),
            move || {
                caption(
                    move || {
                        let p = s.progress.get().unwrap_or_default();
                        format!(
                            "{} · {} / {} files · {} sessions · {}s",
                            p.phase,
                            count(p.done),
                            count(p.total),
                            count(p.sessions),
                            s.seconds.get()
                        )
                    },
                    10.,
                    GREEN,
                )
                .number_of_lines(1)
                .flex_1()
            },
        ))
        .child(when(
            move || !s.busy.get(),
            move || {
                row()
                    .flex_1()
                    .gap(px(16.))
                    .child(caption(
                        move || {
                            let root = s.snapshot.get().root;
                            format!(
                                "{} sessions · {} messages · {} tokens · {} cache · {}",
                                count(root.sessions),
                                count(root.messages),
                                compact(root.tokens()),
                                compact(root.cache()),
                                money(root.cost)
                            )
                        },
                        10.,
                        MUTED,
                    ))
                    .child(
                        caption(move || s.message.get(), 10., MUTED)
                            .number_of_lines(1)
                            .flex_1(),
                    )
            },
        ))
        .child(
            caption(
                if cfg!(target_os = "macos") {
                    "⌘R Rescan   ⇧⌘S Share   ⌥↑ Parent   ↑↓ Select   Enter Open   Esc Back"
                } else {
                    "Ctrl+R Rescan   Ctrl+Shift+S Share   Alt+↑ Parent   ↑↓ Select   Enter Open   Esc Back"
                },
                10.,
                MUTED,
            )
            .number_of_lines(1),
        )
        .child(caption("color = agent · area = metric", 10., AMBER).number_of_lines(1))
}
