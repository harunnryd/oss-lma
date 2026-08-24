---
title: "Virtual Participant"
---

# Virtual Participant

The **Virtual Participant (VP)** is a headless browser bot that joins online
meetings on your behalf — while you are late, double-booked, or simply not
attending. It appears as a regular guest, records and transcribes the
session, and hands you the same meeting record as every other source.

> **Status:** Zoom and Google Meet today; Teams and WebEx follow the same
> adapter interface. Requires Docker.

## How it works

1. Schedule a join in the VP dashboard: platform, meeting URL, recurrence,
   options — or trigger one immediately.
2. The host app starts a local container carrying Chromium (Playwright),
   a virtual display, and a **virtual audio graph**; the platform adapter
   runs **inside** the container (the image vendors `lma_vp`'s adapter
   runtime), so bot logic never depends on host Python.
3. The container dials back to the host sidecar over
   `host.docker.internal` using the same wire protocol as desktop capture;
   the handshake token arrives in the `SIDECAR_TOKEN` environment variable.
4. The bot joins via the platform adapter, posts an AI-assistant intro and
   start-of-recording notice into meeting chat (platform consent dialogs are
   auto-acknowledged), and begins capturing.
5. At meeting end the container finalizes video, and summaries run like any
   other meeting.

Because the audio graph is entirely virtual inside the container, no OS
audio devices or drivers are needed on your machine.

## Audio graph

Every node is named and wired once at container start:

```text
[Chromium tab] ──► meeting_sink ──► meeting_monitor ──► FFmpeg (stereo PCM → sidecar)
                          │
                          └──► combined_sink ──► combined_monitor ──► (reserved: voice assistant hearing)
[TTS output]   ──► tts_sink ──► tts_loopback_source ──► [Chromium microphone]
```

- The sidecar-facing stream taps **`meeting_monitor` only** — the TTS branch
  never enters it, so spoken replies cannot be re-heard as room audio (no
  echo/dedup problem).
- `tts_loopback_source` is bound as Chromium's microphone input, giving the
  assistant its speaking path.
- FFmpeg resamples to the wire contract's stereo 48 kHz; channel 0 carries
  meeting audio (`CALLER`). With no local microphone in the container,
  channel 1 is silence unless the voice assistant is active.

## Video recording

FFmpeg records the bot's display at 5 fps (1080p) via x11grab into 60-second
mpegts segments under the meeting's recording directory; on finalization the
segments are joined with the concat demuxer into one seekable MP4 — no
re-encode. Recorder hiccups retry within the current segment; gaps are never
dropped because everything after a gap would be undecodable.

## Scheduling

Schedules live locally (`vp_schedules`): platform, URL, **RRULE recurrence**
(RFC 5545 subset: FREQ/WEEKLY-BYDAY, INTERVAL, UNTIL/COUNT), options. The
scheduler wakes tasks at join time in the host's timezone; DST transitions
resolve to the wall-clock time nearest the recurrence rule. Spoken replies
from the bot are covered separately in [Voice Assistant](voice-assistant.md).

## Platform adapters

Each platform implements one interface (`python/lma_vp/adapters`):

```text
join(ctx) -> JoinResult          # navigate, wait for entry, post notice
leave()                          # graceful exit
send_chat(text)                  # in-meeting chat commands / notices
set_muted(flag)                  # mic state as seen by the room
screenshot() -> bytes            # PNG for takeover view + DOM resolver
```

### Per-platform surfaces

| | Zoom | Google Meet |
|---|---|---|
| **Join entry point** | `https://zoom.us/wc/<meeting-id>/join` — the adapter extracts the numeric ID and passcode from whatever invite URL you paste | `https://meet.google.com/<xxx-yyyy-zzz>` — joined directly; there is no passcode |
| **Auth modes** | Guest by default; optional stored Zoom account signs in before navigating (OS-keychain credentials) — required for meetings that disallow guests and materially harder for bot detection to block | Guest with a display name only; some workspace meetings block anonymous joins and need a stored Google account instead |
| **Fragile surfaces** | Prejoin inputs (`#input-for-pwd`, `#input-for-name`, `.preview-join-button`), SVG icon classes on the audio/video toggles, footer-toolbar class names, chat-panel aria-labels — all rename across Zoom releases | Green-room "Join now" / "Ask to join" buttons, mic/camera consent prompts, and toolbar labels that move between the bottom bar and the overflow menu |
| **Adapter settings** | Display name (scribe identity), mute-on-join, camera off unless the [Voice Assistant](voice-assistant.md) is active, stored-account sign-in toggle | Display name, mute-on-join, camera off by default, stored-account sign-in toggle |

