<p align="center">
  <img src="src/logo.svg" alt="WakaToken" width="112" height="112">
</p>

<h1 align="center">WakaToken</h1>

<p align="center">
  Track AI coding assistant token usage from your desktop and sync it to the WakaToken dashboard.
</p>

<p align="center">
  <a href="https://github.com/wakatoken/wakatoken/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/wakatoken/wakatoken/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/wakatoken/wakatoken/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/wakatoken/wakatoken?label=release"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-blue"></a>
</p>

WakaToken is a lightweight desktop app for developers who want token visibility across AI coding tools. It runs in the system tray, scans local session files incrementally, shows local usage in the app, and periodically syncs verified events to the [WakaToken dashboard](https://wkt.tftt.cc).

## Highlights

- **Local-first usage view** - See total tokens, today's activity, runtime distribution, and session-level usage before anything is uploaded.
- **Automatic collection** - Reads supported runtime session files incrementally, so you do not need manual tracking or API keys.
- **Browser sign-in** - Uses device login through WakaToken Cloud; credentials are stored separately from app settings.
- **Background sync** - Uploads usage every 5 minutes with deduplication and progress reporting.
- **Runtime controls** - Enable only the runtimes you want to monitor from onboarding or settings.
- **Auto-update** - Checks GitHub Releases and prompts before installing an available update.

## Supported Runtimes

| Runtime | Source |
|---|---|
| Claude Code | `~/.claude/projects/` |
| Codex CLI | `~/.codex/sessions/` |
| GitHub Copilot | `~/.copilot/session-state/*/events.jsonl` |
| Gemini CLI | `~/.gemini/tmp/*/chats/session-*` |

## Install

### macOS

```bash
brew tap wakatoken/tap
brew install --cask wakatoken
```

### Manual Download

Download the latest installer from [GitHub Releases](https://github.com/wakatoken/wakatoken/releases/latest).

| Platform | Asset |
|---|---|
| macOS Apple Silicon | `WakaToken_x.x.x_aarch64.dmg` |
| macOS Intel | `WakaToken_x.x.x_x64.dmg` |
| Linux Debian/Ubuntu | `WakaToken_x.x.x_amd64.deb` |
| Linux Fedora/RHEL | `WakaToken-x.x.x-1.x86_64.rpm` |
| Linux Universal | `WakaToken_x.x.x_amd64.AppImage` |
| Windows | `WakaToken_x.x.x_x64-setup.exe` |

## Setup

1. Launch WakaToken.
2. Choose which runtimes to monitor during onboarding.
3. Click **Sign in with Browser** and complete the device login flow.
4. Keep WakaToken running in the tray for background sync.

The main window opens on launch. You can reopen it from the tray menu at any time.

## How It Works

1. Scans supported runtime session directories.
2. Parses new entries and records local session summaries.
3. Extracts token counts, model, project, language, and tool context.
4. Deduplicates events by stable message IDs.
5. Uploads unsynced local records in batches.
6. Marks records as synced only after the server accepts them.

## Development

### Prerequisites

- [Rust](https://rustup.rs/) stable
- [Node.js](https://nodejs.org/)
- Tauri system dependencies for your platform

### Run Locally

```bash
npm install
npm run tauri dev
```

### Verify

```bash
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Build

```bash
npm run tauri build
```

## Project Layout

```text
src/                  Frontend HTML, JS, and CSS
src-tauri/src/
  lib.rs              App setup and Tauri commands
  config.rs           Runtime settings and onboarding state
  credentials.rs      Browser login token persistence
  heartbeat.rs        Token usage event model
  local_stats.rs      Local session store and dashboard queries
  collector/          Runtime-specific parsers
  scheduler.rs        Periodic sync orchestration
  reporter.rs         API batch uploader
  tray.rs             System tray menu
```

## License

Apache-2.0
