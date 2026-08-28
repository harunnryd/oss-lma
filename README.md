# oss-lma

Local-first live meeting transcription. Captures meeting audio on your machine,
streams it through pluggable cloud speech-to-text engines, transcribes both
sides of the conversation in real time, and stores meeting history locally.

## Features

- Live dual-channel transcription (system/meeting audio + microphone)
- Pluggable STT providers (Deepgram, AssemblyAI, Azure) behind one engine interface
- Local meeting history and transcript detail views
- Fully local storage: SQLite + recordings on disk, no accounts, no cloud backend

Assistant chat, summaries, semantic search, and Virtual Participant flows are
documented product increments that are not part of the current capture release.

## Requirements

- macOS 13+ (Windows capture is planned but not implemented yet)
- Python 3.12+ with [uv](https://docs.astral.sh/uv/), Rust stable with rustup
- Node.js 20+ with npm
- API key for at least one STT provider

## Quick start

```bash
git clone <repo-url> oss-lma && cd oss-lma
uv sync --all-packages
npm --prefix src ci
cargo tauri dev
```

On first launch, pick an STT provider in Settings and paste its API key. Keys
are stored in the OS keychain.

## Documentation

Full documentation lives in [`docs/`](docs/INDEX.md):

| Document | Contents |
|---|---|
| [Prerequisites & Installation](docs/prerequisites-and-install.md) | Requirements, permissions, provider keys, build |
| [Quick Start Guide](docs/quick-start-guide.md) | First meeting in five minutes |
| [Transcription & Translation](docs/transcription-and-translation.md) | STT providers, speakers, languages, recording |
| [Meeting Assistant](docs/meeting-assistant.md) | Agent chat, tools, wake phrase, MCP |
| [Transcript Summarization](docs/transcript-summarization.md) | Summaries, templates, action items |
| [Meetings Query Tool](docs/meetings-query-tool.md) | Semantic search across past meetings |
| [Desktop Capture App](docs/desktop-capture-app.md) | System + mic capture, controls |
| [Virtual Participant](docs/virtual-participant.md) | Meeting bot in a local container |
| [Developer Guide](docs/developer-guide.md) | Architecture and testing strategy |

## Repository layout

```
crates/       Rust workspace (app shell, capture, transport link)
src/          React TS webview
python/       uv workspace (sidecar + lma_* packages)
contracts/    Single source of truth: event schemas + error catalog
prompts/      Default prompt templates
```
