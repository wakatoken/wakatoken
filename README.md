# WakaToken

A lightweight desktop app that tracks your AI coding assistant token usage. Runs silently in the system tray, collects usage data from Claude Code, Codex CLI, and GitHub Copilot sessions, and syncs to the [WakaToken dashboard](https://wkt.tftt.cc).

## Features

- **Automatic collection** - Scans Claude Code, Codex CLI, and GitHub Copilot session files incrementally, no manual tracking needed
- **System tray** - Runs in the background with a menu showing today's token usage and sync status
- **Periodic sync** - Uploads usage data every 5 minutes with deduplication
- **Auto-update** - Checks for new versions from GitHub Releases
- **Cross-platform** - macOS (Apple Silicon & Intel), Linux, Windows

## Install

### macOS (Homebrew)

```bash
brew tap wakatoken/tap
brew install --cask wakatoken
```

### Manual Download

Download from the [Releases page](https://github.com/wakatoken/wakatoken/releases/latest).

| Platform | File |
|---|---|
| macOS (Apple Silicon) | `WakaToken_x.x.x_aarch64.dmg` |
| macOS (Intel) | `WakaToken_x.x.x_x64.dmg` |
| Linux (Debian/Ubuntu) | `WakaToken_x.x.x_amd64.deb` |
| Linux (Fedora/RHEL) | `WakaToken-x.x.x-1.x86_64.rpm` |
| Linux (Universal) | `WakaToken_x.x.x_amd64.AppImage` |
| Windows | `WakaToken_x.x.x_x64-setup.exe` |

## Setup

1. Launch WakaToken - it appears as a tray icon
2. Click the tray icon -> **Settings...**
3. Enter your API key (get one from [wkt.tftt.cc](https://wkt.tftt.cc))
4. Click **Test Connection** to verify, then **Save**

The app will start syncing your Claude Code, Codex CLI, and GitHub Copilot token usage automatically.

## Development

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) (for Tauri CLI)

### Build & Run

```bash
npm install
npm run tauri dev
```

### Test

```bash
cd src-tauri
cargo test
```

### Build Release

```bash
npm run tauri build
```

## Architecture

```
src/                  # Frontend - settings window (HTML/JS/CSS)
src-tauri/src/
  lib.rs              # App setup & Tauri commands
  config.rs           # API key persistence (~/.config/com.wakatoken.client/)
  heartbeat.rs        # Token usage data model
  collector/          # Pluggable data collectors
    claude.rs         # Claude Code session parser
    codex.rs          # Codex CLI session parser
    copilot.rs        # GitHub Copilot session parser
  scheduler.rs        # Periodic sync orchestration
  reporter.rs         # API batch uploader
  tray.rs             # System tray menu
```

## How It Works

1. Scans `~/.claude/projects/`, `~/.codex/sessions/`, and `~/.copilot/session-state/*/events.jsonl`
2. Parses new entries incrementally (tracks byte offsets per file)
3. Extracts token counts, model, project, language, and tool context
4. Deduplicates by message ID
5. Uploads in batches of 100 to the WakaToken API
6. Commits offsets only after successful upload (retry on failure)

## License

Apache-2.0
