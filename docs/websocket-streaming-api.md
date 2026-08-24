---
title: "WebSocket Streaming API"
---

# WebSocket Streaming API

Full protocol specification for streaming clients that feed meetings into
oss-lma — useful for custom capture frontends, tests, and integrations.

> The canonical machine-readable contract is
> `contracts/events.schema.json` (JSON Schema, draft 2020-12) plus
> `contracts/errors.yaml` for error codes. This document is the human guide;
> the schema is the source of truth and both language sides validate against
> it in CI.

> The desktop app speaks this same protocol to the sidecar; anything that
> implements it becomes a meeting source. See
> [Transcription & Translation](transcription-and-translation.md) for what
> happens downstream.

## Wire conventions

- **Text frames** are JSON events; **binary frames** are audio.
- All JSON keys are **PascalCase** on the wire (`CallId`, not `call_id`) —
  matching every example below. Language-native structs convert at their own
  boundary.
- Wire timestamps are float seconds from stream start; SQLite stores integer
  milliseconds (conversion happens once, at the DB write boundary inside the
  sidecar).

## Endpoint

```
ws://127.0.0.1:<port>/ws?token=<handshake-token>
```

### Sidecar lifecycle

The sidecar is spawned by the shell as a child process:

1. The sidecar binds a random localhost port, retrying up to 10 attempts if
   a bind fails (`PORT_BIND_FAILED`).
2. It prints one line to stdout: `SIDECAR_READY port=<port>
   token=<hex>` — the only stdout line the shell parses.
3. The token is random per sidecar process. It stays valid for the
   process's lifetime, so reconnecting clients reuse it; it is reissued
   only when the shell respawns the sidecar (`SIDECAR_UNAVAILABLE`).
4. A client that presents no token or a stale token after a respawn gets
   HTTP 401 on the upgrade request.

Containers reach the same endpoint through `host.docker.internal`
([Virtual Participant](virtual-participant.md)) with the token passed as an
environment variable.

## Control messages (client → sidecar)

### START

Begins a meeting session:

```json
{
  "EventType": "START",
  "CallId": "<uuid>",
  "SamplingRate": 48000,
  "DiarizeSystemChannel": false,
  "DiarizeMicChannel": true
}
```

Diarization flags default to false; enable them when several people share a
channel ([Desktop Capture App](desktop-capture-app.md#optional-identify-separate-speakers)).

### SPEAKER_CHANGE

Declares who is currently speaking into **one named channel**:

```json
{ "EventType": "SPEAKER_CHANGE", "CallId": "...",
  "Channel": "AGENT", "ActiveSpeaker": "Ayu" }
```

`Channel` is required — with two mono sources there is no implicit "current"
side. Non-diarized items on that channel bin against this name until the
next change.

### PAUSE / RESUME

One mechanism, stated once: `PAUSE` makes the sidecar consume and discard
audio while keeping the session open; capture-side pause sends this frame
and does not additionally discard locally.

```json
{ "EventType": "PAUSE", "CallId": "..." }
```

### END

Finalizes the meeting and triggers summarization.

```json
{ "EventType": "END", "CallId": "..." }
```

### VP_COMMAND

Drives an in-flight [Virtual Participant](virtual-participant.md) session
(manual takeover, chat commands):

```json
{ "EventType": "VP_COMMAND", "TaskId": "<task-uuid>",
  "Command": "CLICK", "Payload": {"x": 412, "y": 380} }
```

Commands: `CLICK`, `TYPE`, `CHAT`, `RESET_SELECTORS`, `END`, `PAUSE`,
`RESUME`.

## Audio (client → sidecar)

Binary frames carry interleaved stereo signed 16-bit little-endian PCM at
the `SamplingRate` declared in `START`. Channel 0 = system/meeting audio
(`CALLER`), channel 1 = microphone (`AGENT`).

The canonical chunk is **exactly 100 ms**:

```text
bytes_per_chunk = SamplingRate × 2 channels × 2 bytes × 0.1 s
                = 19,200 B @ 48 kHz
```

Larger frames are re-chunked to this size on arrival; smaller ones are
buffered up to it, so downstream consumers always see fixed-size chunks.

### Reconnect timeline

There is no session resume. After any drop the client reconnects and sends a
fresh `START` **with the same `CallId`** — segments and summaries continue
one meeting record. The sidecar carries a cumulative time offset (max end
time seen) into the new session, so timestamps never go backwards; the
client-side link layer only buffers, it never adjusts times.

```text
t=0.0s  START {SamplingRate: 48000}      → segments at 0.00–…
t=9.2s  connection drops                 → client buffers audio
        buffer holds ≤3 s (oldest dropped + counted)
t=9.6s  reconnect OK                     → START again, same CallId
        buffered 0.4 s flushed first, live audio resumes
```

Backoff schedule: first retry after 0.5 s, doubling to a 10 s ceiling;
the counter resets once a connection survives long enough to send `START`.
Concurrent reconnect attempts collapse into one (single-flight guard).

## Events (sidecar → client)

| Event | Payload |
|---|---|
| `ADD_TRANSCRIPT_SEGMENT` | `{SegmentId, Channel, Speaker, StartTime, EndTime, Transcript, IsPartial}` |
| `ADD_SUMMARY` | `{Section, SummaryText}` |
| `ADD_AGENT_ASSIST` | `{SegmentId, TriggerSegmentId, Transcript, IsPartial}` |
| `AGENT_TOKEN` | `{Seq, Delta}` |
| `THINKING_STEP` | `{Seq, StepType, Content?, ToolName?, ToolInput?, ToolResult?, Success?}` |
| `VP_STATUS` | `{TaskId, State, Detail?}` |
| `VP_SCREENSHOT` | `{TaskId, ImageBase64}` |
| `ERROR` | `{Code, Context}` |

All carry the envelope `{EventType, CallId}`. Full field constraints live in
`contracts/events.schema.json`; every field above is PascalCase on the wire.

Segment IDs stay stable across partial→final transitions — overwrite by
`SegmentId` to track updates. After any disconnect, start a fresh session
with `START` (same `CallId`); there is no resume.
