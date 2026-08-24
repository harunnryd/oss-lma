---
title: "AssemblyAI Setup"
---

# AssemblyAI Setup

**AssemblyAI** gives oss-lma Universal Streaming: a WebSocket API with
turn-based results, per-word timestamps, optional streaming speaker labels,
and multilingual models with language detection. It implements the same
Engine interface as the Deepgram reference adapter
([Transcription & Translation](transcription-and-translation.md)) — its v3
protocol differs in shape (turns instead of utterance finals, milliseconds
instead of seconds), and this page records exactly how that maps. Prices and
model names below were verified 2026-08-24.

## Getting credentials

1. Sign up at [assemblyai.com](https://www.assemblyai.com/app) — new accounts
   get **$50 in free credit**, no card required.
2. In the dashboard open **Workspace → API Keys → Create New API Key**.
   Keys are named; the free plan allows two, so give it a recognizable one,
   then copy the secret when shown.
3. In oss-lma, open **Settings → Transcription**, choose AssemblyAI, and paste
   the key. It is stored in the OS keychain
   ([Prerequisites & Installation](prerequisites-and-install.md)).

The default endpoint `streaming.assemblyai.com` is global and
latency-optimized. If audio must stay in a jurisdiction, regional variants
exist at `wss://streaming.us.assemblyai.com/v3/ws` and
`wss://streaming.eu.assemblyai.com/v3/ws`.

## Recommended model

Streaming models are selected with the `speech_model` connection parameter:

| Model | `speech_model=` value | Languages | Price |
|---|---|---|---|
| **Universal-3.5 Pro Streaming** | `universal-3-5-pro` | 18 languages incl. en/es/de/fr/pt/it/tr/nl/sv/no/da/fi/hi/vi/ar/he/ja/zh; language detection; native code-switching | $0.45/hr |
| Universal Streaming Multilingual | `universal-streaming-multilingual` | en/es/de/fr/pt/it, per-turn switching | $0.15/hr |
| Universal Streaming English | `universal-streaming-english` | English only | $0.15/hr |

For multilingual meetings configure **Universal-3.5 Pro**: broadest language
coverage plus automatic detection, which is what mixed-language meetings need.
If your meetings stay within English + Western European languages, the
multilingual variant transcribes the same meetings at a third of the cost.
The older `universal-2` model is pre-recorded-only and never used for live
streams.

## How oss-lma connects

| Aspect | Specification |
|---|---|
| Handshake | `wss://streaming.assemblyai.com/v3/ws?sample_rate=48000&encoding=pcm_s16le&speech_model=<configured>`; header `Authorization: <key>` — the raw key, no `Bearer` prefix |
| Audio | binary frames of deinterleaved mono s16le PCM; mono-only service, so the adapter opens one Engine session per channel behind the same interface; our 100 ms chunks sit inside the enforced 50–1000 ms frame range |
| Session lifecycle | server replies `Begin` once (session id, expiry); adapter ends streams with `{"type": "Terminate"}`, flushing final turns before the server's `Termination` message (`audio_duration_seconds` is the billing basis) |
| Results mapping | each `Turn` supersedes the previous turn of the same channel by `turn_order` — render latest, never append; `words[]` entries carry `text`, `start`, `end`, `confidence`, `word_is_final` |
| Speakers | turn-level `speaker_label` ("A", "B", …) formatted `spk_N`; per-word `speaker` may be missing or `"PENDING"` until `word_is_final`; a `SpeakerRevision` message after Terminate corrects changed turns |
| Result identity | no native result id — synthesize `${begin_id}-turn-${turn_order}` |
| Failure classification | WebSocket close 1008 at handshake → `ProviderAuthError`; `Error` frames (3006 inactivity, 3007 chunk violation, 3008 3-hour cap, 3009 concurrency) and any other post-handshake close → `ProviderResetError` |

Speaker labels are off by default (`speaker_labels=true` not set): each engine
already covers one meeting channel, and attribution comes from channel
mapping — 0 → `CALLER`, 1 → `AGENT`. Enable labels only if a single channel
mixes several speakers; they are a paid add-on.

Mapping onto the WordItem contract
(`{content, type, start_time, end_time, speaker, channel, result_id}`):

| WordItem field | Source |
|---|---|
| `content` | word `text` |
| `type` | always `pronunciation` — punctuation arrives attached to tokens or in the formatted turn transcript, never as separate tokens |
| `start_time` / `end_time` | word `start` / `end` in milliseconds from stream start, divided by 1000 to float seconds |
| `speaker` | formatted `spk_N` when present; `null` while `"PENDING"`/absent, filled when the turn or `SpeakerRevision` finalizes |
| `channel` | assigned by the pipeline from which engine session produced the item |
| `result_id` | `${begin_id}-turn-${turn_order}`, stable across a turn's partial updates |

## Pricing sanity check

Billing is **per open session duration, not audio sent** — idle silence bills
the same, and un-terminated sessions close and bill fully at 3 hours. Because
oss-lma keeps both channel sessions open for the whole meeting, cost is
wall-clock time × sessions × rate.

Pay-as-you-go rates (verified 2026-08-24):

| Item | Price |
|---|---|
| Universal Streaming English / Multilingual | $0.15/hr |
| Universal-3.5 Pro Realtime | $0.45/hr |
| Speaker diarization add-on | +$0.12/hr |

Worked example — 60-minute meeting, two channel sessions:
Universal-3.5 Pro = 2 hr × $0.45 = **$0.90** (+$0.24 if diarization is on);
multilingual variant = 2 hr × $0.15 = **$0.30**. The $50 free credit covers
roughly 165 meeting-hours at the multilingual rate. Usage appears in the
dashboard billing page.

## Troubleshooting

Symptoms are indexed by error catalog code
([Developer Guide](developer-guide.md#error-catalog)); provider-specific
causes:

**`STT_PROVIDER_AUTH` with close code 1008** — missing or malformed
`Authorization` header (a `Bearer` prefix on the raw key causes rejection),
or an account problem: exhausted credit and disabled accounts also arrive as
1008. Check balance before re-entering the key.

**Garbled or wrong-speed transcripts** — the declared `sample_rate` does not
match the actual chunk rate. v3 accepts any integer between 8000–96000 Hz, so
a mismatch never errors; it silently mis-transcribes. Verify the pipeline's
resample step.

**`ProviderResetError` with error 3007** — input duration violation: chunks
outside the 50–1000 ms window or sent faster than real time. Points at the
capture loop, not the network.

**Stream stops after ~3 hours** — error 3008 maximum session duration; long
meetings need an engine restart. Reconnects go through normal backoff
(`STT_STREAM_RESET`), and five consecutive failures stop the stream by design
([Troubleshooting](troubleshooting.md#transcription)).
