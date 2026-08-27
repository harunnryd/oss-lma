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
| Docker Desktop | only if you use the [Virtual Participant](virtual-participant.md); its containers dial the host sidecar over `host.docker.internal` |

## Capture permissions

**macOS** grants access through two privacy prompts:

- **Microphone** — for your side of the conversation.
- **Screen Recording** — required even for audio-only system capture, because
  the loopback path goes through ScreenCaptureKit.

Both permissions attach to the bundled `.app` identity. Launching inner
binaries directly from a terminal attributes the permission to the terminal
and silently fails.

For a source build, use `cargo tauri dev` to exercise capture. For a packaged
build, grant **Microphone** and **Screen & System Audio Recording** to the
bundled `oss-lma.app` in **System Settings → Privacy & Security**, then quit
and relaunch that app. Do not grant the permissions only to Terminal, `cargo`,
or the nested executable: that does not authorize the bundled app.

**Windows** needs no permission for system-audio loopback — WASAPI provides
it natively. The only gate is the microphone privacy setting.

> During development, ad-hoc signing pins the macOS TCC identity per build;
> rebuilds invalidate prior grants. Install a persistent self-signed
> certificate to keep them stable across iterations.

### Capture device selection (macOS)

The system-audio source always follows the **current macOS default output**.
Although the capture API reports output devices for diagnostics, choosing a
specific system-output ID is rejected because ScreenCaptureKit cannot honor
that override. Change the default output in macOS, then start a new meeting.

The microphone may be left on the macOS default or set to a listed microphone
ID in Settings → Capture. An explicit microphone remains selected across
default-device changes. Device selections cannot change while a meeting is
active. If the selected microphone disconnects, capture rebuilds that source
when it returns; the meeting stays active unless the rebuild itself fails.

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

## Verify your install

Run this smoke checklist in order after a fresh build or update. Stop at the
first failing step; [Troubleshooting → Diagnostics](troubleshooting.md#diagnostics)
explains how to gather state for each surface.

1. **Launch** — run `cargo tauri dev`, or open the bundled app.
   *Expected:* tray/menu-bar icon appears, the live-view window opens, and
   the terminal prints `SIDECAR_READY port=<port> token=<hex>`.
   *If it fails:* window missing or the line absent — check the terminal and
   `<app-data>/logs/` for a sidecar crash or port-bind failure
   ([Troubleshooting → Sidecar lifecycle](troubleshooting.md#sidecar-lifecycle)).

2. **Providers configured** — Settings → Providers: pick an STT provider and
   an LLM provider, paste API keys.
   *Expected:* both save without error into the OS keychain.
   *If it fails:* keychain prompt denied → re-grant and re-save;
   `STT_PROVIDER_AUTH` on step 4 → re-paste the key and confirm the account
   has streaming entitlements.

3. **Devices visible, meters moving** — Settings → Capture: select your
   microphone if needed, then play any audio on the machine.
   *Expected:* your mic appears in the device list and both VU meters move
   while audio plays.
   *If it fails:* empty list or flat meters means capture permissions were
   granted to the wrong bundle ([Capture permissions](#capture-permissions),
   [Troubleshooting → Capture](troubleshooting.md#capture)).

4. **30-second test recording** — press **Record**, join or play a meeting
   (anything audible through the speakers works), say a sentence aloud, then
   press **Stop**.
   *Expected:* the meeting appears under **Meetings** with transcript text,
   and the mixdown is exactly
   `<app-data>/recordings/<meeting_id>/audio.wav` (for example, on macOS):

   ```bash
   find ~/Library/Application\ Support/oss-lma/recordings \\
     -path '*/audio.wav' -type f -print
   ```

   ```powershell
   Get-ChildItem "$env:APPDATA\oss-lma\recordings"             # Windows
   ```

   *If it fails:* meeting created but no transcript → STT provider issue
   ([Troubleshooting → Transcription](troubleshooting.md#transcription));
   no meeting row at all → audio never reached the sidecar, recheck step 3
   and the sidecar log.

5. **Assistant answers one question** — open the chat panel on that meeting
   and ask "What did we talk about?".
   *Expected:* a streamed answer referencing the sentence you spoke, with a
   thinking-step timeline.
   *If it fails:* `AGENT_TOOL_FAILURE` steps or an empty reply point at the
   LLM key or model selection
   ([Troubleshooting → Assistant](troubleshooting.md#assistant)).

## Logs and data locations

Everything lives under the per-user application-data directory:

| OS | Base path (`<app-data>`) |
|---|---|
| macOS | `~/Library/Application Support/oss-lma/` |
| Windows | `%APPDATA%\oss-lma\` |

```text
<app-data>/
├── lma.db          # single SQLite WAL database (+ -wal/-shm siblings)
├── logs/           # sidecar stdout/stderr in bundled launches
└── recordings/
    └── <meeting_id>/   # audio.wav; video.mp4 for VP sessions
```

| Item | Path | Notes |
|---|---|---|
| Database | `<app-data>/lma.db` | inspect read-only — see [Troubleshooting → Diagnostics](troubleshooting.md#database-inspection) |
| Recordings | `<app-data>/recordings/<meeting_id>/` | raw stereo mix per meeting; video only for [Virtual Participant](virtual-participant.md) sessions |
| Logs | `<app-data>/logs/` | with `cargo tauri dev`, sidecar output goes to the foreground terminal instead |

Deleting the directory resets the database and recordings completely.
Keychain entries are not stored here — removing the directory does not remove
saved API keys.
