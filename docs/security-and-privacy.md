---
title: "Security & Privacy"
---

# Security & Privacy

oss-lma is a **single-user, local-first application**: everything sensitive
stays on your machine, and the only outbound traffic is to the providers you
configure yourself.

## What leaves your machine

| Data | Destination | When |
|---|---|---|
| Audio | your chosen STT provider | while transcribing |
| Transcript excerpts / questions | your chosen LLM provider | chat, summaries, extraction |
| Text to synthesize | your chosen TTS provider | voice assistant replies |
| Search queries | your configured search provider | `web_search` tool |
| Tool calls | MCP servers **you** registered | when the agent uses them |

Nothing else. No accounts, no telemetry, no analytics, no backend. Recordings,
transcripts, summaries, and settings never leave disk unless a tool above
sends their content.

## Secret storage

All credentials — STT/LLM/TTS/search keys, platform sign-in tokens, OAuth
secrets, MCP auth — live in the **OS keychain** (Keychain Services /
Windows Credential Manager). Nothing secret is written to the database or
config files; the database holds only references.

## Network surface

- The sidecar binds to `127.0.0.1` on a random port and requires a one-time
  handshake token passed to authorized clients at spawn
  ([WebSocket Streaming API](websocket-streaming-api.md)).
- No inbound listener exists beyond that local socket. Remote access is not
  supported — by design.
- Provider calls use TLS to the official endpoints of the configured
  provider.

## Data locality

Everything persists under the per-user application-data directory: one
SQLite database plus per-meeting recording files
([Prerequisites & Installation](prerequisites-and-install.md)). Deleting that
directory deletes everything — there is no copy anywhere else.

File permissions follow the OS default for user application data; the
database is excluded from OS-synced locations (Desktop/Documents) so meeting
recordings are never uploaded by iCloud/OneDrive-style sync.

## Consent

- **Local capture** shows a recording disclaimer before the first capture in
  each session.
- **Virtual Participant** announces itself as a recording bot in the
  meeting chat on every join ([Meeting Sources](meeting-sources.md)).

## Supply chain

Event schemas and the error catalog in `contracts/` are validated by both
language sides in CI; provider payloads cross adapters only — vendor SDKs
never leak outside `python/lma_stt`
([Developer Guide](developer-guide.md#testing)).
