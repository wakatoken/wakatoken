# WakaToken

A lightweight desktop app that tracks your AI coding assistant token usage. Runs silently in the system tray, collects usage data from Claude Code, Codex CLI, GitHub Copilot sessions, and Gemini CLI sessions, and syncs to the [WakaToken dashboard](https://wkt.tftt.cc).

## Features

- **Automatic collection** - Scans Claude Code, Codex CLI, GitHub Copilot, and Gemini CLI session files incrementally, no manual tracking needed
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

1. Launch WakaToken - the main window opens and the app also appears in the system tray
2. Click **Sign in with Browser**
3. Complete the device login flow in your browser
4. Choose which runtimes to monitor during onboarding or from **Settings**

The app will start syncing your Claude Code, Codex CLI, GitHub Copilot, and Gemini CLI token usage automatically.

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
  config.rs           # Runtime settings and onboarding state
  credentials.rs      # Browser login token persistence
  heartbeat.rs        # Token usage data model
  local_stats.rs      # Local session database and dashboard queries
  collector/          # Pluggable data collectors
    claude.rs         # Claude Code session parser
    codex.rs          # Codex CLI session parser
    copilot.rs        # GitHub Copilot session parser
    gemini.rs         # Gemini CLI session parser
  scheduler.rs        # Periodic sync orchestration
  reporter.rs         # API batch uploader
  tray.rs             # System tray menu
```

## How It Works

1. Scans `~/.claude/projects/`, `~/.codex/sessions/`, `~/.copilot/session-state/*/events.jsonl`, and `~/.gemini/tmp/*/chats/session-*`
2. Parses new entries incrementally (tracks byte offsets per file)
3. Extracts token counts, model, project, language, and tool context
4. Deduplicates by message ID
5. Uploads in batches of 100 to the WakaToken API
6. Commits offsets only after successful upload (retry on failure)

## License

Apache-2.0
