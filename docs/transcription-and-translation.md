---
title: "Transcription & Translation"
---

# Transcription & Translation

Every meeting source — [desktop capture](desktop-capture-app.md) or
[Virtual Participant](virtual-participant.md) — feeds the same transcription
pipeline: a **pluggable speech-to-text layer** with normalized output,
speaker attribution, multi-language support, and local recording.

## Pluggable STT providers

Speech-to-text is cloud-based and swappable. Every provider implements one
engine interface (`python/lma_stt`) and converts its native payloads into a
common word-item shape at the adapter boundary — nothing downstream sees
vendor formats.

| Provider | Streaming | Role |
|---|---|---|
| **Deepgram** | WebSocket | reference implementation |
| **AssemblyAI** | WebSocket | adapter |
| **Azure Speech** | WebSocket | adapter |

Provider selection, keys, and language options live in Settings. Provider
auth failures surface as `STT_PROVIDER_AUTH`; mid-stream failures reconnect
with backoff (`STT_STREAM_RESET`) — five consecutive failures stop the
stream by design; a session that survived ≥10 s resets the failure counter.

## Pipeline specification

**Input contract** — adapters yield normalized word items:

```json
{ "content": "budget", "type": "pronunciation",
  "start_time": 12.42, "end_time": 12.88, "speaker": "spk_1" }
```

`type` is `pronunciation` or `punctuation`; times are float seconds from
stream start; `speaker` is a formatted label (`spk_N`) or `null`.

**Channel mapping** is owned by the pipeline, not the client: channel 0 =
system/meeting audio = `CALLER`, channel 1 = microphone = `AGENT`.

**Stage flow** — order matters and is pinned by upstream's measured design:

1. Each engine result's items are **bucketed by timestamp into windows** of
   ≤ `MAX_SEGMENT_SECONDS` (20 s), anchored to the result start. Engines cap
   results near 30 s and label only finals, so windowing exists to let the
   live view settle earlier.
2. Within each window, contiguous same-speaker **runs** are built from
   labelled items; punctuation rides along unlabelled — text is never
   dropped.
3. Runs below **either** threshold (`MIN_RUN_WORDS` 3 or
   `MIN_RUN_SECONDS` 0.5) are **absorbed**: a weak run follows its previous
   strong neighbour; leading weak runs wait and prepend to the first strong
   run; all-weak input collapses under its first label.
4. A window whose audio is already in the past is emitted **FINAL early**
   even while its engine result is still partial — this is what keeps the
   live view settling within ~20 s instead of ~30 s.
5. When the engine finalizes the result, all windows re-emit with confirmed
   labels, overwriting by stable ID.

Two invariants make partial→final safe: the **window anchor is computed
once per result** and never recomputed from revised timestamps (boundary
items cannot migrate between windows), and each assembler keeps a
**high-water mark of already-settled windows**, so per-partial updates only
touch the newest window. Setting `MAX_SEGMENT_SECONDS=0` disables
windowing and follows engine boundaries exactly.

Each emission becomes an `ADD_TRANSCRIPT_SEGMENT` event with a stable ID;
non-diarized channels bin items against the active speaker declared by
`SPEAKER_CHANGE` for that channel.

**Measured constants** (tuned against real two-speaker recordings, where
run lengths are cleanly bimodal — noise runs 1–2 words / 0.1–0.9 s, real
turns 6–42 words / 1.2–13.4 s):

| Constant | Default | Purpose |
|---|---|---|
| `MIN_RUN_WORDS` | 3 | a speaker run shorter than this is noise |
| `MIN_RUN_SECONDS` | 0.5 | same, by duration |
| `MAX_SEGMENT_SECONDS` | 20 | longest stretch in one in-progress segment (engines cap results near 30 s and label only finals) |

**Segment identity grammar** — stable across partial→final, deliberately
label-independent (labels arrive only on finals; identity must not depend on
them):

```text
diarized:      ${result_id}-${channel}-w${window}-r${run}
non-diarized:  ${speaker}-${start_time_ms}-${channel}
```

`channel` is the wire channel token (`CALLER`/`AGENT`). In the non-diarized
form the speaker comes from the `SPEAKER_CHANGE` timeline; a mid-utterance
speaker change starts a **new** segment rather than mutating the old ID.
Worked example of partial→final stability:

```text
result r7, window 1, run 2 → "r7-CALLER-w1-r2"   (partial, speaker null)
final labels arrive        → "r7-CALLER-w1-r2"   (same ID, Speaker filled)
```

Run indexes are assigned after absorption, so smoothing changes never
rewrite IDs of already-emitted windows.

Finals overwrite partials by primary key in SQLite — no orphaned partials.

## Deepgram reference adapter

The reference implementation of the Engine interface:

| Aspect | Specification |
|---|---|
| Handshake | `wss://api.deepgram.com/v1/listen?encoding=linear16&multichannel=true&channels=2&sample_rate=48000&model=<configured>&interim_results=true&smart_format=true&diarize=true`; header `Authorization: Token <key>` |
| Audio | binary frames, exactly the 100 ms stereo chunks from the wire protocol |
| KeepAlive | `{"type": "KeepAlive"}` JSON every 5 s during silence so the session survives pauses |
| Results mapping | each response's `channel.alternatives[0].words[]` → one `WordItem` per word (`punctuated_word`, `word`, `start`, `end`); multichannel responses carry per-channel results → `channel` assignment |
| Speakers | per-channel `speaker` integers formatted `spk_N`; only present on `is_final` responses |
| Result identity | no native result id — synthesize `${metadata.request_id}-${sequence}` |
| Failure classification | HTTP 401/403 at handshake → `ProviderAuthError`; any post-handshake close/error frame → `ProviderResetError` |

AssemblyAI and Azure adapters follow the same table shape in their own docs
when implemented.

## Multi-language support

Meetings transcribe in whatever language is spoken — providers with
automatic language identification handle mixed usage without configuration.
The assistant and summaries work across languages; prompt templates are
language-neutral.

## Live translation

Transcript segments can be translated for display using the configured LLM
provider — each finalized segment is translated on arrival and shown
alongside the original in the meeting detail view.

## Recording

While transcribing, the raw stereo mix is written to
`recordings/<meeting_id>/audio.wav`; VP sessions also keep video. Both play
back in the meeting detail view.
