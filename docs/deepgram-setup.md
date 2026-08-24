---
title: "Deepgram Setup"
---

# Deepgram Setup

**Deepgram** is oss-lma's reference speech-to-text provider: a streaming
WebSocket API that accepts the full stereo capture mix on one connection,
returns per-channel word timings, and labels speakers on finalized results.
It implements the Engine interface described in
[Transcription & Translation](transcription-and-translation.md) — every other
provider adapter is held to the behavior this one establishes. Prices and
model names below were verified 2026-08-24.

## Getting credentials

1. Sign up at [console.deepgram.com](https://console.deepgram.com). New
   accounts get **$200 in free credit**, no card required; unused credit does
   not expire.
2. Open **API Keys** → **Create a New Key**, give it a name, and copy the
   secret when it is shown — it is displayed once.
3. In oss-lma, open **Settings → Transcription**, choose Deepgram, and paste
   the key. It is stored in the OS keychain
   ([Prerequisites & Installation](prerequisites-and-install.md)).

Deepgram has no region selection — `api.deepgram.com` is a single global
endpoint. Concurrency limits apply per plan (150 streaming connections on
pay-as-you-go); one oss-lma meeting uses one connection.

## Recommended model

| Model | `model=` value | Languages | Role |
|---|---|---|---|
| **Nova-3** | `nova-3` | ~50 languages, one per request; `language=multi` enables code-switching across en/es/fr/de/hi/ru/pt/ja/it/nl | recommended |
| Nova-2 | `nova-2` | wide coverage, older generation | still selectable |
| Flux | `flux-general-*` | English-focused turn-detection agents | not used by oss-lma |

For multilingual meetings configure **Nova-3 with `language=multi`**: it
detects and switches among the ten covered languages per word, which is what
mixed-language meetings need without per-meeting configuration. If meetings
are reliably single-language, pinning that language uses the cheaper
monolingual rate. When switching to `language=multi`, Deepgram recommends
tighter endpointing (`endpointing=100`) so language boundaries flush promptly;
oss-lma sets this automatically.

Diarization stays enabled (`diarize=true`). Do not additionally set
`diarize_model` — Deepgram rejects a request containing both.

## How oss-lma connects

The adapter is the reference implementation of the Engine interface
([Transcription & Translation](transcription-and-translation.md#deepgram-reference-adapter)):

| Aspect | Specification |
|---|---|
| Handshake | `wss://api.deepgram.com/v1/listen?encoding=linear16&multichannel=true&channels=2&sample_rate=48000&model=<configured>&interim_results=true&smart_format=true&diarize=true`; header `Authorization: Token <key>` |
| Audio | binary frames, exactly the 100 ms stereo chunks from the wire protocol |
| KeepAlive | `{"type": "KeepAlive"}` JSON every 5 s during silence so the session survives pauses |
| Results mapping | each response's `channel.alternatives[0].words[]` → one `WordItem` per word (`punctuated_word`, `word`, `start`, `end`); multichannel responses carry per-channel results → `channel` assignment |
| Speakers | per-channel `speaker` integers formatted `spk_N`; only present on `is_final` responses |
| Result identity | no native result id — synthesize `${metadata.request_id}-${sequence}` |
| Failure classification | HTTP 401/403 at handshake → `ProviderAuthError`; any post-handshake close/error frame → `ProviderResetError` |

A resolved handshake for the recommended configuration appends the language
option from Settings:

```text
wss://api.deepgram.com/v1/listen?encoding=linear16&multichannel=true&channels=2
  &sample_rate=48000&model=nova-3&language=multi
  &interim_results=true&smart_format=true&diarize=true
```

Mapping onto the WordItem contract
(`{content, type, start_time, end_time, speaker, channel, result_id}`):

| WordItem field | Source |
|---|---|
| `content` | `punctuated_word`, falling back to `word` |
| `type` | `punctuation` when `punctuated_word` carries attached punctuation that `word` lacks, otherwise `pronunciation` |
| `start_time` / `end_time` | `start` / `end` — already float seconds from stream start |
| `speaker` | per-channel integer `speaker`, formatted `spk_N`; `null` until an `is_final` response carries it |
| `channel` | response channel index, mapped by the pipeline: 0 → `CALLER`, 1 → `AGENT` |
| `result_id` | `${metadata.request_id}-${sequence}`, stable across partial → final |

Timestamps continue through silence: KeepAlive frames hold the session open
but are not counted as audio, so word offsets stay aligned with the local
recording ([Security & Privacy](security-and-privacy.md) — audio goes only to
this endpoint).

## Pricing sanity check

Pay-as-you-go streaming rates (verified 2026-08-24; the lower figures were
flagged limited-time on the pricing page):

| Configuration | Price |
|---|---|
| Nova-3 monolingual streaming | $0.0048/min (regular $0.0077/min) |
| Nova-3 multilingual (`language=multi`) streaming | $0.0058/min (regular $0.0092/min) |
| Pre-recorded Nova-3 | $0.0043/min — streaming rates differ |

**Multichannel audio bills per channel**: a 10-minute stereo file bills as 20
minutes, so oss-lma's two-channel session doubles the per-minute figure.

Worked example — 60-minute meeting, `channels=2`, Nova-3 `language=multi`:
120 billable minutes × $0.0058 = **≈ $0.70** ($1.10 at the regular rate).
Single-language: 120 × $0.0048 ≈ **$0.58**. The $200 credit covers roughly
280 such meetings. Monitor usage in the Deepgram console; growth-plan
pre-paid credits discount further once volume justifies it.

## Troubleshooting

Symptoms are indexed by error catalog code
([Developer Guide](developer-guide.md#error-catalog)); provider-specific
causes:

**`STT_PROVIDER_AUTH` at connect** — HTTP 401 `INVALID_AUTH`: the key is
wrong, revoked, or pasted with extra whitespace; re-enter it in Settings.
HTTP 403 means the key authenticated but the project lacks access to the
configured model — check the `model=` value, not the key.

**`STT_STREAM_RESET` repeating mid-meeting** — HTTP 402
`ASR_PAYMENT_REQUIRED` (credit exhausted) arrives as a post-handshake
failure, so it surfaces as a reconnect loop rather than an auth error; only
401/403 classify as auth. Check remaining credit in the console. Five
consecutive failures stop the stream by design.

**Close code 1011 with `NET-0001` / `NET-0002`** — the service stopped
receiving frames during silence. KeepAlive resets the `NET-0002`
no-audio timer but cannot prevent `NET-0001` (no frames of any kind), so this
pattern usually means local capture stalled — see
[Troubleshooting](troubleshooting.md#capture).

**Close code 1008 with `DATA-0000`** — the payload was not decodable as the
configured encoding; the declared `sample_rate`/`encoding` no longer matches
what the capture pipeline is sending.
