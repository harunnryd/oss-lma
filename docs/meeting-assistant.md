---
title: "Meeting Assistant"
---

# Meeting Assistant

The **Meeting Assistant** answers questions about the current meeting and
your history. It is a LangGraph agent with a tool loop (`python/lma_agent`):
it decides which context to read, streams its answer token by token, and
shows every step it took.

## How it works

The system prompt ships as a versioned asset,
`prompts/agent-system-prompt.txt`, and carries explicit routing rules:

1. Question mentions *this meeting / just said / action items* → read the
   `current_meeting_transcript`
2. Question about past meetings → use `meeting_history` or
   `recent_meetings_list`
3. General knowledge → `web_search` or answer directly
4. Ambiguous → default to the current transcript

Tool docstrings double as selection prompts, telling the model when **not**
to reach for each tool.

## Built-in tools

| Tool | Input | Reads | Returns to model |
|---|---|---|---|
| `current_meeting_transcript(lines?, mode)` | mode: `recent` \| `full` \| `semantic` | live segments | `"SPEAKER [mm:ss]: text"` lines; `semantic` embeds the recent window in memory (no vector store — nothing is indexed before meeting end) |
| `recent_meetings_list(limit)` | limit | meetings table | JSON array `{title, started_at, status}` |
| `meeting_history(query, call_ids?)` | query string | rag_chunks retriever | answer passages with `[title @ mm:ss]` citations |
| `document_search(query)` | query string | rag_chunks (`channel='DOC'`) | passages with `[filename]` citations |
| `web_search(query)` | query string | configured provider | result snippets; hidden entirely when unconfigured |

Multi-turn context flattens the last 10 conversation messages into the
prompt. Tool failures emit a failed thinking step and the graph continue —
one bad tool never aborts an answer.

External tools join via [MCP Servers](mcp-servers.md).

## Streaming protocol

Wire frames are PascalCase (see
[WebSocket Streaming API](websocket-streaming-api.md)). Reasoning progress:

```json
{ "EventType": "THINKING_STEP", "CallId": "...", "QueryId": "...",
  "Seq": 3, "StepType": "tool_use",
  "Content": "",
  "ToolName": "current_meeting_transcript",
  "ToolInput": {"mode": "recent", "lines": 30},
  "ToolResult": null, "Success": null }
```

Questions enter as `AGENT_QUERY {QueryId, Message, History}`; every token
and step streams back correlated by that `QueryId`.

`ToolName`/`ToolInput`/`ToolResult`/`Success` are null except on
`tool_use`/`tool_result` steps. The UI renders them as a timeline — see the
chat panel in the [Desktop App Guide](desktop-app-ui.md).

The ReAct graph runs at most 8 model iterations per question before it must
answer.

## Wake phrase

Data flow, one owner per stage:

1. **Detection** — the pipeline checks each finalized segment against
   `assistant.wake_pattern`, only on channels listed in
   `assistant.wake_channels` (default: CALLER).
2. **Context** — the matched segment plus its preceding utterance window
   (same speaker, up to 30 s back) becomes the agent's input.
3. **Invocation** — the sidecar's assistant module runs the graph; tokens
   stream as `AGENT_TOKEN`.
4. **Delivery** — the final reply is written as an `AGENT_ASSISTANT`
   segment linked to the trigger via `ADD_AGENT_ASSIST {TriggerSegmentId}`.

Default pattern `(OK|Okay)[.,! ]*[Aa]ssistant`; configurable in Settings.

## Shortcut buttons

Canned prompts shipped as defaults (editable via the template layer):

| Button | Prompt |
|---|---|
| SUMMARIZE | Summarize this meeting |
| ACTIONS | What were the action items? |
| TOPICS | What topics were discussed? |
| ASK ASSISTANT! | Please respond to the last question or instruction in this meeting. |
| FACT CHECK | Can you fact-check the recent statements made in this meeting? |
| DISCUSSED BEFORE? | Based on what we're discussing in this meeting, have we talked about this topic in previous meetings? |
| FIND DOCS | Based on the current meeting discussion, are there any relevant company documents or policies? |

## Model selection

Any LLM provider works through the same interface — pick provider and model
in Settings. Summaries run deterministically (temperature 0); the assistant
runs conversational (temperature 0.3). See also
[Transcript Summarization](transcript-summarization.md).
