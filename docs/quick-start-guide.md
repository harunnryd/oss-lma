---
title: "Quick Start Guide"
---

# Quick Start Guide

Your first meeting in five minutes using [desktop capture](desktop-capture-app.md).

## 1. Launch

Open **oss-lma** from your Applications folder (or run `cargo tauri dev`
from a source checkout). The menu-bar / system-tray icon appears; the main
window opens on the live view.

## 2. Configure providers

Settings → **Providers**: pick your STT provider and LLM provider, paste
API keys. Keys go straight into the OS keychain. See
[Prerequisites & Installation](prerequisites-and-install.md) for which keys
are required.

## 3. Pick devices

Settings → **Capture**: choose your microphone and confirm the system-audio
source. Levels show on the VU meters as soon as any audio plays.

## 4. Start a meeting

Join your meeting from any app — Zoom, Teams, Meet in a browser, a phone
bridge, anything that plays through your speakers — then press **Record**:

- Your microphone is transcribed as the **My Mic** channel.
- Everything else in the room is transcribed as the **Meeting Audio**
  channel.
- Live transcript streams into the window with speaker labels and
  timestamps.

## 5. Ask questions while it runs

Open the chat panel and ask about what was said — the assistant reads the
live transcript through its tools ([Meeting Assistant](meeting-assistant.md)).
Shortcut buttons give you one-click summaries and action items.

## 6. End and review

Press **Stop**. The meeting finalizes automatically: summary sections are
generated, action items are extracted, and everything becomes searchable
([Meetings Query Tool](meetings-query-tool.md)). Find it under **Meetings**
with its recording for playback.
