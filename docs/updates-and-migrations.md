---
title: "Updates & Migrations"
---

# Updates & Migrations

How oss-lma updates are applied and how local data survives them.

## Versioning

The version in the root `VERSION` file follows semantic versioning and is
surfaced in [docs/INDEX.md](INDEX.md) and the app's about panel.

## Updating the app

Rebuild from source (or install a newer release) over the existing one.
Your data directory — database, recordings, keychain entries, settings — is
never touched by an update. First launch of a newer version applies any
pending schema migrations before the sidecar accepts connections.

## Schema migrations

- The SQLite schema evolves through **numbered forward-only migrations**
  applied by the sidecar at startup, recorded in the database itself.
- Migrations never rewrite recordings; they only touch the database.
- Settings keys change additively: new keys appear with defaults, removed
  keys are ignored, user prompt templates survive verbatim.
- Vector data re-embeds automatically if the embedding model changed
  between versions.

## Downgrades

Downgrading across a migration is unsupported — the migrated schema may be
ahead of what the older build reads. Before testing a downgrade or a risky
change, back up the data directory (it is a single folder).

## Verifying after update

1. Open the app: version shown in the about panel matches expectations.
2. Meetings list renders history intact.
3. Start a 10-second capture and confirm live transcript flows.
4. Ask the assistant one question about a past meeting.
