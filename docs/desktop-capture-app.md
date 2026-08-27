---
title: "Desktop Capture App"
---

# Desktop Capture App

The **desktop capture app** is the primary way to use oss-lma: it captures
your **microphone**, your computer's **system (meeting) audio**, and — during
[Virtual Participant](virtual-participant.md) sessions — nothing else is
needed. Because it captures the operating system's audio rather than a
browser tab, it transcribes meetings joined from **native desktop apps**
(Zoom, Teams, WebEx, Slack huddles, phone bridges, …). It adds **no bot**
or extra attendee to the meeting.

> **Status:** macOS 13+ in this phase. Capture lives in Rust
> (`crates/lma-capture`); Windows and Linux capture are planned separately.

## How it works

- Your **microphone** is transcribed as the **My Mic** channel (the AGENT
  channel downstream).
- Your computer's **system audio** — everyone else — is the **Meeting
  Audio** channel (the CALLER channel downstream).
- Audio flows to the local sidecar over one WebSocket using the
  [WebSocket Streaming API](websocket-streaming-api.md), then through the
  same pipeline as every other source ([Transcription &
  Translation](transcription-and-translation.md)).
- **macOS** captures system audio via **ScreenCaptureKit** (loopback) and
  the mic via an AVAudioEngine input tap. The two mono sources are interleaved
  into stereo 16-bit PCM at 48 kHz, cut into 100 ms chunks.

The capture stack survives device changes: unplug a headset mid-meeting and
the mic stream rebuilds itself against the new default device without
dropping the session.

## Capture internals (`crates/lma-capture`)

Both sources run as independent mono Float32 streams at the target rate
(48 kHz default) and meet in a mixer that:

- buffers the two streams independently, draining `min(frames_a, frames_b)`
  every 100 ms tick — so muting or stalling one side never shifts the other.
  A tick with less than a full 100 ms of *both* channels emits nothing; the
  remainder carries into the next tick (the link layer only ever receives
  exact chunks);
- converts to interleaved stereo int16: `sample × 32768`, truncated toward
  zero and saturated to `[-32768, +32767]`;
- on **mute**, zero-fills a channel but keeps consuming its frames
  (alignment preserved);
- on **pause**, sends the wire-level `PAUSE` frame and discards captured audio
  before recording or streaming while the session stays open.

> The reconnect buffer is sized from the configured rate:
> `rate × 2ch × 2B × 3`. At the default 48 kHz that is exactly 3 s.

## Link internals (`crates/lma-link`)

| Parameter | Value |
|---|---|
| Reconnect buffer | ≤3 s stereo (`rate × 2ch × 2B × 3`); oldest dropped, drop-count exposed as telemetry |
| Backoff | 0.5 s doubling → 10 s ceiling |
| Connect guard | single-flight |
| Resume | none — fresh `START` every reconnect; server carries time offset |

Device enumeration and level meters surface to the webview through Tauri
commands; audio never passes through the webview.

## Optional: identify separate speakers

By default each channel gets a single label — everything from system audio
is **Meeting Audio**, everything from your mic is **My Mic**. That is enough
for a one-to-one call, but not when several people share a channel.

**Settings → Speaker identification** turns on engine diarization,
independently per channel:

| Setting | Turn it on when |
|---|---|
| Identify separate speakers in meeting audio | Several remote participants share the Meeting Audio channel |
| Identify separate speakers in my mic | Multiple people speak into one microphone |

When enabled, the transcription engine labels individual speakers per
channel and smoothing merges short fragments so labels stay stable — see
[Transcription & Translation](transcription-and-translation.md).

## Controls

| Control | Behavior |
|---|---|
| Record / Stop | opens or closes the meeting |
| Mute mic | zeroes the mic channel, keeps both channels aligned |
| Mute meeting audio | same, for system audio |
| Pause | consumes and discards audio while keeping the connection open |

Muting never shifts timestamps — alignment between channels is preserved no
matter what you mute, when.
