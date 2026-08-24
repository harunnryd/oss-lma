---
title: "Meetings Query Tool"
---

# Meetings Query Tool

Semantic search across every meeting you have had: transcripts, summaries,
and ingested documents — answered with citations linking back to the exact
meetings.

## How it works

- At meeting end, finalized transcript segments, summaries, and imported
  documents chunk (~300 tokens, 15% token overlap) and embed into a **local
  vector store** (`rag_chunks`, sqlite-vec virtual table, cosine distance).
- Default embedder: `multilingual-e5-small` (384 dims) via local ONNX — no
  API key, multilingual by default. Cloud embedding providers are
  selectable; the store's dimension is fixed at creation, so switching
  models triggers an automatic full re-embed (see
  [Updates & Migrations](updates-and-migrations.md)).
- Chunk row shape (flat columns; citations depend on the offsets):

```json
{ "id": "...", "meeting_id": "...", "text": "…",
  "embedding": [0.013, …], "start_ms": 872000, "end_ms": 901500,
  "channel": "CALLER", "speaker": "Budi", "created_at": "…" }
```

Document imports use channel `"DOC"` with `speaker = null`.

Queries filter before vector search runs:

| Filter | Syntax |
|---|---|
| Meeting | `meeting_id` equality |
| Date range | `started_at` between presets/custom |
| Channel | one of `CALLER`, `AGENT`, `AGENT_ASSISTANT`, `DOC` |
| Speaker | exact string |

Defaults: top-k 8, minimum cosine similarity 0.25, follow-up questions
reuse the previous query's session context (last query + answer).

## Citations

Chunk offsets make citations computable — `t=` comes straight from the
chunk's `start_ms`:

```text
… budget was approved [Team Sync — 14:32](oss-lma://meeting/<id>?t=872)
```

The detail view opens scrolled and cued to that point in playback.

## Document ingestion

Settings → **Documents**: import `.md`, `.txt`, or `.pdf`. Files chunk and
embed with the same parameters as transcripts and appear to the agent as
`document_search` results with `[filename]` citations.

## Where it surfaces

- The assistant's `meeting_history` and `document_search` tools
- The dedicated search view in the [Desktop App Guide](desktop-app-ui.md)

Ingestion defers gracefully when embeddings are unavailable
(`RAG_EMBEDDING_UNAVAILABLE`) and can be retried per meeting from the
detail view.

## Where it surfaces

- The assistant's `meeting_history` and `document_search` tools
- The dedicated search view in the [Desktop App Guide](desktop-app-ui.md)

Ingestion defers gracefully when embeddings are unavailable
(`RAG_EMBEDDING_UNAVAILABLE`) and can be retried per meeting from the
detail view.