Platform UI changes are handled by an AI DOM resolver that re-derives
selectors from screenshots and caches them — a broken selector costs a
retry, not a failed join (see [VP Join
Reliability](vp-join-reliability.md#ai-dom-resolver)). Adapter tests run
against a locally served fake meeting page — see [Virtual Participant Local
Development](virtual-participant-local-dev.md).

Persistent Chromium profiles keep platform sign-ins across runs; credentials
live in the OS keychain, never in the database.

## Container contract

| Piece | Value |
|---|---|
| Image components | Playwright Chromium (persistent profile volume), Xvfb, x11vnc, PulseAudio graph, FFmpeg |
| `MEETING_URL` | join target |
| `PLATFORM` | adapter selection (`zoom` \| `meet`) |
| `SIDECAR_WS_URL` | host sidecar endpoint for audio streaming + events |
| Profile volume | named volume; survives container restarts |
| Display ports | VNC mapped to localhost for inspection |

Task lifecycle tracked in `vp_tasks`:

```text
PENDING → LAUNCHING → JOINING → IN_MEETING → FINALIZING → DONE
                                   ↓ (CAPTCHA / 2FA / consent)
                          AWAITING_ACTION  ──manual takeover──↗
any stage ──container failure──→ FAILED (restart per schedule policy)
```

Audio leaves the container over the same framing as desktop capture — see
[WebSocket Streaming API](websocket-streaming-api.md); video segments land
under the meeting's recording directory.

## Join lifecycle

One scheduled occurrence end to end:

```mermaid
sequenceDiagram
    autonumber
    participant SCH as Scheduler (host app)
    participant BM as BotManager (sidecar)
    participant CT as Container
    participant AD as Adapter (Chromium)
    participant PF as Meeting platform
    participant ST as Transcription pipeline

    SCH->>BM: wake task at RRULE fire time
    BM->>CT: start container (MEETING_URL, PLATFORM, SIDECAR_WS_URL, SIDECAR_TOKEN)
    CT->>BM: WebSocket connect + START (CallId)
    CT->>AD: launch adapter with persistent profile
    AD->>PF: optional sign-in with stored account
    alt CAPTCHA / 2FA / SSO / consent wall
        AD-->>BM: escalate → AWAITING_ACTION
        BM-->>SCH: VP_STATUS + VP_SCREENSHOT (takeover view)
        SCH->>AD: VP_COMMAND CLICK / TYPE — human clears the wall
        Note over AD,PF: 300 s window; expiry → FAILED
    end
    AD->>PF: open meeting URL, settle prejoin, fill and submit
    Note over AD,PF: a waiting room holds here until the host admits
    PF-->>AD: admitted — IN_MEETING
    AD->>PF: post AI intro + recording notice in chat
    CT->>ST: meeting_monitor PCM (100 ms stereo frames)
    Note over CT,ST: live transcript, summaries, 5 fps video segments
    alt asked to leave / last attendee left / host ended
        AD->>PF: graceful leave
    end
    CT->>ST: END — meeting record finalized
    CT->>CT: concat video segments into one MP4
    BM-->>SCH: DONE — summaries run like any other meeting
```

Stage details that matter operationally:

- **JOINING is long and chatty by design** — the adapter pushes substep
  labels (signing in → join screen → audio/video setup → waiting to be
  admitted) every ~20 s so the dashboard never looks frozen.
- The escalation branch fires from two places: the dialog watchdog
  classifying an un-clearable modal, and the sign-in driver hitting a
  human-only step. Both stream screenshots to the takeover view; both
  self-clear when the wall disappears.
- Admission polling outlives an active escalation (+5 min grace), so a
  human clearing a CAPTCHA never races the join timeout.
- Chat posting after admission is best-effort: a chat hiccup must never
  fail an already-joined meeting.
- Every terminal path funnels through `vp_tasks` states — `never-joined`
  ends in FAILED (see [VP Join Reliability](vp-join-reliability.md#retry-policy));
  anything after admission finalizes as DONE even if the bot was removed
  mid-meeting.

## Manual takeover

CAPTCHA, 2FA, SSO walls, and consent prompts raise
`VP_MANUAL_ACTION_REQUIRED`. The dashboard opens a takeover view: live
screenshots of the bot's display with click/type passthrough plus chat
commands (`end`, `pause`, `start`). Complete the wall once; the persistent
profile remembers subsequent joins.
