---
title: "Quick Start Guide"
---

# Quick Start Guide

Your first local meeting in five minutes using [desktop capture](desktop-capture-app.md).

## 1. Launch

Open **oss-lma** from your Applications folder (or run `cargo tauri dev`
from a source checkout). The main window opens on the capture and live
transcript view. The app starts its local Python sidecar automatically; its
localhost port and token stay inside the desktop process.

## 2. Configure providers

In **Transcription provider**, select Deepgram, enter the model and language,
and save your API key. The key is written directly to the OS keychain; the UI
only shows whether a key is present. AssemblyAI and Azure use the same fields
once their adapter credentials are configured. LLM and assistant settings are
not part of this capture increment. See [Prerequisites & Installation](prerequisites-and-install.md)
for provider setup.

## 3. Pick devices

Use **Open microphone settings** and **Open screen recording settings** when
the access status is unknown or denied. The macOS system-audio source follows
the default output; the selected microphone is shown by the capture backend.
Levels appear as soon as audio is available.

## 4. Start a meeting

Join your meeting from any app — Zoom, Teams, Meet in a browser, or a phone
bridge — then press **Start meeting**:

- Your microphone is transcribed as the **My Mic** channel.
- Everything else in the room is transcribed as the **Meeting Audio**
  channel.
- Live transcript streams into the window with speaker labels and
  timestamps.

## 5. Pause or stop

Use **Pause** to keep the socket and synchronized audio cadence alive while
discarding audio server-side. Press **Stop** to send `END`, finalize the
transcript, close the WAV writer, and return to Idle. The recording is stored
under the app data directory documented in [Prerequisites & Installation](prerequisites-and-install.md#logs-and-data-locations).

Assistant chat, summaries, action items, history search, and virtual
participant flows are documented separately and will be enabled by their
respective product increments.
