---
title: "oss-lma Documentation"
---

# oss-lma Documentation

**oss-lma v0.1.0** — Real-time meeting transcription, AI-powered meeting assistance, and virtual meeting participation, running entirely on your machine.

> For the changelog, see [CHANGELOG.md](../CHANGELOG.md). For contributing, see [CONTRIBUTING.md](../CONTRIBUTING.md).

---

## Table of Contents

### Getting Started

- [Prerequisites & Installation](prerequisites-and-install.md) — OS requirements, capture permissions, provider API keys, building from source
- [Quick Start Guide](quick-start-guide.md) — Your first meeting in 5 minutes using desktop capture

### Core Features

- [Transcription & Translation](transcription-and-translation.md) — Pluggable STT providers, speaker attribution, multi-language support, live translation, audio recording
- [Meeting Assistant](meeting-assistant.md) — LangGraph agent chat, built-in tools, wake phrase, MCP tools, model selection, custom prompts
- [Transcript Summarization](transcript-summarization.md) — Automatic and on-demand summaries, custom prompt templates, structured action items
- [Meetings Query Tool](meetings-query-tool.md) — Semantic search across past meetings via the local vector store

### Meeting Sources

- [Meeting Sources Overview](meeting-sources.md) — Side-by-side comparison of capture options and guidance on when to use each
- [Desktop Capture App](desktop-capture-app.md) — Menu-bar / system-tray app capturing system + mic audio for meetings joined from native apps (no bot)
- [Virtual Participant](virtual-participant.md) — Headless browser bot joining Zoom and Google Meet meetings inside a local container

### Voice Assistant

- [Voice Assistant Overview](voice-assistant.md) — In-meeting spoken replies from the Virtual Participant, activation modes, TTS providers

### MCP Server Integration

- [MCP Servers Overview](mcp-servers.md) — Model Context Protocol servers, authentication methods, keychain storage, built-in assistant tools

### Desktop App UI

- [Desktop App Guide](desktop-app-ui.md) — Live view, meeting history, chat panel, settings, virtual participant dashboard

### Security & Privacy

- [Security & Privacy](security-and-privacy.md) — What leaves your machine, secret storage, network surface, data locality, consent

### Integration & API

- [WebSocket Streaming API](websocket-streaming-api.md) — Full protocol specification for building custom streaming clients against the sidecar

### Maintenance & Development

- [Updates & Migrations](updates-and-migrations.md) — Updating the app, schema migrations, downgrade policy
- [Developer Guide](developer-guide.md) — Architecture, process model, data flows, data model, error catalog, testing strategy
- [Virtual Participant Local Development](virtual-participant-local-dev.md) — Debug the bot container locally: VNC, fake meeting harness, selector cache
- [Troubleshooting](troubleshooting.md) — Symptoms indexed by error catalog code, common issues
