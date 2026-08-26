# macOS Desktop Capture Design

## Status

Approved design for the macOS-first desktop capture phase. Windows capture is
explicitly out of scope for this phase and will use a later design.

## Goal

Deliver a native Rust capture path that records all system audio and the
microphone, streams the aligned channels to the local sidecar, and writes the
meeting recording described by the existing desktop-capture and WebSocket
documentation.

## Requirements

- macOS 13+ is the only target platform for this phase.
- System audio is captured for all audio routed through the default output,
  using ScreenCaptureKit. Microphone audio uses an AVAudioEngine input tap.
- A meeting cannot start unless microphone and Screen Recording permissions are
  available and both capture streams become active.
- Each source is normalized to mono Float32 at 48 kHz before mixing.
- The mixer emits interleaved stereo signed 16-bit little-endian PCM: channel 0
  is system/meeting audio (`CALLER`) and channel 1 is microphone (`AGENT`).
- Each binary WebSocket frame is exactly 100 ms (19,200 bytes at 48 kHz).
- A short source or stalled source emits no partial tick; buffered remainder is
  carried into the next tick. Mute consumes frames and zero-fills its channel.
- Pause sends the wire-level `PAUSE` event and keeps the audio cadence alive.
- The link reconnects with a maximum three-second stereo buffer, 0.5–10 second
  exponential backoff, and a single-flight connect guard. Reconnect sends a
  fresh `START` with the same `CallId`; the client never adjusts timestamps.
- Device changes rebuild the affected stream without ending the meeting.
- The recorder writes one file at
  `<app-data>/recordings/<meeting_id>/audio.wav`.
- Device enumeration, permission state, level meters, device overrides, and
  capture lifecycle are exposed through Tauri commands/events. Audio never
  passes through the webview.

## Architecture

`crates/lma-capture` owns platform capture, normalization, mixing, and WAV
recording. It exposes platform-neutral state and audio events to the Tauri
shell. `crates/lma-link` owns the sidecar WebSocket, wire framing, reconnect
buffer, and lifecycle messages. The shell coordinates permission checks and
meeting start/stop, while the sidecar remains the authority for transcript
timestamps and persistence.

The capture layer has four boundaries:

1. **Permission/device boundary** — queries Screen Recording and microphone
   authorization, enumerates defaults and overrides, and emits device-change
   notifications.
2. **Source boundary** — independent system and microphone streams produce
   mono Float32 frames at the target rate. A source restart is local to that
   source.
3. **Mixer/recorder boundary** — the mixer drains equal frame counts on each
   100 ms tick, converts/clamps samples, sends the interleaved bytes to the
   link, and writes the same PCM to `audio.wav`.
4. **Link boundary** — control JSON uses the documented PascalCase schema;
   binary frames are fixed-size PCM. Reconnect buffering is bounded and emits
   drop telemetry when its oldest audio is discarded.

## Lifecycle and data flow

1. The shell requests permission status and device inventory before enabling
   Record. Missing permission returns a structured status and opens the
   platform System Settings URL on request.
2. On Record, the shell selects defaults or saved overrides, starts both source
   streams, and waits for both active states. If either stream cannot activate,
   no meeting is created and the UI receives a recoverable error.
3. After both streams are active, the shell creates one `CallId`, opens the WAV
   writer, sends `START` with `SamplingRate: 48000`, and begins 100 ms binary
   frames.
4. On a device change, the affected source is stopped and rebuilt against the
   new default/override. The other source and meeting clock remain active.
5. On link loss, capture and recording continue. `lma-link` stores at most three
   seconds of stereo PCM, drops oldest frames if necessary, reconnects with the
   documented backoff, sends `START` with the same `CallId`, and flushes buffered
   frames before live frames.
6. On Pause, the shell sends `PAUSE`; the mixer continues consuming and emits
   aligned zero/discard cadence as required by the wire contract. On Stop, it
   sends `END`, drains/flushes the WAV writer, closes both sources, and reports
   the final recording path.

## Errors and recovery

All user-visible failures use the existing error catalog and recovery action;
code dispatch never depends on message text. Permission denial is a preflight
failure. A source interruption is retryable and triggers a source rebuild. A
sidecar disconnect uses the link reconnect path. A stale token or respawned
sidecar causes the shell to obtain the new `SIDECAR_READY` token before retrying.
If either source cannot be rebuilt, the meeting transitions to a failed state
and the partial WAV is closed safely.

## Testing strategy

Pure Rust tests cover mixer drain math, exact 100 ms chunking, Float32-to-int16
clamping, mute zero-fill, pause cadence, and reconnect-buffer size/drop rules.
Platform-adapter tests use fake source callbacks to cover permission states,
default-device selection, device replacement, source restart, and the
requirement that one inactive channel prevents meeting start. Link tests cover
fresh `START` on reconnect, same `CallId`, backoff, single-flight behavior, and
wire-contract validation against `contracts/events.schema.json`. Recorder tests
open the generated WAV and verify stereo channel count, 48 kHz rate, 16-bit
encoding, frame count, and safe close after interruption.

The manual macOS smoke test follows `docs/prerequisites-and-install.md`:
grant both permissions to the bundled app, verify device list and meters,
record 30 seconds of meeting and microphone audio, stop, and confirm the
meeting transcript plus `audio.wav` under the documented app-data directory.

## Out of scope

- Windows WASAPI implementation.
- Linux/PipeWire support.
- Per-application audio filtering.
- Separate mono recording artifacts.
- Custom recording directories.
- Browser/webview audio capture.
