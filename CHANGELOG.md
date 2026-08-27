# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial skeleton: documentation, contracts layout, workspace structure.
- Desktop capture for macOS: system audio via ScreenCaptureKit and microphone
  audio via AVAudioEngine, mixed into 48 kHz interleaved 16-bit stereo PCM at
  100 ms tick granularity, exposed to the Tauri UI and persisted to
  `<app-data>/recordings/<meeting_id>/audio.wav`. Bounded reconnect buffer,
  device rebuild without meeting interruption, pause and per-channel mute.
- Sidecar reconnect with cumulative time-offset continuity: STT streams recover
  from `ProviderResetError` via exponential backoff (500 ms→10 s, capped),
  segments written across reconnects share one continuous wire timeline, and
  the meeting row resumes from its persisted `time_offset_ms` after a sidecar
  restart. Reconnect budget exhaustion closes the WebSocket with code 1013
  and marks the meeting FAILED.
- Sidecar SQLite persistence for transcript segments, summaries, agent
  outputs, thinking steps, and WAV recording, exposed to the query tool.

### Fixed

- Sidecar stream pump guards against provider exceptions and timeouts during
  drain, surfaces structured ERROR frames, and rejects binary frames whose
  size does not match the negotiated sample rate.
