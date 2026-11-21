# Sessiontree

A native treemap explorer for the AI coding-agent sessions already on your machine.
Built with [Native Shell](https://nsh.dev) and Rust, it renders local history as a
drill-down map: **agent → project → session**.

![Sessiontree showing token and cost usage across masked agents, projects and sessions](assets/screenshot.png)

*This window uses Sessiontree's share mode: agent, project and session names are replaced
with `Agent N`, `Project N` and `Session N` before the screenshot is copied.*

## Share without leaking your work

Click **Share** in the toolbar (**⇧⌘S** on macOS, **Ctrl+Shift+S** on Windows and Linux)
to copy the current window as a PNG. Before copying, Sessiontree can replace:

- agent names
- project names
- session titles
- counts, tokens, cost and dates

The masked window is captured and the real data is restored immediately afterwards. The
screenshot above was taken this way: names are masked, so it is safe to paste into a PR,
an issue or a chat. Nothing is uploaded anywhere.

## Compare agents

The bar-chart button in the toolbar compares every agent side by side: sessions, input,
output, cache, cost, messages and each agent's share of the selected metric. Switch the
metric inside the dialog, or click a row to drill into that agent. Values are masked too
when you share the window.

## Time range and activity

The range chips in the map header filter every agent, project and session by its last
activity: **All**, **7D**, **30D**, **90D**, or a custom start and end date. The range applies
to the treemap, totals, search, compare sheet and share image, and the choice is remembered
between launches. The **Activity** bars in the right panel show the selected metric day by
day over the selected period (the last 30 days for **All**).

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

## Install

[GitHub](https://github.com/taokeqin/sessiontree) · [Releases](https://github.com/taokeqin/sessiontree/releases)

### macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/taokeqin/sessiontree/main/install.sh | sh
```

The installer downloads the latest release, verifies it against `SHA256SUMS.txt`, and
installs `Sessiontree.app` into `~/Applications` (macOS) or `Sessiontree/` into
`~/.local/share` with a `sessiontree` command and desktop entry (Linux). Run it again
to upgrade.

```sh
# A specific release
curl -fsSL https://raw.githubusercontent.com/taokeqin/sessiontree/main/install.sh | sh -s -- --version v0.3.0

# Another location (the folder that holds Sessiontree.app / Sessiontree/)
curl -fsSL https://raw.githubusercontent.com/taokeqin/sessiontree/main/install.sh | sh -s -- --dir /Applications
```

### Windows

```powershell
irm https://raw.githubusercontent.com/taokeqin/sessiontree/main/install.ps1 | iex
```

This installs to `%LOCALAPPDATA%\Sessiontree` and adds a Start menu shortcut. For a
specific release:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/taokeqin/sessiontree/main/install.ps1))) -Version v0.3.0
```

Linux needs GTK 4 and GStreamer at runtime, for example on Ubuntu:

```sh
sudo apt install libgtk-4-1 libgstreamer1.0-0 libgstreamer-plugins-base1.0-0
```

### Uninstall

- macOS: delete `~/Applications/Sessiontree.app`
- Linux: delete `~/.local/share/Sessiontree`, `~/.local/bin/sessiontree`, and the
  `com.hacken.sessiontree` desktop/icon files under `~/.local/share`
- Windows: delete `%LOCALAPPDATA%\Sessiontree` and the Sessiontree Start menu shortcut

## Search

The toolbar field searches every agent, project, session title, model id and file path
on the machine. Results stay in the right panel; selecting a project opens its details
with a searchable session list.

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

Rust app packaging uses `nsh` 0.6.0. CI uploads macOS, Linux and Windows archives as
artifacts on every `main` push; a `v*` tag publishes those archives to one GitHub
release. The install scripts are served from `raw.githubusercontent.com` and verify release
checksums.

## Privacy

Everything is local. The app reads transcript files and SQLite stores, keeps a metadata
cache in the platform's application-support directory, and writes a screenshot to the
clipboard only when you ask.
