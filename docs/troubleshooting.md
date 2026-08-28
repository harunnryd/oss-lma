---
title: "Troubleshooting"
---

# Troubleshooting

Symptoms are indexed by error catalog code (see
[Developer Guide](developer-guide.md#error-catalog)); the machine-readable
catalog lives in `contracts/errors.yaml`.

## Diagnostics

Gather this state before working a symptom — most fixes below assume you can
see these surfaces.

### Where the sidecar logs go

The sidecar is a child process of the Rust shell; where its output lands
depends on launch mode:

| Launch mode | Sidecar stdout/stderr |
|---|---|
| `cargo tauri dev` | interleaved in the foreground terminal. The one line to keep is `SIDECAR_READY port=<port> token=<hex>` — the shell parses it, then the readiness stdout pipe closes. The token remains private to the shell and is never shown in the webview. |
| Bundled app | appended under `<app-data>/logs/` — see [Logs and data locations](prerequisites-and-install.md#logs-and-data-locations). |

After a respawn the shell reissues a token internally; a client presenting the
old one gets HTTP 401 ([WebSocket Streaming API](websocket-streaming-api.md#sidecar-lifecycle)).

### Container logs (Virtual Participant)

Get the container id from the task row in the VP dashboard, or from the
database:

```bash
sqlite3 -readonly "<db>" \
  "SELECT id, state, container_id, started_at FROM vp_tasks ORDER BY started_at DESC LIMIT 5;"
docker logs -f <container_id>
```

For a live look instead of logs, connect a VNC client to the mapped display
port or open the takeover view — see [Virtual Participant Local
Development](virtual-participant-local-dev.md#inspecting-the-bots-display).

### Database inspection

Open read-only: both app processes hold write connections, and a read-write
CLI session invites `DB_WRITE_CONFLICT`.

```bash
DB="$HOME/Library/Application Support/com.osslma.desktop/lma.db"
```

```powershell
$DB = "$env:APPDATA\oss-lma\lma.db"
```

```bash
sqlite3 -readonly "$DB" "SELECT id, title, status, duration_ms FROM meetings ORDER BY started_at DESC LIMIT 5;"
sqlite3 -readonly "$DB" "SELECT channel, count(*) FROM segments WHERE meeting_id='<id>' GROUP BY channel;"
sqlite3 -readonly "$DB" "SELECT section, length(content) FROM summaries WHERE meeting_id='<id>';"
sqlite3 -readonly "$DB" "SELECT id, state, container_id FROM vp_tasks ORDER BY started_at DESC LIMIT 5;"
```

A meeting row with zero segments means audio never reached the sidecar;
zero-length summary rows point at the LLM chains, not the transcript.

### Health checks

Liveness probe — any HTTP answer (typically 401 Unauthorized, the handshake
gate) proves something is listening on the sidecar port; connection refused
means it is down or still starting:

```bash
curl -si "http://127.0.0.1:<port>/ws" | head -1
```

Full handshake with the token from the spawn line:

```bash
uv run --with websockets python - <<'PY'
import asyncio, websockets

async def main():
    async with websockets.connect("ws://127.0.0.1:<port>/ws?token=<hex>") as ws:
        print("handshake ok")

asyncio.run(main())
PY
```

A 401 here with a freshly copied token means the sidecar respawned since you
read `SIDECAR_READY` — read the new line and retry.

### What to attach to a bug report

- App version, OS, launch mode (`cargo tauri dev` vs bundled).
- Sidecar output around the failure, including the `SIDECAR_READY` line.
- The `ERROR {Code, Context}` payload surfaced in the UI, if any.
- Output of the queries above for the affected meeting.
- Listing of the meeting directory: `ls -l <app-data>/recordings/<meeting_id>/`.
- For VP tasks: `docker logs` of the container plus the `vp_tasks` row.
- Redact transcript content if the meeting is sensitive.

## Capture

**No system audio captured (macOS)** — `CAPTURE_PERMISSION_DENIED`. Screen
Recording permission missing or attributed to the wrong bundle. Grant
**Screen & System Audio Recording** to the bundled app in System Settings →
Privacy & Security, then quit and relaunch the app. A grant to Terminal or a
nested executable does not authorize the bundle. Rebuilds with ad-hoc signing
invalidate prior grants; install the development certificate to keep them
stable.

**Microphone blocked** — `CAPTURE_PERMISSION_DENIED`. The macOS Microphone
prompt was denied. Re-enable access in System Settings → Privacy & Security,
then reselect the device in Settings → Capture
([Capture permissions](prerequisites-and-install.md#capture-permissions)).

**No microphone input after unplugging headset** — `CAPTURE_DEVICE_LOST`.
Capture rebuilds the affected source on device-change events without ending
the meeting. Reconnect the selected microphone and wait for its meter to
resume. If it stays silent, stop the meeting and reselect it in Settings;
selections cannot change mid-meeting. Repeated rebuilds mean a driver is
flapping — pick a stable device explicitly instead of leaving the OS default.

**The wrong system output is captured (macOS)** — system capture follows the
current macOS default output only. A system-output override is intentionally
rejected (`system audio capture supports the default output only`). Switch
the default output in macOS before starting the meeting; only microphone
selection can be overridden in Settings → Capture.

**Transcript pauses, then resumes with a gap** — `LINK_DISCONNECTED`. The
capture→sidecar WebSocket dropped. Recovery is automatic: up to 3 s of audio
is buffered and flushed on reconnect (anything older is dropped), a fresh
`START` reuses the same `CallId`, and timestamps stay continuous. Isolated
drops around sleep/resume are normal; frequent ones mean the sidecar keeps
dying — find the crash in [Diagnostics](#where-the-sidecar-logs-go).

For developer diagnostics, subscribe to `lma_link::LinkClient` events:
`Disconnected` marks an interrupted transport, `Connected` marks recovery,
and `BufferDropped` means the bounded reconnect buffer evicted oldest audio.
`BufferDropped` is link-layer telemetry today; it is not persisted as a
meeting field or displayed as a UI counter. Record the event time and the
link/sidecar log when filing a capture-loss bug.

## Transcription

**`STT_PROVIDER_AUTH`** — key invalid, expired, or lacks streaming
entitlements. Re-enter it in Settings; keys are stored in the OS keychain.

**`SIDECAR_UNAVAILABLE` immediately after Start** — the selected provider key
is missing or the sidecar could not start. Save a provider key in the app,
then retry. The desktop shell keeps the sidecar token and port private; do not
work around this by adding them to the UI or command line.

**`STT_STREAM_RESET` repeated** — check provider status and network; five
consecutive failures stop the stream by design. A session that survives ≥10 s
resets the counter, so a blip during meeting start is the risky window.

## Sidecar lifecycle

**Transcript, chat, and search fail together; banner says `err.sidecar_unavailable`**
— `SIDECAR_UNAVAILABLE`. Everything Python-owned dies at once because the
sidecar process exited or stopped answering. The shell respawns it and
reissues the token automatically — one respawn recovers silently. A respawn
loop points at the Python environment: read the traceback in the sidecar log
([Diagnostics](#diagnostics)) for a failed import or a broken bundled env.

**Startup stalls before the live view activates** — `PORT_BIND_FAILED`. The
sidecar could not bind a localhost port and moved to the next, giving up
after 10 attempts. Security software blocking listener sockets or an
exhausted ephemeral port range is the usual cause; allow binding on
`127.0.0.1` or reboot. Each attempt is logged with the port it tried.

## Assistant

**`AGENT_TOOL_FAILURE` on every answer** — usually a missing web-search or
embedding key; check Settings → providers.

**Empty citations in past-meeting answers** — `RAG_EMBEDDING_UNAVAILABLE`.
Ingestion deferred because embeddings were unavailable; retry from meeting
detail.

## Virtual Participant

**Container exits at join** — `VP_CONTAINER_FAILED`. Platform DOM changed;
the selector resolver refreshes automatically, but persistent failure means
the adapter needs an update. Check the task log in the dashboard.

**`VP_MANUAL_ACTION_REQUIRED` loops** — the meeting requires interactive
login (CAPTCHA / 2FA / SSO). Complete it once in the takeover view; the
persistent profile remembers subsequent joins. The escalation expires after
300 s and fails the task if nobody responds, so start the takeover promptly.
Loops across runs mean the profile volume was reset or the platform demands
verification on every join from this account.

## Storage

**`DB_WRITE_CONFLICT`** — another process holds the database beyond the
retry window; close stray sidecar processes. When inspecting manually, open
SQLite read-only ([Diagnostics](#database-inspection)).

## Common questions

**Audio transcribes, but everyone shows the same generic speaker label.**

Channels carry one label each by default — Meeting Audio and My Mic. Real
names come from engine diarization, which is opt-in per channel: enable it in
Settings → Capture → Speaker identification for sources where several people
share one microphone or one system-audio feed
([Desktop Capture App](desktop-capture-app.md#optional-identify-separate-speakers)).
Labels arrive only on finalized results, and sub-threshold fragments
(below 3 words / 0.5 s) are absorbed into the neighbouring speaker
([Transcription & Translation](transcription-and-translation.md#pipeline-specification)).

**The wake phrase never triggers the assistant.**

Detection runs on finalized segments, only on channels listed in
`assistant.wake_channels` — which defaults to `CALLER`, i.e. meeting audio.
Saying "OK Assistant" into your own mic lands on the `AGENT` channel and is
ignored until you add `AGENT` to the setting. Then check the pattern itself
(default `(OK|Okay)[.,! ]*[Aa]ssistant`) against how you actually say it, and
allow a beat: detection waits for the segment to settle
([Meeting Assistant](meeting-assistant.md#wake-phrase)).

**Summaries are empty, or whole sections are missing, after a meeting ends.**

Each summary section is one independent LLM call — an invalid or
unconfigured LLM key loses all of them while the transcript still looks
fine. A custom template whose value is `NONE` or empty disables just that
section. Fix the key, then re-run on demand from the chat shortcut buttons;
failed thinking steps in the timeline show which sections erred
([Transcript Summarization](transcript-summarization.md)).

**The Virtual Participant image build fails behind a corporate proxy.**

Pass the proxy to the image build and to the running container, and exclude
the host callback address — otherwise the container routes its connection to
`host.docker.internal` through the proxy and every join stalls:

```yaml
build:
  args:
    - HTTP_PROXY=http://proxy.corp.example:8080
    - HTTPS_PROXY=http://proxy.corp.example:8080
environment:
  - HTTP_PROXY=http://proxy.corp.example:8080
  - HTTPS_PROXY=http://proxy.corp.example:8080
  - NO_PROXY=localhost,127.0.0.1,host.docker.internal
```

With a TLS-intercepting proxy, the corporate CA must also be trusted inside
the image for Playwright/Chromium downloads. Debug outside the app first:
`docker compose -f vp-container/compose.yaml up --build`
([Virtual Participant Local Development](virtual-participant-local-dev.md)).

**Why do locally captured meetings have audio but no video?**

Video exists only for [Virtual Participant](virtual-participant.md) sessions
— it is the bot's own display, recorded inside the container. Desktop capture
records the stereo audio mix only and deliberately never screen-records your
display. Check the meeting folder under `recordings/`: local meetings have
`audio.wav`; VP meetings add `video.mp4`.

**Can I switch STT provider when I already have meeting history?**

Yes. The provider selected in Settings applies to sessions started
afterwards; existing transcripts remain exactly as transcribed, and
past-meeting search runs over stored text, not audio. Switching the
*embedding* provider or model triggers automatic re-embedding of stored
chunks on the next launch
([Updates & Migrations](updates-and-migrations.md#schema-migrations)).
