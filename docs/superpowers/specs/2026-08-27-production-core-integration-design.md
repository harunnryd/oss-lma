# Production Core Integration Design

## Status

Approved design for the first product-ready oss-lma increment. This document
implements the documented desktop-capture workflow with a real provider and
secure runtime configuration. It does not implement the assistant, RAG,
summaries, virtual participant, voice assistant, or Windows capture; each is
a separate follow-up design.

## Goal

Make the existing macOS desktop capture vertical slice usable as an end-user
application: a user configures a real STT provider, grants permissions,
starts a meeting in Tauri, sees live transcript events, and finishes with a
locally stored transcript and WAV recording.

The smoke-tested reference path is Deepgram STT. AssemblyAI and Azure Speech
are implemented behind the same engine interface and covered by deterministic
contract tests, but require their respective user credentials for a live
provider smoke test. OpenAI, LangChain, and LangGraph belong to the later
assistant increment.

## Scope and Non-goals

### In scope

- Tauri supervision of the Python sidecar lifecycle.
- Secure, in-memory transfer of runtime provider configuration to the
  sidecar.
- Persistent non-secret provider and capture settings in the Rust-owned
  SQLite tables.
- OS-keychain storage for provider API keys.
- A provider registry and concrete Deepgram, AssemblyAI, and Azure Speech
  STT engine adapters.
- A usable Tauri webview with onboarding/settings and live-meeting screens.
- End-to-end macOS desktop capture using Deepgram and local persistence.
- Deterministic tests for all provider adapters, supervisor behavior, and
  protocol compatibility.

### Explicitly out of scope

- LangGraph assistant, LangChain tools, summaries, action-item extraction,
  translation, embeddings, RAG, documents, MCP, web search, or TTS.
- Virtual participant scheduling, Docker images, browser adapters, and
  takeover UI.
- Windows capture implementation or physical Windows-device smoke testing.
- Changing the documented SQLite table ownership model.

## Existing Foundations

The implementation extends rather than replaces the working vertical slice:

- `lma-link` already implements the authenticated, reconnecting sidecar
  client and fixed-size stereo frames.
- `lma-capture` captures macOS system audio and microphone audio, then mixes
  and records 48 kHz stereo PCM.
- The Python sidecar already validates wire frames, persists meetings and
  segments, and runs the segment assembler.
- `lma_stt.deepgram` implements the documented Deepgram WebSocket mapping.
- The current sidecar entrypoint uses `FakeEngine`; the current HTML UI has
  only static capture controls and passes no usable sidecar credentials.

## Architecture

### Process ownership

The Rust Tauri application owns the application lifecycle, user interaction,
keychain access, and capture configuration. It creates a `SidecarSupervisor`
at startup. The supervisor launches the sidecar as a child process, accepts
only the documented `SIDECAR_READY port=<port> token=<hex>` readiness line,
and retains the resulting endpoint in process memory.

The Python sidecar owns active meeting state, STT sessions, transcript
processing, and the Python-owned SQLite tables. It never loads secrets from
SQLite. The sidecar receives a short-lived runtime configuration at launch
through a private inherited pipe; that payload includes the selected provider
and its API credential. It is not passed as a CLI argument, written to disk,
logged, exposed through WebSocket frames, or returned to the webview.

The Rust shell owns its documented SQLite tables (`settings`,
`prompt_templates`, `mcp_servers`, and `vp_schedules`) and opens its
connection with WAL, foreign keys, and the same busy timeout. Python retains
ownership of `meetings`, `segments`, `summaries`, `action_items`,
`rag_chunks`, and `vp_tasks`. This increment adds only Rust-owned settings
migrations; it does not allow both processes to write a table.

### Runtime configuration

`ProviderSettings` records provider selection and public options:

- STT provider (`deepgram`, `assemblyai`, or `azure`), model, language, and
  Azure region when applicable.
- The keychain reference/key-presence state, never the secret value.
- Capture preferences already exposed by the Rust app, such as microphone
  choice and diarization flags.

The UI can create, update, select, and delete a provider configuration. A
missing secret makes the configuration invalid for starting a meeting and
returns an actionable structured error. The supervisor resolves the selected
secret from the OS keychain just before sidecar spawn.

On launch, the sidecar parses its private configuration exactly once and
constructs an `EngineRegistry`. It selects the configured `SpeechEngine`
factory for each meeting. The registry is dependency-injected for unit and
integration tests; no adapter reads global environment variables directly.

### STT adapter contract

All engines expose the existing asynchronous start/feed/result/close contract
and yield normalized `Result`/`WordItem` objects. Provider-specific protocol
translation happens entirely in the adapter.

- Deepgram consumes the documented interleaved stereo 48 kHz PCM stream.
- AssemblyAI starts one mono PCM s16le 48 kHz connection per channel,
  deinterleaves incoming stereo frames, and merges results with stable
  channel/result identities.
- Azure starts one mono 16 kHz connection per channel, deinterleaves and
  downsamples the source PCM, sends its required WAV/header framing, and
  merges detailed results. Its speakers remain unset because the documented
  raw WebSocket path does not diarize.

