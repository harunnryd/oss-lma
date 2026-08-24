---
title: "Meeting Sources Overview"
---

# Meeting Sources Overview

oss-lma ingests meetings from two sources. Both feed the identical pipeline —
[transcription](transcription-and-translation.md), [assistant](meeting-assistant.md),
[summaries](transcript-summarization.md), search.

| | [Desktop Capture App](desktop-capture-app.md) | [Virtual Participant](virtual-participant.md) |
|---|---|---|
| **How it captures** | Your machine's system audio + microphone | A headless browser joins the meeting as a guest |
| **Adds a bot to the call?** | No | Yes, visible attendee |
| **Works with** | Any app that plays through your speakers: native Zoom/Teams, phone bridges, in-person rooms | Platforms with web clients: Zoom, Google Meet (Teams, WebEx planned) |
| **Speaker names** | Channel labels + optional diarization ("My Mic" / "Meeting Audio") | Platform display names where the platform exposes them |
| **Requires Docker** | No | Yes |
| **Your machine attends?** | Yes — you must be in the meeting | No — works while you are away |
| **Consent surface** | Recording disclaimer at start; nothing enters the meeting | Bot announces itself; a recording notice is posted in meeting chat |

## Choosing

- You are attending the meeting yourself → **Desktop Capture App**. Zero
  setup, no bot, works everywhere.
- You cannot attend, or want a searchable recording of a meeting you skip →
  **Virtual Participant**. Needs Docker and platform credentials for
  signed-in joins.

Both sources can run simultaneously; each becomes its own meeting record.
