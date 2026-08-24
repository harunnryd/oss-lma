---
title: "Database Schema"
---

# Database Schema

**One SQLite file at `<app-data>/lma.db`, opened WAL-mode by both processes.
Every table has exactly one owning writer process — the Python sidecar owns
`meetings`, `segments`, `summaries`, `action_items`, `rag_chunks`, and
`vp_tasks`; the Rust shell owns `settings`, `prompt_templates`,
`mcp_servers`, and `vp_schedules`. No table is ever written by both. UI
edits to sidecar-owned rows go through Tauri commands executing
parameterized writes on the shell's own connection. Ownership rules and the
process model are defined in the
[Developer Guide](developer-guide.md#data-model); this page is the concrete
DDL those rules apply to.**

Both processes set `journal_mode=WAL`, `foreign_keys=ON`, and a
`busy_timeout`; readers never block on writers. Recordings are files under
`<app-data>/recordings/<meeting_id>/` (`audio.wav`, plus video for VP
sessions) and are referenced by path from `meetings` — they are not stored
in the database and are never rewritten by migrations
([Updates & Migrations](updates-and-migrations.md)).

## Conventions

- **Ids**: UUIDv4, lowercase, stored as `TEXT`.
- **Timestamps**: `INTEGER`, Unix epoch milliseconds, UTC. Wire frames carry
  float seconds from stream start; conversion happens once, at the DB write
  boundary ([WebSocket Streaming API](websocket-streaming-api.md)).
- **Booleans**: `INTEGER` constrained to `0` / `1`.
- **Enums**: plain `TEXT` with an inline `CHECK` constraint listing the
  allowed values. New enum members require a migration.
- **JSON blobs**: `TEXT` holding canonical JSON (`value_json`,
  `options_json`).
- **Segment ids**: stable across partial→final replacement, e.g.
  `r7-CALLER-w1-r2` ([Transcript Summarization](transcript-summarization.md));
  partial updates overwrite by primary key, finals replace their partials
  in place.

## Tables

### meetings

One row per meeting session, opened by the sidecar at `START`.

```sql
CREATE TABLE meetings (
  id          TEXT    PRIMARY KEY,
  title       TEXT    NOT NULL DEFAULT '',
  source      TEXT    NOT NULL CHECK (source IN ('LOCAL', 'VP')),
  platform    TEXT    NOT NULL DEFAULT 'local'
                      CHECK (platform IN ('local', 'zoom', 'meet')),
  status      TEXT    NOT NULL DEFAULT 'RECORDING'
                      CHECK (status IN ('RECORDING', 'FINALIZING',
                                        'COMPLETED', 'FAILED')),
  started_at  INTEGER NOT NULL,
  ended_at    INTEGER,
  duration_ms INTEGER,
  audio_path  TEXT,
  video_path  TEXT,
  CHECK (ended_at IS NULL OR ended_at >= started_at)
);
```

```sql
CREATE INDEX idx_meetings_started_at
  ON meetings (started_at DESC);
```

`title` is the one column the shell edits directly (rename in the history
list). `platform` is the VP adapter (`zoom` / `meet`) or `local` for desktop
capture. `ended_at`, `duration_ms`, and the recording paths fill in at
finalization; paths are stored relative to `<app-data>` so backing up or
moving the single data folder stays valid.

### segments

Transcript segments, live partials and finals alike.

```sql
CREATE TABLE segments (
  segment_id      TEXT    PRIMARY KEY,
  meeting_id      TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  channel         TEXT    NOT NULL
                          CHECK (channel IN ('CALLER', 'AGENT', 'AGENT_ASSISTANT')),
  speaker         TEXT,
  start_ms        INTEGER NOT NULL,
  end_ms          INTEGER NOT NULL,
  text            TEXT    NOT NULL,
  original_text   TEXT    NOT NULL,
  is_partial      INTEGER NOT NULL CHECK (is_partial IN (0, 1)),
  sentiment_score REAL,
  CHECK (end_ms >= start_ms)
);
```

```sql
CREATE INDEX idx_segments_meeting_id_end_ms
  ON segments (meeting_id, end_ms);
```

`speaker` is the diarization label (`spk_N`) or display name, null when the
channel carries none. With
[live translation](transcription-and-translation.md) enabled, `text` holds
the translated display line and `original_text` the untranslated one;
otherwise they hold the same content. `sentiment_score` is the per-segment
value rendered in the detail view, null when unavailable. `segment_id` is
globally stable, which is what lets `action_items.source_segment_id` point
at a row.

### summaries

One generated section per meeting.

```sql
CREATE TABLE summaries (
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  section    TEXT NOT NULL,
  content    TEXT NOT NULL,
  PRIMARY KEY (meeting_id, section)
);
```

`section` is a template key (`SUMMARY`, `DETAILS`, `ACTIONS`, or the
healthcare-domain `SOAP` / `BIRP`). Re-running a section overwrites by
primary key. No secondary index — the composite primary key already serves
the only access pattern, fetch-all-sections-for-meeting.

### action_items

Structured checklist extracted after section chains complete
([Transcript Summarization](transcript-summarization.md)).

```sql
CREATE TABLE action_items (
  action_item_id    TEXT    PRIMARY KEY,
  meeting_id        TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  description       TEXT    NOT NULL,
  detail            TEXT    NOT NULL DEFAULT '',
  owner             TEXT    NOT NULL DEFAULT 'TBD',
  due_date          TEXT    CHECK (due_date IS NULL
                                    OR date(due_date) = due_date),
  status            TEXT    NOT NULL DEFAULT 'open'
                             CHECK (status IN ('open', 'done')),
  source_segment_id TEXT    REFERENCES segments(segment_id) ON DELETE SET NULL
);
```

```sql
CREATE INDEX idx_action_items_meeting_id_status
  ON action_items (meeting_id, status);

CREATE INDEX idx_action_items_source_segment_id
  ON action_items (source_segment_id);
```

`due_date` is a calendar date (`YYYY-MM-DD`), not a timestamp — extraction
rarely knows more than a day — validated with SQLite's own `date()` so a
malformed LLM output is rejected at write time rather than rendered wrong.
`source_segment_id` is resolved by matching quoted spans against segments
(exact, then fuzzy; unresolved → null) and kept as a real foreign key so a
bad resolution fails loudly.

### prompt_templates

The custom layer of the two-layer template registry, seeded with read-only
copies of the shipped defaults ([Transcript
Summarization](transcript-summarization.md)). Owned by the shell; edited in
Settings → prompt editor.

```sql
CREATE TABLE prompt_templates (
  scope         TEXT    NOT NULL CHECK (scope IN ('summary', 'chat_button')),
  key           TEXT    NOT NULL,
  layer         TEXT    NOT NULL CHECK (layer IN ('default', 'custom')),
  sort_order    INTEGER NOT NULL,
  template_text TEXT    NOT NULL DEFAULT '',
  PRIMARY KEY (scope, key, layer)
);
```

```sql
CREATE INDEX idx_prompt_templates_layer_sort_order
  ON prompt_templates (layer, sort_order);
```

A `custom` row replaces the `default` row with the same `(scope, key)`;
an empty `template_text` disables that section entirely. `{transcript}` is
the only variable. The index serves the editor list, which renders one
layer ordered by `sort_order`.

### settings

Key/value store behind the [Settings
registry](developer-guide.md#settings-registry). Owned by the shell.

```sql
CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);
```

No index beyond the primary key and no constraint on `key`: keys evolve
additively across versions (new keys appear with defaults, removed keys are
ignored), so the valid-key list lives in code, not in the schema.

### mcp_servers

Registered [MCP servers](mcp-servers.md). Owned by the shell.

```sql
CREATE TABLE mcp_servers (
  server_id      TEXT PRIMARY KEY,
  transport      TEXT NOT NULL CHECK (transport IN ('streamable-http',
                                                    'pypi')),
  url_or_package TEXT NOT NULL,
  status         TEXT NOT NULL DEFAULT 'ACTIVE'
                 CHECK (status IN ('ACTIVE', 'FAILED')),
  auth_ref       TEXT
);
```

`auth_ref` is an OS-keychain reference (e.g. `keychain:mcp/deepwiki`) into
a per-server auth JSON blob — secrets never sit in the database.
`status` reflects the last health check; failed servers restart on demand
without changing the row shape. The table holds a handful of rows read
whole by the MCP loader, so it needs no secondary index.

### vp_schedules

Recurrence rules for bot joins. Owned by the shell, written by the VP
dashboard.

```sql
CREATE TABLE vp_schedules (
  id           TEXT    PRIMARY KEY,
  platform     TEXT    NOT NULL CHECK (platform IN ('zoom', 'meet')),
  meeting_url  TEXT    NOT NULL,
  rrule        TEXT    NOT NULL,
  options_json TEXT    NOT NULL DEFAULT '{}',
  enabled      INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);
```

```sql
CREATE INDEX idx_vp_schedules_enabled
  ON vp_schedules (enabled);
```

`rrule` is the RFC 5545 subset documented in [Virtual
Participant](virtual-participant.md) (FREQ/WEEKLY-BYDAY, INTERVAL,
UNTIL/COUNT); the scheduler resolves wake times in the host's timezone.
`options_json` carries adapter options verbatim.

### vp_tasks

One scheduled or immediate bot join, tracked through the container
lifecycle. Owned by the sidecar.

```sql
CREATE TABLE vp_tasks (
  id           TEXT    PRIMARY KEY,
  schedule_id  TEXT    REFERENCES vp_schedules(id) ON DELETE SET NULL,
  meeting_url  TEXT    NOT NULL,
  state        TEXT    NOT NULL DEFAULT 'PENDING'
                       CHECK (state IN ('PENDING', 'LAUNCHING', 'JOINING',
                                        'IN_MEETING', 'AWAITING_ACTION',
                                        'FINALIZING', 'DONE', 'FAILED')),
  container_id TEXT,
  started_at   INTEGER NOT NULL,
  ended_at     INTEGER,
  CHECK (ended_at IS NULL OR ended_at >= started_at)
);
```

```sql
CREATE INDEX idx_vp_tasks_state_started_at
  ON vp_tasks (state, started_at);

CREATE INDEX idx_vp_tasks_schedule_id
  ON vp_tasks (schedule_id);
```

`state` mirrors `VP_STATUS` frames in
[contracts/events.schema.json](../contracts/events.schema.json); transitions
are written by whichever supervisor stage advanced. `schedule_id` is null
for immediately-triggered tasks, and deleting a schedule nulls it out
rather than destroying task history — which is why this is `SET NULL`, the
one foreign key not anchored to `meetings`. `idx_vp_tasks_state_started_at`
serves both crash recovery (resume non-terminal states) and the dashboard's
recent-tasks view; `idx_vp_tasks_schedule_id` serves history-per-schedule.

### rag_chunks

Vector search corpus: finalized transcript segments, summaries, and
imported documents, chunked (~300 tokens, 15% overlap) and embedded at
meeting end ([Meetings Query Tool](meetings-query-tool.md)). sqlite-vec
splits storage in two: a `vec0` virtual table holding embeddings, and an
ordinary side table holding the flat metadata that citations and filters
need.

```sql
CREATE VIRTUAL TABLE rag_chunks_vec USING vec0(
  chunk_id  TEXT PRIMARY KEY,
  embedding FLOAT[384] distance_metric=cosine
);

CREATE TABLE rag_chunks (
  id         TEXT    PRIMARY KEY,
  meeting_id TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  text       TEXT    NOT NULL,
  start_ms   INTEGER,
  end_ms     INTEGER,
  channel    TEXT    NOT NULL
                     CHECK (channel IN ('CALLER', 'AGENT', 'AGENT_ASSISTANT',
                                        'DOC')),
  speaker    TEXT,
  created_at INTEGER NOT NULL,
  CHECK ((start_ms IS NULL AND end_ms IS NULL)
         OR (start_ms IS NOT NULL AND end_ms IS NOT NULL
             AND end_ms >= start_ms))
);
```

```sql
CREATE INDEX idx_rag_chunks_meeting_id_channel
  ON rag_chunks (meeting_id, channel);
```

The dimension is fixed when the virtual table is created — 384 for the
default local `multilingual-e5-small` — so switching embedding models is a
full re-embed migration, not an update
([Updates & Migrations](updates-and-migrations.md)). Transcript chunks keep
their timeline offsets in `start_ms`/`end_ms`, which is what makes
`t=` citations computable; imported document chunks have no timeline, so
those columns are null and their rows use `channel = 'DOC'` with
`speaker = null`.

The join works in one direction: `rag_chunks_vec.chunk_id` equals
`rag_chunks.id`. A search runs KNN against the virtual table, then joins to
the side table for text and offsets and to `meetings` for titles. Deletes
go the same way — because foreign keys do not propagate into virtual
tables, the sidecar deletes matching rows from both tables inside one
transaction (meeting deletion, per-meeting re-ingest retries).

## Access patterns

| Query | Surface | Tables |
|---|---|---|
| Meeting history list | History list (search, date-range presets, pagination) | `meetings` |
| Meeting detail transcript | Detail view | `segments`, `summaries`, `action_items` |
| Recent transcript window | Assistant tool `current_meeting_transcript(mode='recent')` | `segments` |
| Tick an action item | Detail view checklist | `action_items` |
| Semantic search | `meeting_history` / `document_search` tools, search view | `rag_chunks_vec`, `rag_chunks`, `meetings` |

**Meeting history list** — one preset or custom range per page, newest
first:

```sql
SELECT id, title, source, platform, status, started_at, duration_ms
FROM meetings
WHERE started_at >= :from_ms AND started_at < :to_ms
ORDER BY started_at DESC
LIMIT :page_size OFFSET :offset;
```

Title search filters these rows further in the shell with a parameterized
`LIKE`; the range index bounds each page regardless.

**Meeting detail transcript** — finalized lines only, partials excluded
because the live ones exist only while the meeting runs:

```sql
SELECT segment_id, channel, speaker, start_ms, end_ms, text,
       original_text, sentiment_score
FROM segments
WHERE meeting_id = :meeting_id AND is_partial = 0
ORDER BY end_ms;
```

Summaries and action items for the same view are fetched by
`meeting_id = :meeting_id`, served by their primary keys and
`idx_action_items_meeting_id_status`.

**Recent transcript window** — the agent's last N finalized lines, newest
first via the backward scan on
`idx_segments_meeting_id_end_ms`, re-reversed into reading order:

```sql
SELECT speaker, start_ms, end_ms, text
FROM segments
WHERE meeting_id = :call_id AND is_partial = 0
ORDER BY end_ms DESC
LIMIT :lines;
```

**Action item tick** — the checklist writes a status flip by row id:

```sql
UPDATE action_items SET status = :status
WHERE action_item_id = :action_item_id;
```

**Semantic search with filters** — top-k over the virtual table, filtered
to a date range and channel, capped at minimum cosine similarity 0.25
(cosine distance ≤ 0.75):

```sql
SELECT c.id, c.text, c.start_ms, c.channel, c.speaker,
       m.id AS meeting_id, m.title, v.distance
FROM rag_chunks_vec v
JOIN rag_chunks c ON c.id = v.chunk_id
JOIN meetings m   ON m.id = c.meeting_id
WHERE v.embedding MATCH :query_embedding
  AND v.k = :top_k
  AND m.started_at >= :from_ms AND m.started_at < :to_ms
  AND c.channel = :channel
  AND v.distance <= :max_distance
ORDER BY v.distance;
```

`document_search` runs the same statement with `c.channel = 'DOC'` and no
date filter.

## Migrations

Schema changes ship as **numbered, forward-only migrations**, applied by
the sidecar at startup before it accepts connections. Each runs once,
inside one transaction, recorded by number:

```sql
CREATE TABLE IF NOT EXISTS _migrations (
  number     INTEGER PRIMARY KEY,
  name       TEXT    NOT NULL,
  applied_at INTEGER NOT NULL
);
```

At startup the sidecar reads `MAX(number)`, applies every pending migration
in ascending order, and inserts a row per applied step. Downgrades across a
migration are unsupported — see [Updates & Migrations](updates-and-migrations.md)
for the backup guidance. Two standing rules:

- Migrations touch only the database. Recordings live outside it as files
  and are never rewritten.
- Switching the embedding model is expressed as a migration that recreates
  `rag_chunks_vec` with the new dimension and re-embeds every `rag_chunks`
  row; settings keys migrate additively, and user prompt templates survive
  verbatim.
