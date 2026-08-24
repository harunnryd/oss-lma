---
title: "Desktop App Guide"
---

# Desktop App Guide

A tour of the desktop app surfaces. All data is local — see
[Prerequisites & Installation](prerequisites-and-install.md) for storage
locations.

## Live view

- Recording consent disclaimer before first capture
- Dual-channel live transcript, color-coded **My Mic** / **Meeting Audio**,
  speaker labels, timestamps, partial segments rendered distinctly
- Elapsed timer, per-channel VU meters, independent mutes, pause
- Recent meeting names for one-click restart

## Meetings

- **History list**: search, date-range presets, status column, pagination
- **Detail view**: full transcript with per-segment sentiment, summary
  sections, editable action-item checklist, audio player, and video player
  for Virtual Participant sessions

## Assistant

- Chat panel with token streaming and a thinking-step timeline — reasoning,
  tool use, tool results
- Shortcut buttons for canned prompts (customizable — see
  [Meeting Assistant](meeting-assistant.md))
- Dedicated search view for past-meeting questions
  ([Meetings Query Tool](meetings-query-tool.md))

## Settings

- **Providers**: STT engine, LLM, embeddings, web search, TTS — keys stored
  in the OS keychain
- **Capture → Speaker identification**: engine diarization per channel
  (see [Desktop Capture App](desktop-capture-app.md#optional-identify-separate-speakers))
- **Prompts**: template editor over defaults, domain variant selection
- **MCP servers**: registration and auth ([MCP Servers](mcp-servers.md))

## Virtual Participant dashboard

- Schedule creation: platform, meeting URL, recurrence
- Live task list with container state and logs
- Takeover view for `VP_MANUAL_ACTION_REQUIRED`: screenshot stream,
  click/type passthrough, in-meeting commands
  ([Virtual Participant](virtual-participant.md))
