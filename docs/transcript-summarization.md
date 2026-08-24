---
title: "Transcript Summarization"
---

# Transcript Summarization

Every meeting ends with generated summaries and structured action items —
automatically, or on demand while the meeting still runs.

## How it works

- The finalized transcript is formatted (speaker-prefixed lines) and passed
  to one summary chain per template section, run in parallel with
  deterministic settings.
- Sections are independent — a failure in one never blocks the others.
- Results land in the meeting record and stream into the UI as they
  complete.

## Prompt templates

Canonical default templates ship as real files and are the source of truth:

| File | Keys |
|---|---|
| `prompts/summary-templates.json` | `SUMMARY`, `DETAILS`, `ACTIONS` |
| `prompts/summary-templates-healthcare.json` | `SUMMARY`, `DETAILS`, `SOAP`, `BIRP` (few-shot note examples) |
| `prompts/chat-buttons.json` | the seven shortcut-button prompts |

Templates come in two layers at runtime:

| Layer | Source | Editing |
|---|---|---|
| **Default** | the files above, loaded read-only | via file PRs |
| **Custom** | your overrides (`prompt_templates` table) | Settings → prompt editor |

A custom template replaces its default when keys match; a value of `NONE`
or empty disables that section entirely. `{transcript}` is the only
variable; `<br>` entities normalize to newlines at load time. Example,
verbatim from the shipped default:

> The following is the transcript of a meeting.<transcript>{transcript}</transcript>
> Write a concise summary of the meeting without preamble or additional
> explanation. Where possible reference speaker names, but only use gender
> neutral pronouns.

The healthcare variant is selected by the `domain` setting.

## Section chains

Each section runs as one chain — single user message from the template,
temperature 0, max output 512 tokens — executed in parallel across
sections; one section failing never blocks the others. Results stream into
the UI per section as they complete.

## Structured action items

After section chains complete, one extraction pass converts the ACTIONS
output plus the finalized transcript into rows:

- **Input**: the ACTIONS section text + speaker-prefixed transcript lines
- **Method**: one LLM call with structured output (temperature 0)
- **Output row**:

```json
{ "description": "Send revised budget", "detail": "…",
  "owner": "TBD", "due_date": null, "status": "open",
  "source_segment_id": "r7-CALLER-w1-r2" }
```

`source_segment_id` is resolved by matching each item's quoted text span
back to transcript segments (exact match first, then fuzzy; unresolved →
null). The [meeting detail view](desktop-app-ui.md) renders rows as an
editable checklist — ticking sets `status`.

## On-demand summaries

Shortcut buttons in the chat panel run any template against the live
transcript without ending the meeting — same templates, same layering.
See [Meeting Assistant](meeting-assistant.md).
