---
title: "Voice Assistant Overview"
---

# Voice Assistant Overview

The voice assistant lets the [Virtual Participant](virtual-participant.md)
**speak**: it hears the meeting and can answer aloud when addressed —
answering questions you relay, covering talking points, or responding to its
wake phrase.

## How it works

- The assistant's hearing path is the `combined_monitor` tap inside the
  container — see the named audio graph in
  [Virtual Participant](virtual-participant.md). The TTS branch never enters
  it, so spoken replies are not re-heard.
- Activation modes:

  | Mode | Behavior |
  |---|---|
  | Off | silent participant (default) |
  | Wake phrase | speaks only after its phrase — same pattern and settings as the [Meeting Assistant](meeting-assistant.md) wake phrase, detected on finalized transcript segments |
  | Relayed | answers questions you type from the takeover view |

- When activated, the transcript is read by the same agent behind the
  [Meeting Assistant](meeting-assistant.md); the reply streams as
  `AGENT_TOKEN`, lands in the transcript as an `AGENT_ASSISTANT` segment,
  and is synthesized to speech through a pluggable TTS provider into
  `tts_sink`.
- No barge-in in v1: once speaking, the assistant finishes its turn;
  follow-ups queue.

Because the whole audio graph is virtual and lives inside the container,
no audio hardware or drivers are needed on your machine.

## TTS providers

Speech synthesis is pluggable — configure provider and voice in Settings.
Provider keys live in the OS keychain like everything else. Replies are also
written to the transcript as the assistant channel, so spoken answers remain
searchable alongside everything else.

## Limits

The assistant speaks only during Virtual Participant sessions — desktop
capture has no outbound path into the room, by design: your own voice is
your voice.
