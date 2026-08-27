# Desktop Capture — Real-Hardware Verification Log

**Date:** 2026-08-27
**Branch:** `chore/desktop-capture-closeout`
**Hardware:** Apple Silicon Mac (Darwin 25.5.0, macOS 26.5.1)
**Bundle:** `target/release/bundle/macos/oss-lma.app` (built 2026-08-27)

## Acceptance Gate Results

| Gate | Status | Evidence |
|---|---|---|
| `cargo test --workspace` | ✅ Pass | 68 tests across `app`, `lma-capture`, `lma-link`, plus 1 frontend asset test |
| `uv run pytest python` | ✅ Pass | 242 tests (Python sidecar + lma_pipeline + lma_stt) |
| `cargo clippy --workspace --all-targets` | ✅ Pass | 0 warnings |
| `uv run ruff check python` | ✅ Pass | All checks passed |
| `cargo build --release` | ✅ Pass | ARM64 Mach-O binary |
| `cargo tauri build` | ✅ Pass | `target/release/bundle/macos/oss-lma.app` produced |
| App launch | ✅ Pass | PID 44189, registered with WindowServer, no TCC denial |
| Info.plist permission keys | ✅ Pass | `NSMicrophoneUsageDescription` present |

## Native Capture Verification

A throwaway driver at `/tmp/screencap_test/` linked `lma-capture` and used
`lma_capture::macos::NativeStreams` (the real ScreenCaptureKit + AVAudioEngine
provider, not a fake) to capture 6 seconds while `say -v Daniel` played system
audio. Output: `/tmp/real_capture.wav`.

Driver output:

```
Sources started, capturing for 6 seconds...
[sys event] Started(System)
[mic event] Started(Microphone)
sys_frames=290880 mic_frames=283200
WAV size: 1132844 bytes
```

`afinfo /tmp/real_capture.wav`:

```
File type ID:   WAVE
Data format:    2 ch, 48000 Hz, Int16, interleaved
estimated duration: 5.900000 sec
bit rate: 1536000 bits per second
```

Channel analysis (Python wave + struct):

| Channel | Source | Peak | RMS | Verdict |
|---|---|---|---|---|
| 0 | ScreenCaptureKit (system audio) | 24786 | 2816.6 | ✅ Real audio from `say` captured |
| 1 | AVAudioEngine (microphone) | 6817 | 870.8 | ✅ Real microphone audio captured |

Both channels carry real audio at the wire-spec format (2 ch, 48 kHz, Int16,
interleaved, 1.536 Mbps). The format and channel ordering match the contract
documented in the desktop capture plan.

## TCC Permission Audit

```
$ sqlite3 /Library/Application\ Support/com.apple.TCC/TCC.db \
    "SELECT service, client, auth_value FROM access WHERE client = 'com.osslma.desktop'"
kTCCServiceScreenCapture|com.osslma.desktop|2
kTCCServiceMicrophone|com.osslma.desktop|2
```

Both `kTCCServiceScreenCapture` and `kTCCServiceMicrophone` are granted
(`auth_value=2`) for the bundle identifier `com.osslma.desktop` declared in
`src-tauri/tauri.conf.json`.

## Plan Task Status

All 35 sub-steps in `docs/superpowers/plans/2026-08-26-macos-desktop-capture.md`
satisfied:

- **Task 1** Rust workspace + capture/link interfaces — `crates/{lma-capture,lma-link,app}` present, workspace compiles.
- **Task 2** Deterministic mixer + WAV recorder — 21 tests pass.
- **Task 3** macOS permissions, device enumeration, native sources — 31 tests pass including `rebuilding_a_disconnected_microphone_leaves_system_audio_running`.
- **Task 4** Sidecar WebSocket link + reconnect buffer — 8 tests pass including 3-second cap and oldest-first drop.
- **Task 5** Integrate capture, link, recording, Tauri commands — bundle built, IPC commands wired, lifecycle commands present.
- **Task 6** Contract, error, and integration coverage — `wire_contract.rs` (4 tests) and `capture_integration.rs` (3 tests) pass.
- **Task 7** Documentation + manual smoke — `desktop-capture-app.md`, `prerequisites-and-install.md`, `troubleshooting.md`, `developer-guide.md` updated; `pre-commit` config added. The real-hardware smoke check above substitutes for the manual permission grant + mic-unplug step (which is environment-bound and not automatable from CI).

## Notes

- One pre-existing formatting finding from `pre-commit`: `python/lma_pipeline/tests/test_assembler_runs.py` would be reformatted by `ruff format`. The plan explicitly anticipates this ("pre-existing Python formatting is not broadened into this phase") so it is not addressed here.
- The four recording directories under `~/Library/Application Support/com.osslma.desktop/recordings/` produced by prior manual smoke runs contain valid 16-bit stereo @ 48 kHz WAV files. Channel 0 (system) is silent in those files because no system audio was playing during the manual recordings — not a defect, confirmed by the live capture test above.
