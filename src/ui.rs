//! Native Rust components for the session treemap.
use crate::{
    model::*,
    state::{State, now_millis},
};
use nsh_base::{Input, dialog, input, nav_page, nav_stack, tooltip};
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

fn tip(text: &'static str) -> impl IntoTooltip {
    tooltip().child(
        view()
            .px(px(8.))
            .py(px(5.))
            .rounded(px(6.))
            .bg(rgb(0x1c1c22))
            .border_1()
            .border_color(rgb(0x3a3a42))
            .child(caption(text, 10., INK).number_of_lines(1)),
    )
}

fn search_input(
    value: Signal<String>,
    placeholder: &'static str,
    width: f32,
    test_id: &'static str,
    handle: Option<&ElementRef>,
) -> impl IntoElement {
    view()
        .w(px(width))
        .h(px(28.))
        .flex_row()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .rounded(px(7.))
        .border_1()
        .border_color(rgb(0x2f2f36))
        .bg(rgb(0x141418))
        .window_drag(false)
        .child(caption("⌕", 12., MUTED))
        .child(
            view()
                .flex_1()
                .h(px(24.))
                .child(when(
                    move || value.get().is_empty(),
                    move || {
                        caption(placeholder, 11., MUTED)
                            .absolute()
                            .left(px(4.))
                            .top(px(5.))
                            .number_of_lines(1)
                    },
                ))
                .child({
                    let field = input()
                        .w(relative(1.))
                        .h(px(24.))
                        .test_id(test_id)
                        .value(move || value.get())
                        .placeholder("")
                        .caret_color(rgb(GREEN))
                        .selection_color(rgba(0x65f19340))
                        .text_color(rgb(INK))
                        .on_key_press(move |key: &str| {
                            if key == "escape" {
                                value.set(String::new());
                            }
                        })
                        .on_change(move |text: &str| value.set(text.to_owned()));
                    match handle {
                        Some(handle) => field.element_ref(handle),
                        None => field,
                    }
                }),
        )
        .child(when(
            move || !value.get().is_empty(),
            move || {
                view()
                    .role(AccessibilityRole::Button)
                    .label("Clear search")
                    .w(px(18.))
                    .h(px(18.))
                    .rounded(px(4.))
                    .items_center()
                    .justify_center()
                    .hover(|v| v.bg(rgb(0x1f1f24)))
                    .on_press(move |_: &()| value.set(String::new()))
                    .child(caption("✕", 10., MUTED))
            },
        ))
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
        "▥" => "M5 20V10M12 20V4M19 20v-7",
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
    let search_field = element_ref();
    let keys = KeyOptions {
        when: When::NotEditing,
        ..Default::default()
    };
    on_key_with("mod+r", keys, move |_| s.scan());
    on_key_with(
        "mod+f",
        KeyOptions {
            when: When::Always,
            ..Default::default()
        },
        move |_| search_field.focus(),
    );
    on_key_with("alt+up", keys, move |_| s.up());
    on_key_with("enter", keys, move |_| s.enter(s.selection.get_untracked()));
    on_key_with(
        "escape",
        KeyOptions {
            when: When::Always,
            ..Default::default()
        },
        move |_| {
            if !s.query.get_untracked().is_empty() {
                s.query.set(String::new());
            } else if !s.project_query.get_untracked().is_empty() {
                s.project_query.set(String::new());
            } else {
                s.close();
            }
        },
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
        .child(toolbar(s, &search_field))
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
        .child(when(
            move || s.compare_open.get(),
            move || compare_dialog(s),
        ))
        .child(when(
            move || s.custom_open.get(),
            move || custom_range_dialog(s),
        ))
}

fn toolbar(s: State, search_field: &ElementRef) -> impl IntoElement {
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
        .child(search_input(
            s.query,
            "Search sessions, projects, models",
            280.,
            "global-search",
            Some(search_field),
        ))
        .child(view().flex_1())
        .child(
            control(
                "Share image",
                Some("↗"),
                move || s.busy.get(),
                move || s.share_open.set(true),
            )
            .tooltip(tip("Share a screenshot (⇧⌘S)")),
        )
        .child(
            control(
                "Rescan sessions",
                Some("⟳"),
                move || s.busy.get(),
                move || s.scan(),
            )
            .tooltip(tip("Rescan local sessions (⌘R)")),
        )
        .child(
            control(
                "Up one level",
                Some("↑"),
                move || s.busy.get() || s.scope.get() == "root",
                move || s.up(),
            )
            .tooltip(tip("Up one level (⌥↑)")),
        )
        .child(
            control(
                "Compare agents",
                Some("▥"),
                move || s.busy.get(),
                move || s.compare_open.set(true),
            )
            .tooltip(tip("Compare agent usage")),
        )
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

fn range_control(s: State) -> impl IntoElement {
    let options = [
        TimeRange::All,
        TimeRange::Last7,
        TimeRange::Last30,
        TimeRange::Last90,
        TimeRange::Custom { start: 0, end: 0 },
    ];
    row()
        .gap(px(1.))
        .p(px(2.))
        .rounded(px(7.))
        .bg(rgb(0x0b0b0d))
        .border_1()
        .border_color(rgb(LINE))
        .children(options.into_iter().map(move |range| {
            let custom = matches!(range, TimeRange::Custom { .. });
            view()
                .role(AccessibilityRole::Tab)
                .label(range.title())
                .accessibility_state(move || nsh_core::AccessibilityState {
                    selected: if custom {
                        matches!(s.range.get(), TimeRange::Custom { .. })
                    } else {
                        s.range.get() == range
                    },
                    ..Default::default()
                })
                .h(px(22.))
                .px(px(if custom { 8. } else { 7. }))
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .bg(move || {
                    rgba(
                        if if custom {
                            matches!(s.range.get(), TimeRange::Custom { .. })
                        } else {
                            s.range.get() == range
                        } {
                            0x24242aff
                        } else {
                            0
                        },
                    )
                })
                .hover(|v| v.bg(rgb(0x1a1a1f)))
                .on_press(move |_: &()| {
                    if custom {
                        s.open_custom();
                    } else {
                        s.set_range(range);
                    }
                })
                .child(caption(range.label(), 9., INK).text_color(move || {
                    rgb(
                        if if custom {
                            matches!(s.range.get(), TimeRange::Custom { .. })
                        } else {
                            s.range.get() == range
                        } {
                            INK
                        } else {
                            MUTED
                        },
                    )
                }))
        }))
}

fn date_input(value: Signal<String>, test_id: &'static str) -> Input {
    input()
        .w(px(122.))
        .h(px(28.))
        .test_id(test_id)
        .value(move || value.get())
        .placeholder("YYYY-MM-DD")
        .placeholder_color(rgb(MUTED))
        .text_color(rgb(INK))
        .caret_color(rgb(GREEN))
        .selection_color(rgba(0x65f19340))
        .on_change(move |text: &str| value.set(text.to_owned()))
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
        .child(range_control(s))
        .child(
            view()
                .w(px(250.))
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

fn compare_number(value: impl IntoReactive<SharedString>, width: f32) -> impl IntoElement {
    view()
        .w(px(width))
        .flex_row()
        .justify_end()
        .child(caption(value, 10., MUTED))
}

fn compare_head(label: &'static str, width: f32) -> impl IntoElement {
    view()
        .w(px(width))
        .flex_row()
        .justify_end()
        .child(caption(label, 9., MUTED))
}

fn compare_metric_tabs(s: State) -> impl IntoElement {
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
                        .h(px(24.))
                        .px(px(10.))
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
                        .on_press(move |_: &()| s.metric.set(metric))
                        .child(caption(metric.label(), 10., INK).text_color(move || {
                            rgb(if s.metric.get() == metric { INK } else { MUTED })
                        }))
                }),
        )
}

fn compare_row(s: State, agent: ReadSignal<Arc<Node>>) -> impl IntoElement {
    view()
        .role(AccessibilityRole::Button)
        .label(move || format!("Open {}", agent.get().name))
        .px(px(8.))
        .py(px(6.))
        .rounded(px(5.))
        .hover(|v| v.bg(rgb(0x1a1a1f)))
        .on_press(move |_: &()| {
            let agent = (*agent.get_untracked()).clone();
            s.compare_open.set(false);
            s.enter(agent);
        })
        .child(
            row()
                .gap(px(4.))
                .child(
                    view()
                        .w(px(3.))
                        .h(px(16.))
                        .bg(move || rgb(agent_color(&agent.get().tool))),
                )
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(1.))
                        .child(
                            caption(move || agent.get().name.clone(), 11., INK).number_of_lines(1),
                        )
                        .child(
                            caption(move || agent.get().tool.clone(), 9., MUTED).number_of_lines(1),
                        ),
                )
                .child(compare_number(move || count(agent.get().sessions), 66.))
                .child(compare_number(move || compact(agent.get().input), 66.))
                .child(compare_number(move || compact(agent.get().output), 66.))
                .child(compare_number(move || compact(agent.get().cache()), 66.))
                .child(compare_number(move || money(agent.get().cost), 66.))
                .child(compare_number(move || count(agent.get().messages), 66.))
                .child(compare_number(
                    move || {
                        if numbers_hidden() {
                            return "•••".to_owned();
                        }
                        let total = s.metric.get().weight(&s.snapshot.get().root);
                        if total <= 0. {
                            "—".to_owned()
                        } else {
                            format!("{:.1}%", s.metric.get().weight(&agent.get()) / total * 100.)
                        }
                    },
                    66.,
                )),
        )
}

