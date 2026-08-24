---
title: "Virtual Participant Local Development"
---

# Virtual Participant Local Development

Developing and debugging the [Virtual Participant](virtual-participant.md)
without joining real meetings.

## Running from a checkout

```bash
docker compose -f vp-container/compose.yaml up --build
```

The compose project starts the bot container with the same image recipe the
host app uses. Point it at any meeting URL through the environment file; the
host app is not required — the container streams audio to whatever sidecar
endpoint `SIDECAR_WS_URL` names.

## Inspecting the bot's display

The container runs Xvfb plus x11vnc:

| Need | How |
|---|---|
| Watch the join live | connect a VNC client to the mapped display port |
| One-off look | open the takeover view in the dashboard (screenshots) |
| Post-mortem | task logs in the dashboard / container logs |

## Fake meeting harness

Platform adapters are tested against a **locally served fake meeting page**
that mimics the platform surfaces adapters touch: join button, mute toggle,
chat panel, participant list. It lives beside the adapter tests so every
join-flow change runs against it in CI — no real accounts, no anti-bot
noise, deterministic DOM.

Adapter quirks that only real platforms expose (SSO walls, consent prompts)
are covered by manual passes recorded against the persistent profile.

## Selector cache

The AI-resolved DOM selector cache persists inside the container volume.
When a platform update changes surfaces mid-development:

1. Delete the cache entry for that platform (dashboard → task → reset
   selectors).
2. Re-run the join; the resolver re-derives selectors from screenshots.
3. Commit refreshed selectors if they changed structurally.
