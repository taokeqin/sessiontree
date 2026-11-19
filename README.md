# Sessiontree

A native treemap explorer for the AI coding-agent sessions already on your machine.
Built with [Native Shell](https://nsh.dev) and Rust, it renders local history as a
drill-down map: **agent → project → session**.

![Sessiontree](assets/icon.png)

## What it reads

Sessiontree is passive: it only reads local stores, never proxies or uploads.

- Claude Code
- Codex
- Cursor
- GitHub Copilot
- OpenCode
- Pi
- DeepSeek Harness
- ZCode
- MiMo Code
- Antigravity (agy)
- Kiro

Sessions are weighted by **tokens**, **cost** or **messages**. Costs use a built-in
price table, including DeepSeek, GLM, MiMo, Qwen and Kimi model ids.

## Share screenshots

The toolbar's share button (⇧⌘S / Ctrl+Shift+S) captures the window exactly as shown
and copies it to the clipboard. Before copying, mask:

- agent names
- project names
- session titles
- counts, tokens, cost and dates

The masked labels and numbers are swapped in for the capture and restored immediately
afterwards. Nothing leaves the machine.

## Platforms

| Platform | Status |
| --- | --- |
| macOS 14+ | AppKit, universal arm64 + x86_64 |
| Windows 10 1809+ | Win32, x86_64 |
| Linux (GTK 4.14+) | GTK 4, x86_64 |

Native window capture is implemented on all three; if a platform cannot capture a
window, Sessiontree falls back to a rendered share image.

## Build

```sh
cargo run              # developer build
cargo test             # headless UI + parser tests
nsh package --target macos --arch universal --icon assets/icon.png
nsh package --target linux --icon assets/icon.png
nsh package --target windows --icon assets/icon.png
```

Rust app packaging uses `nsh` 0.6.0. CI builds macOS, Linux and Windows and publishes
all three archives to one GitHub release when a `v*` tag is pushed.

## Privacy

Everything is local. The app reads transcript files and SQLite stores, keeps a metadata
cache in the platform's application-support directory, and writes a screenshot to the
clipboard only when you ask.
