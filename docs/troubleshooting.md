---
title: "Troubleshooting"
---

# Troubleshooting

Symptoms are indexed by error catalog code (see
[Developer Guide](developer-guide.md#error-catalog)).

## Capture

**No system audio captured (macOS)** — Screen Recording permission missing
or attributed to the wrong bundle. Grant permission to the bundled app and
relaunch through LaunchServices. Rebuilds with ad-hoc signing invalidate
prior grants; install the development certificate to keep them stable.

**No microphone input after unplugging headset** — capture rebuilds on
device-change events; if a device stays silent, reselect it in Settings.

## Transcription

**`STT_PROVIDER_AUTH`** — key invalid, expired, or lacks streaming
entitlements. Re-enter it in Settings; keys are stored in the OS keychain.

**`STT_STREAM_RESET` repeated** — check provider status and network; five
consecutive failures stop the stream by design.

## Assistant

**`AGENT_TOOL_FAILURE` on every answer** — usually a missing web-search or
embedding key; check Settings → providers.

**Empty citations in past-meeting answers** — ingestion deferred because
embeddings were unavailable; retry from meeting detail.

## Virtual Participant

**Container exits at join** — platform DOM changed; the selector resolver
refreshes automatically, but persistent failure means the adapter needs an
update. Check the task log in the dashboard.

**`VP_MANUAL_ACTION_REQUIRED` loops** — the meeting requires interactive
login (CAPTCHA / 2FA / SSO). Complete it once in the takeover view; the
persistent profile remembers subsequent joins.

## Storage

**`DB_WRITE_CONFLICT`** — another process holds the database beyond the
retry window; close stray sidecar processes.
