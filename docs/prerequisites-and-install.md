---
title: "Prerequisites & Installation"
---

# Prerequisites & Installation

oss-lma runs **entirely on your machine**: audio capture, transcription calls,
the assistant, storage, and recordings. There is no cloud backend to deploy
and no account to create — you bring your own provider API keys.

## System requirements

| Requirement | Details |
|---|---|
| OS | macOS 13+ or Windows 10+ |
| Python | 3.12+, managed by [uv](https://docs.astral.sh/uv/) |
| Rust | stable toolchain via rustup |
| Node | only for building the webview UI |
| Docker | only if you use the [Virtual Participant](virtual-participant.md) |

## Capture permissions

**macOS** grants access through two privacy prompts:

- **Microphone** — for your side of the conversation.
- **Screen Recording** — required even for audio-only system capture, because
  the loopback path goes through ScreenCaptureKit.

Both permissions attach to the bundled `.app` identity. Launching inner
binaries directly from a terminal attributes the permission to the terminal
and silently fails.

**Windows** needs no permission for system-audio loopback — WASAPI provides
it natively. The only gate is the microphone privacy setting.

> During development, ad-hoc signing pins the macOS TCC identity per build;
> rebuilds invalidate prior grants. Install a persistent self-signed
> certificate to keep them stable across iterations.

## Provider keys

Configured on first launch in Settings and stored in the **OS keychain** —
never in the database:

| Key | Used by | Required |
|---|---|---|
| STT provider (Deepgram / AssemblyAI / Azure) | live transcription | yes, one |
| LLM provider | assistant chat, summaries | yes, one |
| Embedding provider | past-meeting search | no — local embeddings used by default |
| Web search provider | assistant `web_search` tool | no |
| TTS provider | [voice assistant](voice-assistant.md) | no |

## Build from source

```bash
git clone <repo-url> oss-lma && cd oss-lma
uv sync             # python workspace: sidecar + lma_* packages
cargo tauri dev     # builds the shell, spawns the sidecar, opens the window
```

Production builds package the Python workspace into a bundled environment
next to the Rust binary — no system Python is required at runtime.

## Storage locations

Everything lives under the per-user application-data directory:

```
<app-data>/
├── lma.db          # single SQLite WAL database
└── recordings/
    └── <meeting_id>/   # audio.wav, video.mp4
```

Deleting the directory resets the app completely.
