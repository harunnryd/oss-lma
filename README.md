# oss-lma

Local-first live meeting assistant. Captures meeting audio on your machine,
streams it through pluggable cloud speech-to-text engines, transcribes both
sides of the conversation in real time, and runs a LangGraph-powered assistant
over the live transcript and your meeting history. A headless browser bot can
join online meetings on your behalf.

## Features

- Live dual-channel transcription (system/meeting audio + microphone)
- Pluggable STT providers (Deepgram, AssemblyAI, Azure) behind one engine interface
- Assistant chat grounded in the current meeting and past meetings via local RAG
- Per-meeting summaries and structured action items
- Virtual participant bot that joins Zoom / Google Meet for you, with manual takeover
- Fully local storage: SQLite + recordings on disk, no accounts, no cloud backend

## Requirements

- macOS 13+ (grants Screen Recording and Microphone permission) or Windows 10+
- Python 3.12+ with [uv](https://docs.astral.sh/uv/), Rust stable with rustup
- API keys: at least one STT provider and one LLM provider
- Docker Desktop — only needed for the virtual participant

## Quick start

```bash
git clone <repo-url> oss-lma && cd oss-lma
uv sync                # python workspace: sidecar + lma_* packages
cargo tauri dev        # builds the shell, spawns the sidecar, opens the window
```

On first launch, pick your STT and LLM providers in Settings and paste their
API keys. Keys are stored in the OS keychain.

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
ui/           React TS webview
python/       uv workspace (sidecar + lma_* packages)
vp-container/ Docker recipe for the meeting bot
contracts/    Single source of truth: event schemas + error catalog
prompts/      Default prompt templates
```
