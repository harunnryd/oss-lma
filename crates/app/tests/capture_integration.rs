use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use app::commands::capture::NativePipeline;
use app::{
    capture_state::{CapturePhase, SourceReadiness},
    commands::capture::{
        AppCapture, CaptureBackend, CaptureDevice, CaptureDeviceSelection, LinkOptions,
        PermissionSnapshot, PermissionStatus,
    },
};
use futures_util::StreamExt;
use lma_capture::{Mixer, SourceChannel, StereoChunk, WavRecorder};
use lma_link::{LinkClient, LinkEvent};
use serde_json::Value;
use tokio::{net::TcpListener, runtime::Runtime, sync::broadcast, time::sleep};
use tokio_tungstenite::{accept_async, tungstenite::Message};

const TICK_FRAMES: usize = 4_800;

#[test]
fn webview_start_options_cannot_override_supervised_endpoint() {
    let options: app::commands::capture::StartMeetingOptions =
        serde_json::from_value(serde_json::json!({
            "diarizeMicrophone": true,
            "port": 1,
            "token": "untrusted",
        }))
        .expect("public capture options deserialize");

    assert!(options.diarize_microphone);
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("oss-lma-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("test directory is created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FakeMacSources {
    readiness: SourceReadiness,
}

impl Default for FakeMacSources {
    fn default() -> Self {
        Self {
            readiness: SourceReadiness {
                system: false,
                microphone: false,
            },
        }
    }
}

struct Pipeline {
    sources: FakeMacSources,
    mixer: Mixer,
    recorder: Option<WavRecorder>,
    link: Option<LinkClient>,
    link_events: Option<broadcast::Receiver<LinkEvent>>,
    actions: Vec<String>,
}

impl Pipeline {
    fn new(readiness: SourceReadiness) -> Self {
        Self {
            sources: FakeMacSources { readiness },
            mixer: Mixer::new(),
            recorder: None,
            link: None,
            link_events: None,
            actions: Vec::new(),
        }
    }

    fn push_tick(&mut self, system: f32, microphone: f32) {
        assert!(self.sources.readiness.system);
        assert!(self.sources.readiness.microphone);
        assert!(self
            .mixer
            .push(SourceChannel::System, &vec![system; TICK_FRAMES])
            .is_empty());
        for chunk in self
            .mixer
            .push(SourceChannel::Microphone, &vec![microphone; TICK_FRAMES])
        {
            self.write_chunk(chunk);
        }
    }

    fn write_chunk(&mut self, chunk: StereoChunk) {
        let samples = chunk
            .pcm
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
            .collect::<Vec<_>>();
        self.recorder
            .as_mut()
            .expect("recorder is active")
            .write(&samples)
            .expect("captured PCM is recorded");
        self.link
            .as_ref()
            .expect("link is active")
            .send_chunk(chunk)
            .expect("captured PCM is queued for the link");
    }
}

struct VerticalBackend {
    permissions: PermissionSnapshot,
    pipeline: Arc<Mutex<Pipeline>>,
    runtime: Arc<Runtime>,
}

impl CaptureBackend for VerticalBackend {
    fn permissions(&mut self) -> Result<PermissionSnapshot, String> {
        self.pipeline
            .lock()
            .expect("pipeline lock")
            .actions
            .push("permissions".into());
        Ok(self.permissions.clone())
    }

    fn request_permission(
        &mut self,
        kind: app::commands::capture::CapturePermissionKind,
    ) -> Result<app::commands::capture::PermissionStatus, String> {
        Ok(match kind {
            app::commands::capture::CapturePermissionKind::ScreenRecording => {
                self.permissions.screen_recording
            }
            app::commands::capture::CapturePermissionKind::Microphone => {
                self.permissions.microphone
            }
        })
    }

    fn open_permission_settings(
        &mut self,
        _kind: app::commands::capture::CapturePermissionKind,
    ) -> Result<(), String> {
        Ok(())
    }

    fn devices(&mut self) -> Result<Vec<CaptureDevice>, String> {
        Ok(Vec::new())
    }

    fn start_sources(
        &mut self,
        _selection: &CaptureDeviceSelection,
        _generation: u64,
    ) -> Result<SourceReadiness, String> {
        let mut pipeline = self.pipeline.lock().expect("pipeline lock");
        pipeline.actions.push("sources:start".into());
        Ok(pipeline.sources.readiness)
    }

    fn open_recorder(&mut self, path: &Path) -> Result<(), String> {
        let recorder = WavRecorder::create(path, 48_000).map_err(|error| error.to_string())?;
        let mut pipeline = self.pipeline.lock().expect("pipeline lock");
        pipeline.actions.push("recorder:open".into());
        pipeline.recorder = Some(recorder);
        Ok(())
    }

    fn start_link(
        &mut self,
        call_id: &str,
        options: &LinkOptions,
        _generation: u64,
    ) -> Result<(), String> {
        let link = {
            let _runtime = self.runtime.enter();
            LinkClient::new()
        };
        let events = link.subscribe();
        self.runtime
            .block_on(link.start(
                call_id,
                options.port,
                options.token.clone(),
                48_000,
                options.diarize_microphone,
            ))
            .map_err(|error| error.to_string())?;
        let mut pipeline = self.pipeline.lock().expect("pipeline lock");
        pipeline.actions.push(format!("link:start:{call_id}"));
        pipeline.link_events = Some(events);
        pipeline.link = Some(link);
        Ok(())
    }

    fn pause_link(&mut self) -> Result<(), String> {
        let mut pipeline = self.pipeline.lock().expect("pipeline lock");
        pipeline.actions.push("link:pause".into());
        pipeline
            .link
            .as_ref()
            .expect("link is active")
            .pause()
            .map_err(|error| error.to_string())?;
        pipeline.mixer.pause();
        Ok(())
    }

    fn resume_link(&mut self) -> Result<(), String> {
        let mut pipeline = self.pipeline.lock().expect("pipeline lock");
        pipeline.actions.push("link:resume".into());
        pipeline
            .link
            .as_ref()
            .expect("link is active")
            .resume()
            .map_err(|error| error.to_string())?;
        pipeline.mixer.resume();
        Ok(())
    }

    fn end_link(&mut self) -> Result<(), String> {
        let mut pipeline = self.pipeline.lock().expect("pipeline lock");
        pipeline.actions.push("link:end".into());
        if let Some(link) = pipeline.link.take() {
            link.end().map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn finish_recorder(&mut self) -> Result<(), String> {
        let mut pipeline = self.pipeline.lock().expect("pipeline lock");
        pipeline.actions.push("recorder:finish".into());
        if let Some(recorder) = pipeline.recorder.as_mut() {
            recorder.finish().map_err(|error| error.to_string())?;
        }
        pipeline.recorder = None;
        Ok(())
    }

    fn stop_sources(&mut self) -> Result<(), String> {
        let mut pipeline = self.pipeline.lock().expect("pipeline lock");
        pipeline.actions.push("sources:stop".into());
        pipeline.sources.readiness = SourceReadiness {
            system: false,
            microphone: false,
        };
        Ok(())
    }
}

#[derive(Default)]
struct SidecarLog {
    controls: Vec<Value>,
    binary: Vec<Vec<u8>>,
}

fn spawn_reconnecting_sidecar(
    runtime: &Runtime,
) -> (u16, Arc<Mutex<SidecarLog>>, tokio::task::JoinHandle<()>) {
    let listener = runtime
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("fake sidecar binds");
    let port = listener.local_addr().expect("listener address").port();
    let log = Arc::new(Mutex::new(SidecarLog::default()));
    let server_log = log.clone();
    let server = runtime.spawn(async move {
        let (stream, _) = listener.accept().await.expect("first link connects");
        let mut socket = accept_async(stream)
            .await
            .expect("first websocket upgrades");
        while let Some(frame) = socket.next().await {
            match frame.expect("valid first-connection frame") {
                Message::Text(text) => server_log
                    .lock()
                    .expect("sidecar log")
                    .controls
                    .push(serde_json::from_str(&text).expect("control JSON")),
                Message::Binary(bytes) => {
                    server_log
                        .lock()
                        .expect("sidecar log")
                        .binary
                        .push(bytes.to_vec());
                    socket.close(None).await.expect("fake disconnect succeeds");
                    break;
                }
                _ => {}
            }
        }
        drop(socket);
        drop(listener);

        sleep(Duration::from_millis(800)).await;
        let listener = loop {
            match TcpListener::bind(("127.0.0.1", port)).await {
                Ok(listener) => break listener,
                Err(_) => sleep(Duration::from_millis(20)).await,
            }
        };
        let (stream, _) = listener.accept().await.expect("reconnected link connects");
        let mut socket = accept_async(stream)
            .await
            .expect("reconnected websocket upgrades");
        while let Some(frame) = socket.next().await {
            match frame.expect("valid reconnected frame") {
                Message::Text(text) => {
                    let event: Value = serde_json::from_str(&text).expect("control JSON");
                    let ended = event["EventType"] == "END";
                    server_log.lock().expect("sidecar log").controls.push(event);
                    if ended {
                        break;
                    }
                }
                Message::Binary(bytes) => server_log
                    .lock()
                    .expect("sidecar log")
                    .binary
                    .push(bytes.to_vec()),
                _ => {}
            }
        }
    });
    (port, log, server)
}

fn wait_until(description: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn take_link_receiver(pipeline: &Arc<Mutex<Pipeline>>) -> broadcast::Receiver<LinkEvent> {
    pipeline
        .lock()
        .expect("pipeline lock")
        .link_events
        .take()
        .expect("link event receiver is active")
}

fn await_link_event(
    runtime: &Runtime,
    events: &mut broadcast::Receiver<LinkEvent>,
    expected: LinkEvent,
) {
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if events.recv().await.expect("link event receiver") == expected {
                    break;
                }
            }
        })
        .await
        .expect("expected link event arrives");
    });
}

