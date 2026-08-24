---
title: "Azure Speech Setup"
---

# Azure Speech Setup

**Azure AI Speech** gives oss-lma real-time transcription through Microsoft's
raw WebSocket protocol: per-region resources, a fixed price of $1.00 per
audio hour on the standard tier, and word-level timings in detailed output
mode. It implements the same Engine interface as the Deepgram reference
adapter ([Transcription & Translation](transcription-and-translation.md)) —
its protocol differs in shape (tick-based timestamps, phrase finals instead
of utterance results, no speaker labels), and this page records exactly how
that maps. Prices and endpoints below were verified 2026-08-24.

## Getting credentials

1. In the [Azure portal](https://portal.azure.com) create a **Speech**
   resource (Azure AI services family). Pick the region closest to you — the
   region determines every endpoint hostname.
2. Choose a tier: **Free F0** gives 5 audio hours per month (shared between
   standard and custom usage); **Standard S0** is pay-as-you-go with no
   hourly cap.
3. Open the resource → **Keys and Endpoint**: copy **KEY 1** and note the
   region name (`westus`, `westeurope`, …).
4. In oss-lma, open **Settings → Transcription**, choose Azure Speech, paste
   the key, and select the region. The key is stored in the OS keychain
   ([Prerequisites & Installation](prerequisites-and-install.md)); the
   adapter derives `wss://<region>.stt.speech.microsoft.com` from it.

## Recommended model

Azure's standard real-time STT has **no selectable model identifier** — the
base model ships service-side and improves without client changes.
Configuration is by locale and resource:

| Option | What it is | Role |
|---|---|---|
| Standard real-time (S0/F0), conversation mode | base service model, one BCP-47 locale per connection | recommended |
| Custom Speech | your own trained model behind a hosted endpoint, custom real-time rate ($1.20/hr) | domain-heavy jargon only |
| Fast transcription API | REST over ≤60 s audio clips | not usable for live streams |

For multilingual meetings: automatic language identification exists in the
service context protocol SDKs send (`languageId.languages`, up to 4 locales
detected at audio start or 10 continuously), but it is not part of the
officially documented raw-socket contract — oss-lma pins the single locale
configured in Settings per meeting. Switch languages by restarting the
meeting with a different locale.

## How oss-lma connects

| Aspect | Specification |
|---|---|
| Handshake | `wss://<region>.stt.speech.microsoft.com/speech/recognition/conversation/cognitiveservices/v1?language=<locale>&format=detailed`; header `Ocp-Apim-Subscription-Key: <key>` (or `Authorization: Bearer <token>` minted from `https://<region>.api.cognitive.microsoft.com/sts/v1.0/issueToken`, valid 10 minutes and scoped to its issuing host) |
| Config frame | first message is a UTF-8 JSON text frame carrying headers `Path: speech.config`, `X-RequestId`, `Content-Type: application/json; charset=utf-8` |
| Audio | binary frames prefixed with a big-endian header-length field plus `Path: audio` / `Content-Type: audio/x-wav` headers; the first frame body starts with the 44-byte RIFF/WAV header, later frames are raw PCM. Mono-only streaming input (16-bit PCM at 8/16 kHz), so the adapter downsamples each channel to 16 kHz and opens one Engine session per channel behind the same interface |
| Results mapping | `speech.hypothesis` messages are partials; `speech.phrase` messages are finals whose detailed output carries `NBest[].Words[]` → one `WordItem` per word |
| Speakers | none on this path — standard streaming returns no speaker labels; attribution comes from channel mapping (0 → `CALLER`, 1 → `AGENT`). Diarization exists only in the separate conversation-transcription and batch APIs |
| Result identity | no native result id — synthesize `<connection_id>-<phrase_sequence>` |
| Failure classification | HTTP 401/403 during the upgrade request → `ProviderAuthError`; any post-handshake disconnect or error frame → `ProviderResetError` |

Mapping onto the WordItem contract
(`{content, type, start_time, end_time, speaker, channel, result_id}`):

| WordItem field | Source |
|---|---|
| `content` | word token text from `NBest[].Words[].Word`; display punctuation rides in the phrase's `DisplayText` |
| `type` | `punctuation` when the token text carries attached punctuation absent from the lexical form, otherwise `pronunciation` |
| `start_time` / `end_time` | `Offset` / `Duration` in 100-nanosecond ticks from the first processed audio byte, divided by 10,000,000 to float seconds |
| `speaker` | always `null` — no diarization on this path |
| `channel` | assigned by the pipeline from which engine session produced the item |
| `result_id` | synthesized per connection and phrase sequence, stable across hypothesis → phrase |

`format=detailed` is required: simple output returns only `DisplayText`,
which has no per-word offsets, and the windowing stage depends on word
timestamps.

## Pricing sanity check

Billing measures audio sent to the service, in one-second increments. Two
parallel channel engines mean a stereo meeting sends twice its wall-clock
length.

Rates (verified 2026-08-24 against the Azure Speech pricing page and
third-party mirrors; Microsoft renders prices dynamically per region):

| Item | Price |
|---|---|
| Real-time standard (S0) | $1.00/audio hour ≈ $0.0167/min |
| Real-time custom | $1.20/audio hour |
| Fast transcription (non-streaming) | $0.36/audio hour |
| Free F0 allowance | 5 audio hours/month |

Worked example — 60-minute meeting, two channel engines at 16 kHz:
2 audio-hours × $1.00 = **$2.00**. That makes Azure the most expensive STT
option in oss-lma (Deepgram Nova-3 multilingual ≈ $0.70, AssemblyAI
Universal Streaming ≈ $0.30 for the same meeting). Choose it when data must
stay inside an Azure region or you already carry Azure commitment. The F0
allowance covers about 2.5 such meetings per month.

## Troubleshooting

Symptoms are indexed by error catalog code
([Developer Guide](developer-guide.md#error-catalog)); provider-specific
causes:

**`STT_PROVIDER_AUTH` with HTTP 401** — expired bearer token, or a token
minted by a mismatched host: tokens are scoped to their issuing endpoint, so
a token from a custom-domain host fails against
`<region>.stt.speech.microsoft.com`. Prefer sending the resource key directly
via `Ocp-Apim-Subscription-Key`.

**`STT_PROVIDER_AUTH` with HTTP 403** — key and region do not match, or the
resource does not have speech enabled; re-check **Keys and Endpoint**.

**`STT_STREAM_RESET` repeating mid-meeting** — regional service disruption
or network loss; check the Azure status page for the resource's region. Five
consecutive failures stop the stream by design.

**Transcripts without usable timing** — the handshake omitted
`format=detailed`, so responses carry only `DisplayText`; the windowing stage
needs per-word offsets and will not settle segments correctly
([Troubleshooting](troubleshooting.md#transcription)).