fn compare_dialog(s: State) -> impl IntoElement {
    let agents = computed(move || {
        let metric = s.metric.get();
        let mut agents = s.snapshot.get().root.children.clone();
        agents.sort_by(|a, b| {
            metric
                .weight(b)
                .partial_cmp(&metric.weight(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        agents
    });
    dialog()
        .label("Compare agents")
        .animation(nsh_core::ModalAnimation::None)
        .close_on_escape(true)
        .close_on_backdrop_press(false)
        .on_cancel(move || s.compare_open.set(false))
        .child(
            nsh_core::element::overlay()
                .inset_0()
                .bg(rgba(0x000000aa))
                .items_center()
                .justify_center()
                .on_press(move |_: &()| s.compare_open.set(false))
                .child(
                    view()
                        .w(px(840.))
                        .max_h(px(720.))
                        .p(px(20.))
                        .gap(px(12.))
                        .rounded(px(16.))
                        .bg(rgb(PANEL))
                        .border_1()
                        .border_color(rgb(LINE))
                        .on_press(move |_: &()| {})
                        .child(
                            row()
                                .justify_between()
                                .child(
                                    caption("Compare agents", 18., INK)
                                        .font_weight(FontWeight::SEMIBOLD),
                                )
                                .child(
                                    view()
                                        .role(AccessibilityRole::Button)
                                        .label("Close compare")
                                        .w(px(28.))
                                        .h(px(28.))
                                        .rounded(px(6.))
                                        .items_center()
                                        .justify_center()
                                        .hover(|v| v.bg(rgb(0x1f1f24)))
                                        .on_press(move |_: &()| s.compare_open.set(false))
                                        .child(caption("✕", 14., MUTED)),
                                ),
                        )
                        .child(caption(
                            move || {
                                format!(
                                    "All agents side by side · {}. Pick a metric, compare totals, then click a row to open that agent.",
                                    s.range.get().title()
                                )
                            },
                            11.,
                            MUTED,
                        ))
                        .child(
                            row()
                                .justify_between()
                                .child(compare_metric_tabs(s))
                                .child(caption(
                                    move || format!("Ranked by {}", s.metric.get().label().to_lowercase()),
                                    9.,
                                    MUTED,
                                )),
                        )
                        .child(
                            view()
                                .px(px(8.))
                                .py(px(4.))
                                .border_b_1()
                                .border_color(rgb(LINE))
                                .child(
                                    row()
                                        .gap(px(4.))
                                        .child(caption("AGENT", 9., MUTED).flex_1())
                                        .child(compare_head("SESSIONS", 66.))
                                        .child(compare_head("INPUT", 66.))
                                        .child(compare_head("OUTPUT", 66.))
                                        .child(compare_head("CACHE", 66.))
                                        .child(compare_head("COST", 66.))
                                        .child(compare_head("MSGS", 66.))
                                        .child(compare_head("SHARE", 66.)),
                                ),
                        )
                        .child(
                            scroll().max_h(px(430.)).child(
                                view()
                                    .gap(px(2.))
                                    .child(each(
                                        move || agents.get(),
                                        |agent| agent.path.clone(),
                                        move |agent, _| compare_row(s, agent),
                                    ))
                                    .child(when(move || agents.get().is_empty(), move || {
                                        view().py(px(18.)).items_center().child(caption(
                                            "No agents found yet.",
                                            10.,
                                            MUTED,
                                        ))
                                    })),
                            ),
                        )
                        .child(
                            view()
                                .px(px(8.))
                                .py(px(7.))
                                .border_t_1()
                                .border_color(rgb(LINE))
                                .child(
                                    row()
                                        .gap(px(4.))
                                        .child(
                                            caption("TOTAL", 10., INK)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .flex_1(),
                                        )
                                        .child(compare_number(
                                            move || count(s.snapshot.get().root.sessions),
                                            66.,
                                        ))
                                        .child(compare_number(
                                            move || compact(s.snapshot.get().root.input),
                                            66.,
                                        ))
                                        .child(compare_number(
                                            move || compact(s.snapshot.get().root.output),
                                            66.,
                                        ))
                                        .child(compare_number(
                                            move || compact(s.snapshot.get().root.cache()),
                                            66.,
                                        ))
                                        .child(compare_number(
                                            move || money(s.snapshot.get().root.cost),
                                            66.,
                                        ))
                                        .child(compare_number(
                                            move || count(s.snapshot.get().root.messages),
                                            66.,
                                        ))
                                        .child(compare_number(
                                            move || {
                                                if numbers_hidden() {
                                                    "•••".to_owned()
                                                } else if s.metric.get().weight(&s.snapshot.get().root) <= 0. {
                                                    "—".to_owned()
                                                } else {
                                                    "100.0%".to_owned()
                                                }
                                            },
                                            66.,
                                        )),
                                ),
                        ),
                ),
        )
}

fn custom_range_dialog(s: State) -> impl IntoElement {
    dialog()
        .label("Custom date range")
        .animation(nsh_core::ModalAnimation::None)
        .close_on_escape(true)
        .close_on_backdrop_press(false)
        .on_cancel(move || s.custom_open.set(false))
        .child(
            nsh_core::element::overlay()
                .inset_0()
                .bg(rgba(0x000000aa))
                .items_center()
                .justify_center()
                .on_press(move |_: &()| s.custom_open.set(false))
                .child(
                    view()
                        .w(px(390.))
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
                                    caption("Custom range", 18., INK)
                                        .font_weight(FontWeight::SEMIBOLD),
                                )
                                .child(
                                    view()
                                        .role(AccessibilityRole::Button)
                                        .label("Close custom range")
                                        .w(px(28.))
                                        .h(px(28.))
                                        .rounded(px(6.))
                                        .items_center()
                                        .justify_center()
                                        .hover(|v| v.bg(rgb(0x1f1f24)))
                                        .on_press(move |_: &()| s.custom_open.set(false))
                                        .child(caption("✕", 14., MUTED)),
                                ),
                        )
                        .child(caption(
                            "Sessions are included by their last activity, in local time.",
                            11.,
                            MUTED,
                        ))
                        .child(
                            row()
                                .gap(px(8.))
                                .child(date_input(s.custom_start, "range-start"))
                                .child(caption("to", 11., MUTED))
                                .child(date_input(s.custom_end, "range-end")),
                        )
                        .child(
                            caption(move || s.custom_error.get(), 10., MUTED)
                                .text_color(rgb(AMBER))
                                .number_of_lines(2),
                        )
                        .child(
                            row()
                                .justify_end()
                                .gap(px(8.))
                                .child(button("Cancel", false, move || s.custom_open.set(false)))
                                .child(button("Apply range", false, move || s.apply_custom())),
                        ),
                ),
        )
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
                .border_color(move || rgb(if on() { GREEN } else { 0x3a3a42 }))
                .bg(move || if on() { rgb(GREEN) } else { rgba(0x00000000) })
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
        if !s.query.get().trim().is_empty() {
            return false;
        }
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
                        "•••".to_owned()
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
    view()
        .flex_1()
        .child(when(
            move || !s.query.get().trim().is_empty(),
            move || search_contents(s),
        ))
        .child(when(
            move || s.query.get().trim().is_empty(),
            move || browse_contents(s),
        ))
}

fn activity_chart(s: State) -> impl IntoElement {
    let series = computed(move || {
        let range = s.range.get();
        let end = range.chart_end(now_millis());
        daily_series(&s.current(), s.metric.get(), range.chart_days(), end)
    });
    let peak = computed(move || series.get().into_iter().fold(0.0_f64, f64::max));
    let label = move || {
        let range = s.range.get();
        match range {
            TimeRange::All => "LAST 30 DAYS".to_owned(),
            TimeRange::Custom { .. } => "CUSTOM RANGE".to_owned(),
            _ => format!("LAST {} DAYS", range.chart_days()),
        }
    };
    view()
        .gap(px(6.))
        .py(px(10.))
        .border_b_1()
        .border_color(rgb(LINE))
        .child(
            row()
                .justify_between()
                .child(caption("ACTIVITY", 9., MUTED))
                .child(caption(label, 9., MUTED)),
        )
        .child(
            view()
                .h(px(46.))
                .flex_row()
                .items_end()
                .gap(px(1.))
                .child(each(
                    move || series.get().into_iter().enumerate().collect::<Vec<_>>(),
                    |(index, _)| *index,
                    move |item, _| {
                        view()
                            .flex_1()
                            .min_w(px(1.))
                            .h(move || {
                                let value = item.get().1;
                                let peak = peak.get();
                                let fraction = if peak <= 0.0 {
                                    0.04
                                } else {
                                    (value / peak).max(0.04) as f32
                                };
                                px((46.0 * fraction).max(if value > 0.0 { 2.0 } else { 1.0 }))
                            })
                            .rounded_t(px(2.))
                            .bg(move || {
                                let value = item.get().1;
                                rgb(if value <= 0.0 { 0x2a2a31 } else { GREEN })
                            })
                    },
                )),
        )
}

fn browse_contents(s: State) -> impl IntoElement {
    scroll().flex_1().child(
        view()
            .p(px(14.))
            .gap(px(9.))
            .child(scope_summary(s))
            .child(activity_chart(s))
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

fn search_hits(root: &Node, query: &str, metric: Metric) -> Vec<Arc<Node>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let score = |node: &Node| -> u8 {
        let name = node.name.to_lowercase();
        if name.starts_with(&query) {
            3
        } else if name.contains(&query) {
            2
        } else {
            let extra = if node.kind == "session" {
                format!("{} {} {}", node.title, node.model, node.file)
            } else if node.kind == "project" {
                node.title.clone()
            } else {
                String::new()
            };
            if extra.to_lowercase().contains(&query) {
                1
            } else {
                0
            }
        }
    };
    let mut hits: Vec<(u8, Arc<Node>)> = Vec::new();
    for agent in &root.children {
        let agent_score = score(agent);
        if agent_score > 0 {
            hits.push((agent_score, agent.clone()));
        }
        for project in &agent.children {
            let project_score = score(project);
            if project_score > 0 {
                hits.push((project_score, project.clone()));
            }
            for session in &project.children {
                let session_score = score(session);
                if session_score > 0 {
                    hits.push((session_score, session.clone()));
                }
            }
        }
    }
    hits.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            metric
                .weight(&b.1)
                .partial_cmp(&metric.weight(&a.1))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    hits.truncate(60);
    hits.into_iter().map(|(_, node)| node).collect()
}

fn search_contents(s: State) -> impl IntoElement {
    let hits =
        computed(move || search_hits(&s.snapshot.get().root, &s.query.get(), s.metric.get()));
    scroll().flex_1().child(
        view()
            .p(px(14.))
            .gap(px(8.))
            .child(
                row()
                    .justify_between()
                    .child(caption("SEARCH", 10., MUTED))
                    .child(caption(
                        move || format!("{} results", hits.get().len()),
                        10.,
                        MUTED,
                    )),
            )
            .child(each(
                move || hits.get(),
                |node| node.path.clone(),
                move |node, _| search_row(s, node),
            ))
            .child(when(
                move || hits.get().is_empty(),
                move || {
                    view().py(px(18.)).items_center().child(caption(
                        "No matching agents, projects or sessions.",
                        11.,
                        MUTED,
                    ))
                },
            )),
    )
}

fn search_row(s: State, node: ReadSignal<Arc<Node>>) -> impl IntoElement {
    view()
        .py(px(6.))
        .px(px(5.))
        .gap(px(3.))
        .rounded(px(4.))
        .hover(|s| s.bg(rgb(0x1a1a1e)))
        .role(AccessibilityRole::Button)
        .label(move || format!("Open {}", node.get().name))
        .on_press(move |_: &()| {
            let node = (*node.get_untracked()).clone();
            s.query.set(String::new());
            if node.kind == "agent" {
                s.enter(node);
            } else {
                s.choose(node);
            }
        })
        .child(
            row()
                .gap(px(6.))
                .child(
                    view()
                        .w(px(3.))
                        .h(px(16.))
                        .bg(move || rgb(tile_color(&node.get(), 1))),
                )
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(2.))
                        .child(
                            caption(move || node.get().name.clone(), 11., INK).number_of_lines(1),
                        )
                        .child(
                            caption(
                                move || {
                                    let node = node.get();
                                    if node.kind == "session" {
                                        if node.model.is_empty() {
                                            node.title.clone()
                                        } else {
                                            format!("{} · {}", node.tool, node.model)
                                        }
                                    } else if node.kind == "project" {
                                        format!("{} · {} sessions", node.tool, count(node.sessions))
                                    } else {
                                        format!(
                                            "Agent · {} projects",
                                            count(node.children.len() as u64)
                                        )
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
                            "—".to_owned()
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
                            "•••".to_owned()
                        } else {
                            stamp(node().last_ts)
                        }
                    })),
            )
            .child(when(
                move || node().kind == "project",
                move || project_sessions(s),
            ))
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

fn project_sessions(s: State) -> impl IntoElement {
    let sessions = computed(move || {
        let node = s.selection.get();
        let query = s.project_query.get().trim().to_lowercase();
        let metric = s.metric.get();
        let mut list: Vec<Arc<Node>> = node
            .children
            .iter()
            .filter(|session| {
                query.is_empty()
                    || session.name.to_lowercase().contains(&query)
                    || session.title.to_lowercase().contains(&query)
                    || session.model.to_lowercase().contains(&query)
                    || session.file.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        list.sort_by(|a, b| {
            metric
                .weight(b)
                .partial_cmp(&metric.weight(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total = list.len();
        list.truncate(120);
        (list, total)
    });
    view()
        .gap(px(7.))
        .pt(px(9.))
        .border_t_1()
        .border_color(rgb(LINE))
        .child(
            row()
                .justify_between()
                .child(caption("SESSIONS", 10., MUTED))
                .child(caption(
                    move || {
                        let (list, total) = sessions.get();
                        if total > list.len() {
                            format!("{} of {}", list.len(), count(total as u64))
                        } else {
                            count(total as u64)
                        }
                    },
                    9.,
                    MUTED,
                )),
        )
        .child(search_input(
            s.project_query,
            "Search this project",
            240.,
            "project-search",
            None,
        ))
        .child(each(
            move || sessions.get().0,
            |node| node.path.clone(),
            move |node, _| child_row(s, node),
        ))
        .child(when(
            move || sessions.get().0.is_empty(),
            move || {
                view().py(px(14.)).items_center().child(caption(
                    "No sessions match this search.",
                    10.,
                    MUTED,
                ))
            },
        ))
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
                                "{} · {} sessions · {} messages · {} tokens · {} cache · {}",
                                s.range.get().title(),
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
                    "⌘F Search   ⌘R Rescan   ⇧⌘S Share   ⌥↑ Parent   ↑↓ Select   Enter Open   Esc Back"
                } else {
                    "Ctrl+F Search   Ctrl+R Rescan   Ctrl+Shift+S Share   Alt+↑ Parent   ↑↓ Select   Enter Open   Esc Back"
                },
                10.,
                MUTED,
            )
            .number_of_lines(1),
        )
        .child(caption("color = agent · area = metric", 10., AMBER).number_of_lines(1))
}