Each adapter maps authentication rejection to `ProviderAuthError` and any
post-handshake transport/provider failure to `ProviderResetError`. Existing
sidecar reconnect policy remains the only retry owner.

### Tauri command and event boundary

The shell keeps sidecar endpoint details private. Capture start no longer
accepts arbitrary `port` or `token` from the webview. Instead, it asks the
supervisor for a healthy endpoint and passes those values directly to
`lma-link`.

Tauri commands provide:

- provider configuration CRUD and secret-presence status;
- capture permissions and devices;
- meeting start/pause/resume/stop and current status;
- meeting-event subscription or replay for the active UI window.

The Rust bridge forwards valid sidecar events to the UI through named Tauri
events. Transcript rendering keys updates by `SegmentId`, replacing partial
text in place. Errors use the code in `contracts/errors.yaml`; the UI maps
codes to user-facing recovery actions and never parses message text.

### Usable UI scope

The static page becomes a small, framework-free Tauri UI in this increment:

1. Onboarding checks microphone and Screen Recording permissions, then
   directs the user to Settings to select and configure a provider.
2. Settings saves provider selection and public options, stores the supplied
   secret in keychain, and shows only whether a secret is present.
3. Live Meeting shows capture phase, elapsed time, record/pause/resume/stop,
   per-channel active/mute state where available, connection health, and a
   color-coded live transcript.

Meeting history, assistant chat, search, and VP screens are intentionally not
shown as implemented features in this increment.

## Data Flow

1. The user saves a provider configuration. Rust stores public fields in its
   settings table and stores the API key in the OS keychain.
2. On app startup or a managed restart, `SidecarSupervisor` reads the active
   configuration and injects a single runtime configuration payload into the
   child sidecar. It parses the readiness line and exposes an internal
   endpoint to Rust services.
3. On Record, capture checks permissions and source readiness, obtains the
   endpoint from the supervisor, creates one `CallId`, then sends `START`
   followed by exact 100 ms stereo PCM frames.
4. The sidecar selects its engine, feeds PCM, converts results to segments,
   persists them to the Python-owned tables, and sends contract-valid events
   back over the same WebSocket.
5. The Rust bridge emits those events to the live UI. On Stop, it sends
   `END`; the sidecar finalizes persistence and the capture layer closes the
   WAV at the documented recording path.
6. If the sidecar process exits, the supervisor respawns it and replaces the
   in-memory endpoint. `lma-link` reconnects using a fresh `START` with the
   same `CallId`, subject to its existing bounded buffer and backoff rules.

## Failure Handling

- Invalid/missing provider configuration prevents start before capture
  resources are allocated.
- A bad secret maps to `STT_PROVIDER_AUTH`; the UI offers Settings without
  exposing provider response text or the secret.
- A provider stream reset follows the existing sidecar retry budget. Capture
  and WAV recording remain active during provider reconnection.
- Sidecar failure makes its current endpoint invalid. The supervisor emits a
  health transition, respawns with newly generated handshake credentials, and
  instructs active capture to reconnect through the replacement endpoint.
- Permission denial and native source failures remain preflight or session
  errors according to the existing capture state machine.
- The only stdout consumed from the sidecar is its readiness line; all
  diagnostics belong on stderr to preserve the parser boundary.

## Testing and Acceptance

### Automated

- Unit tests prove provider URL/config generation, auth classification,
  normalized word mapping, mono deinterleave/downsample behavior, close
  semantics, and stable result identities for Deepgram, AssemblyAI, and
  Azure.
- Registry tests prove selection, missing configuration rejection, and
  dependency injection.
- Supervisor tests use a controllable fake child process to prove readiness
  parsing, malformed/multiple readiness-line rejection, shutdown, crash
  respawn, and endpoint replacement without leaking tokens.
- Rust command tests prove a webview caller cannot supply sidecar token or
  port, and provider secrets never appear in serialized command responses or
  settings rows.
- End-to-end tests run sidecar + fake engine + Rust link, asserting event
  forwarding and SQLite/WAV output. Existing contract validation remains
  mandatory for each emitted wire frame.
- CI runs Python tests/lint, Rust tests/clippy, and supported target builds.
  Windows target compilation/tests are required once a Windows runner is
  available; physical device validation is not claimed without that hardware.

### macOS acceptance smoke test

With a user-supplied Deepgram key and granted Screen Recording and microphone
permissions:

1. Launch the packaged Tauri app.
2. Configure and select Deepgram in Settings; verify the UI reports key
   presence without displaying the key.
3. Start a real meeting or system-audio playback plus microphone input.
4. Confirm transcript events arrive in the Live Meeting screen.
5. Pause, resume, and stop. Confirm a local WAV and persisted transcript are
   present for the one `CallId`.
6. Verify application logs, SQLite, and process command line contain no API
   key.

## Follow-up Designs

After this increment is accepted, separate specs and plans will cover:

1. LangChain/LangGraph assistant, OpenAI provider, summaries, action items,
   translation, embeddings, RAG, documents, and MCP.
2. Virtual participant scheduler, Docker runtime, Zoom/Google Meet adapters,
   consent, takeover, and voice assistant.
3. Windows native capture, permissions/device handling, CI runner, and a
   physical Windows smoke test.