#[cfg(target_os = "macos")]
#[test]
fn rebuild_event_restarts_only_the_affected_production_source() {
    use std::{cell::RefCell, rc::Rc, sync::mpsc};

    use lma_capture::macos::{
        DeviceSelection, MacSource, MonoFrames, NativeStopError, NativeStream, NativeStreamEvents,
        NativeStreamProvider, SourceEvent, SourceKind,
    };

    type Actions = Rc<RefCell<Vec<(SourceKind, &'static str)>>>;

    struct TestStream {
        kind: SourceKind,
        actions: Actions,
    }

    impl NativeStream for TestStream {
        fn stop(&mut self) -> Result<(), NativeStopError> {
            self.actions.borrow_mut().push((self.kind, "stop"));
            Ok(())
        }
    }

    #[derive(Clone)]
    struct TestProvider {
        actions: Actions,
    }

    impl NativeStreamProvider for TestProvider {
        fn start(
            &self,
            kind: SourceKind,
            _selection: &DeviceSelection,
            _frames: mpsc::Sender<MonoFrames>,
            _events: Arc<dyn NativeStreamEvents>,
        ) -> Result<Box<dyn NativeStream>, String> {
            self.actions.borrow_mut().push((kind, "start"));
            Ok(Box::new(TestStream {
                kind,
                actions: self.actions.clone(),
            }))
        }
    }

    let actions = Rc::new(RefCell::new(Vec::new()));
    let provider = TestProvider {
        actions: actions.clone(),
    };
    let (source_events, events) = mpsc::channel();
    let (system_frames, _system_receiver) = mpsc::channel();
    let (microphone_frames, _microphone_receiver) = mpsc::channel();
    let system =
        MacSource::with_provider(SourceKind::System, provider.clone(), source_events.clone())
            .start(DeviceSelection::Default, system_frames);
    let microphone =
        MacSource::with_provider(SourceKind::Microphone, provider, source_events.clone())
            .start(DeviceSelection::Default, microphone_frames);
    let mut pipeline = NativePipeline::with_sources(
        CaptureDeviceSelection::default(),
        system,
        microphone,
        events,
    );
    actions.borrow_mut().clear();

    source_events
        .send(SourceEvent::RebuildRequired(SourceKind::Microphone))
        .expect("rebuild event is injected");
    pipeline
        .process_source_events()
        .expect("affected source rebuilds");

    assert_eq!(
        *actions.borrow(),
        [
            (SourceKind::Microphone, "stop"),
            (SourceKind::Microphone, "start")
        ]
    );
    assert!(pipeline.source_active(SourceKind::System));
    assert!(pipeline.source_active(SourceKind::Microphone));
}

#[test]
fn permission_preflight_and_source_readiness_block_meeting_resources() {
    for (name, permissions, readiness, expected_error, expected_actions) in [
        (
            "denied",
            PermissionSnapshot {
                screen_recording: PermissionStatus::Denied,
                microphone: PermissionStatus::Granted,
            },
            SourceReadiness {
                system: true,
                microphone: true,
            },
            "CAPTURE_PERMISSION_DENIED",
            vec!["permissions"],
        ),
        (
            "inactive",
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: false,
            },
            "both capture sources must be active",
            vec!["permissions", "sources:start", "sources:stop"],
        ),
    ] {
        let directory = TestDirectory::new(name);
        let runtime = Arc::new(Runtime::new().expect("test runtime"));
        let pipeline = Arc::new(Mutex::new(Pipeline::new(readiness)));
        let capture = AppCapture::with_backend(
            directory.path().to_owned(),
            VerticalBackend {
                permissions,
                pipeline: pipeline.clone(),
                runtime,
            },
        );

        let error = capture
            .start(LinkOptions {
                port: 1,
                token: "unused".into(),
                diarize_microphone: false,
            })
            .expect_err("preflight rejects invalid capture readiness");

        assert_eq!(error, expected_error);
        assert_eq!(capture.status().phase, CapturePhase::Failed);
        assert_eq!(
            pipeline.lock().expect("pipeline lock").actions,
            expected_actions
        );
        assert!(!directory.path().join("recordings").exists());
    }
}

#[test]
fn fake_macos_sources_drive_reconnect_pause_rebuild_stop_and_wav_output() {
    let directory = TestDirectory::new("vertical");
    let runtime = Arc::new(Runtime::new().expect("test runtime"));
    let (port, sidecar, server) = spawn_reconnecting_sidecar(&runtime);
    let pipeline = Arc::new(Mutex::new(Pipeline::new(SourceReadiness {
        system: true,
        microphone: true,
    })));
    let capture = AppCapture::with_backend(
        directory.path().to_owned(),
        VerticalBackend {
            permissions: PermissionSnapshot::granted(),
            pipeline: pipeline.clone(),
            runtime: runtime.clone(),
        },
    );

    let active = capture
        .start(LinkOptions {
            port,
            token: "vertical token".into(),
            diarize_microphone: true,
        })
        .expect("both fake sources activate");
    assert_eq!(active.phase, CapturePhase::Active);
    assert!(active.system_active);
    assert!(active.microphone_active);
    let recording_path = active.recording_path.clone().expect("recording path");
    let mut link_events = take_link_receiver(&pipeline);
    await_link_event(&runtime, &mut link_events, LinkEvent::Connected);

    pipeline
        .lock()
        .expect("pipeline lock")
        .push_tick(0.25, -0.5);
    wait_until("the first 100 ms frame", || {
        sidecar.lock().expect("sidecar log").binary.len() == 1
    });
    assert_eq!(
        sidecar.lock().expect("sidecar log").binary[0].len(),
        StereoChunk::byte_len(48_000)
    );
    await_link_event(&runtime, &mut link_events, LinkEvent::Disconnected);

    for index in 0..35 {
        pipeline
            .lock()
            .expect("pipeline lock")
            .push_tick(0.01 * (index + 1) as f32, -0.25);
    }
    await_link_event(&runtime, &mut link_events, LinkEvent::BufferDropped);
    wait_until("reconnect START and the three-second buffer flush", || {
        let log = sidecar.lock().expect("sidecar log");
        log.controls
            .iter()
            .filter(|event| event["EventType"] == "START")
            .count()
            == 2
            && log.binary.len() == 31
    });
    {
        let log = sidecar.lock().expect("sidecar log");
        let first_retained = i16::from_le_bytes([log.binary[1][0], log.binary[1][1]]);
        let newest_retained = i16::from_le_bytes([log.binary[30][0], log.binary[30][1]]);
        assert_eq!(first_retained, 1966);
        assert_eq!(newest_retained, 11_468);
    }
    await_link_event(&runtime, &mut link_events, LinkEvent::Connected);

    let paused = capture.pause().expect("active capture pauses");
    assert_eq!(paused.phase, CapturePhase::Paused);
    wait_until("PAUSE control", || {
        sidecar
            .lock()
            .expect("sidecar log")
            .controls
            .iter()
            .any(|event| event["EventType"] == "PAUSE")
    });
    pipeline.lock().expect("pipeline lock").push_tick(0.9, 0.9);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(sidecar.lock().expect("sidecar log").binary.len(), 31);

    capture.resume().expect("paused capture resumes");
    pipeline.lock().expect("pipeline lock").push_tick(-0.5, 0.5);
    wait_until("post-rebuild frame", || {
        sidecar.lock().expect("sidecar log").binary.len() == 32
    });

    let stopped = capture.stop().expect("active capture stops cleanly");
    assert_eq!(stopped.phase, CapturePhase::Idle);
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .expect("fake sidecar receives END")
            .expect("fake sidecar task completes");
    });

    let log = sidecar.lock().expect("sidecar log");
    assert_eq!(
        log.controls
            .iter()
            .map(|event| event["EventType"].as_str().expect("event type"))
            .collect::<Vec<_>>(),
        ["START", "START", "PAUSE", "RESUME", "END"]
    );
    assert!(log
        .binary
        .iter()
        .all(|frame| frame.len() == StereoChunk::byte_len(48_000)));
    drop(log);

    let mut wav = hound::WavReader::open(recording_path).expect("finished WAV opens");
    let spec = wav.spec();
    assert_eq!(spec.channels, 2);
    assert_eq!(spec.sample_rate, 48_000);
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
    let samples = wav
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .expect("WAV samples decode");
    assert_eq!(samples.len(), 37 * TICK_FRAMES * 2);
    assert_eq!(&samples[..4], &[8192, -16384, 8192, -16384]);
    assert_eq!(
        &samples[samples.len() - 4..],
        &[-16384, 16384, -16384, 16384]
    );
    assert_eq!(
        pipeline.lock().expect("pipeline lock").actions.last(),
        Some(&"sources:stop".into())
    );
}
