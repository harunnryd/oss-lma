use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, Runtime};

use crate::capture_state::{CaptureSnapshot, CaptureState, SourceReadiness};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionStatus {
    Unknown,
    Denied,
    Granted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSnapshot {
    pub screen_recording: PermissionStatus,
    pub microphone: PermissionStatus,
}

impl PermissionSnapshot {
    pub fn granted() -> Self {
        Self {
            screen_recording: PermissionStatus::Granted,
            microphone: PermissionStatus::Granted,
        }
    }

    fn has_denial(&self) -> bool {
        self.screen_recording == PermissionStatus::Denied
            || self.microphone == PermissionStatus::Denied
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureDeviceKind {
    SystemOutput,
    Microphone,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub kind: CaptureDeviceKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDeviceSelection {
    pub system_output_id: Option<String>,
    pub microphone_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkOptions {
    pub port: u16,
    pub token: String,
    #[serde(default)]
    pub diarize_microphone: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelMeters {
    pub system: f32,
    pub microphone: f32,
}

pub trait CaptureBackend: Send {
    fn permissions(&mut self) -> Result<PermissionSnapshot, String>;
    fn devices(&mut self) -> Result<Vec<CaptureDevice>, String>;
    fn start_sources(
        &mut self,
        selection: &CaptureDeviceSelection,
        generation: u64,
    ) -> Result<SourceReadiness, String>;
    fn open_recorder(&mut self, path: &Path) -> Result<(), String>;
    fn start_link(
        &mut self,
        call_id: &str,
        options: &LinkOptions,
        generation: u64,
    ) -> Result<(), String>;
    fn pause_link(&mut self) -> Result<(), String>;
    fn resume_link(&mut self) -> Result<(), String>;
    fn end_link(&mut self) -> Result<(), String>;
    fn finish_recorder(&mut self) -> Result<(), String>;
    fn stop_sources(&mut self) -> Result<(), String>;
}

type SnapshotEmitter = Arc<dyn Fn(&CaptureSnapshot) + Send + Sync>;

pub const CAPTURE_STATUS_EVENT: &str = "capture-status";
pub const CAPTURE_LEVELS_EVENT: &str = "capture-levels";

pub struct AppCapture {
    app_data_dir: PathBuf,
    backend: Mutex<Box<dyn CaptureBackend>>,
    operation: Mutex<()>,
    selection: Mutex<CaptureDeviceSelection>,
    state: Arc<Mutex<CaptureState>>,
    emit_snapshot: SnapshotEmitter,
}

impl AppCapture {
    pub fn from_tauri<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Self, String> {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        let state = Arc::new(Mutex::new(CaptureState::default()));

        let status_app = app.clone();
        let emit_snapshot: SnapshotEmitter = Arc::new(move |snapshot| {
            let _ = status_app.emit(CAPTURE_STATUS_EVENT, snapshot);
        });
        let level_app = app.clone();
        let emit_levels: LevelEmitter = Arc::new(move |levels| {
            let _ = level_app.emit(CAPTURE_LEVELS_EVENT, levels);
        });
        let failed_state = state.clone();
        let failed_emitter = emit_snapshot.clone();
        let emit_failure: FailureEmitter = Arc::new(move |generation, error| {
            let snapshot = {
                let mut state = failed_state.lock().unwrap();
                state
                    .fail_if_current(generation, error)
                    .then(|| state.snapshot())
            };
            if let Some(snapshot) = snapshot {
                failed_emitter(&snapshot);
            }
        });

        let backend = NativeCaptureBackend::new(emit_levels, emit_failure)?;
        Ok(Self {
            app_data_dir,
            backend: Mutex::new(Box::new(backend)),
            operation: Mutex::new(()),
            selection: Mutex::new(CaptureDeviceSelection::default()),
            state,
            emit_snapshot,
        })
    }

    pub fn with_backend(app_data_dir: PathBuf, backend: impl CaptureBackend + 'static) -> Self {
        Self::with_backend_and_emitter(app_data_dir, backend, Arc::new(|_| {}))
    }

    pub fn with_backend_and_emitter(
        app_data_dir: PathBuf,
        backend: impl CaptureBackend + 'static,
        emit_snapshot: SnapshotEmitter,
    ) -> Self {
        Self {
            app_data_dir,
            backend: Mutex::new(Box::new(backend)),
            operation: Mutex::new(()),
            selection: Mutex::new(CaptureDeviceSelection::default()),
            state: Arc::new(Mutex::new(CaptureState::default())),
            emit_snapshot,
        }
    }

    pub fn permissions(&self) -> Result<PermissionSnapshot, String> {
        self.backend.lock().unwrap().permissions()
    }

    pub fn devices(&self) -> Result<Vec<CaptureDevice>, String> {
        self.backend.lock().unwrap().devices()
    }

    pub fn set_devices(
        &self,
        selection: CaptureDeviceSelection,
    ) -> Result<CaptureDeviceSelection, String> {
        let _operation = self.operation.lock().unwrap();
        if !matches!(
            self.status().phase,
            crate::capture_state::CapturePhase::Idle | crate::capture_state::CapturePhase::Failed
        ) {
            return Err("capture devices cannot change during a meeting".to_owned());
        }
        let devices = self.backend.lock().unwrap().devices()?;
        validate_selection(&devices, &selection)?;
        *self.selection.lock().unwrap() = selection.clone();
        Ok(selection)
    }

    pub fn start(&self, options: LinkOptions) -> Result<CaptureSnapshot, String> {
        let _operation = self.operation.lock().unwrap();
        self.update_state(|state| state.begin_preflight().map(|_| ()))?;
        let generation = self.state.lock().unwrap().generation();

        let permissions = match self.backend.lock().unwrap().permissions() {
            Ok(permissions) => permissions,
            Err(error) => return self.fail(error),
        };
        if permissions.has_denial() {
            return self.fail("capture permissions are not granted");
        }

        self.update_state(|state| state.begin_starting())?;
        let selection = self.selection.lock().unwrap().clone();
        let readiness = match self
            .backend
            .lock()
            .unwrap()
            .start_sources(&selection, generation)
        {
            Ok(readiness) => readiness,
            Err(error) => return self.fail(error),
        };
        if !readiness.both_active() {
            let _ = self.backend.lock().unwrap().stop_sources();
            return self.fail("both capture sources must be active");
        }
        if let Some(error) = self.startup_error(generation) {
            let _ = self.backend.lock().unwrap().stop_sources();
            return Err(error);
        }

        let meeting_id = uuid::Uuid::new_v4().to_string();
        let meeting_dir = self.app_data_dir.join("recordings").join(&meeting_id);
        if let Err(error) = fs::create_dir_all(&meeting_dir) {
            let _ = self.backend.lock().unwrap().stop_sources();
            return self.fail(format!("recording directory could not be created: {error}"));
        }
        let recording_path = meeting_dir.join("audio.wav");

        let recorder = self.backend.lock().unwrap().open_recorder(&recording_path);
        if let Err(error) = recorder {
            let _ = self.backend.lock().unwrap().stop_sources();
            return self.fail(error);
        }
        if let Some(error) = self.startup_error(generation) {
            let mut backend = self.backend.lock().unwrap();
            let _ = backend.finish_recorder();
            let _ = backend.stop_sources();
            return Err(error);
        }
        let link = self
            .backend
            .lock()
            .unwrap()
            .start_link(&meeting_id, &options, generation);
        if let Err(error) = link {
            let _ = self.backend.lock().unwrap().finish_recorder();
            let _ = self.backend.lock().unwrap().stop_sources();
            return self.fail(error);
        }

        match self.update_state(|state| {
            state.activate_if_current(generation, readiness, meeting_id, recording_path)
        }) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let mut backend = self.backend.lock().unwrap();
                let _ = backend.end_link();
                let _ = backend.finish_recorder();
                let _ = backend.stop_sources();
                Err(self.startup_error(generation).unwrap_or(error))
            }
        }
    }

    pub fn pause(&self) -> Result<CaptureSnapshot, String> {
        let _operation = self.operation.lock().unwrap();
        if self.status().phase != crate::capture_state::CapturePhase::Active {
            return Err("capture can only pause an active meeting".to_owned());
        }
        let result = self.backend.lock().unwrap().pause_link();
        if let Err(error) = result {
            self.close_backend();
            return self.fail(error);
        }
        self.update_state(|state| state.pause())
    }

    pub fn resume(&self) -> Result<CaptureSnapshot, String> {
        let _operation = self.operation.lock().unwrap();
        if self.status().phase != crate::capture_state::CapturePhase::Paused {
            return Err("capture can only resume a paused meeting".to_owned());
        }
        let result = self.backend.lock().unwrap().resume_link();
        if let Err(error) = result {
            self.close_backend();
            return self.fail(error);
        }
        self.update_state(|state| state.resume())
    }

    pub fn stop(&self) -> Result<CaptureSnapshot, String> {
        let _operation = self.operation.lock().unwrap();
        self.update_state(|state| state.begin_stopping())?;

        let mut backend = self.backend.lock().unwrap();
        let end = backend.end_link();
        let finish = backend.finish_recorder();
        let stop = backend.stop_sources();
        drop(backend);

        if let Some(error) = [end.err(), finish.err(), stop.err()]
            .into_iter()
            .flatten()
            .next()
        {
            return self.fail(error);
        }
        self.update_state(|state| state.finish_stopping())
    }

    pub fn status(&self) -> CaptureSnapshot {
        self.state.lock().unwrap().snapshot()
    }

    fn fail<T>(&self, error: impl Into<String>) -> Result<T, String> {
        let error = error.into();
        let snapshot = {
            let mut state = self.state.lock().unwrap();
            state.fail(error.clone());
            state.snapshot()
        };
        (self.emit_snapshot)(&snapshot);
        Err(error)
    }

    fn update_state(
        &self,
        update: impl FnOnce(&mut CaptureState) -> Result<(), String>,
    ) -> Result<CaptureSnapshot, String> {
        let snapshot = {
            let mut state = self.state.lock().unwrap();
            update(&mut state)?;
            state.snapshot()
        };
        (self.emit_snapshot)(&snapshot);
        Ok(snapshot)
    }

    fn startup_error(&self, generation: u64) -> Option<String> {
        let state = self.state.lock().unwrap();
        let snapshot = state.snapshot();
        (state.generation() != generation
            || snapshot.phase != crate::capture_state::CapturePhase::Starting)
            .then(|| {
                snapshot
                    .error
                    .unwrap_or_else(|| "capture start was interrupted".to_owned())
            })
    }

    fn close_backend(&self) {
        let mut backend = self.backend.lock().unwrap();
        let _ = backend.end_link();
        let _ = backend.finish_recorder();
        let _ = backend.stop_sources();
    }
}

type LevelEmitter = Arc<dyn Fn(LevelMeters) + Send + Sync>;
type FailureEmitter = Arc<dyn Fn(u64, String) + Send + Sync>;

#[cfg(target_os = "macos")]
struct NativeCaptureBackend {
    commands: std::sync::mpsc::Sender<NativeCommand>,
}

#[cfg(target_os = "macos")]
enum NativeCommand {
    Permissions(std::sync::mpsc::SyncSender<Result<PermissionSnapshot, String>>),
    Devices(std::sync::mpsc::SyncSender<Result<Vec<CaptureDevice>, String>>),
    StartSources(
        CaptureDeviceSelection,
        u64,
        std::sync::mpsc::SyncSender<Result<SourceReadiness, String>>,
    ),
    OpenRecorder(PathBuf, std::sync::mpsc::SyncSender<Result<(), String>>),
    StartLink(
        String,
        LinkOptions,
        u64,
        std::sync::mpsc::SyncSender<Result<(), String>>,
    ),
    Pause(std::sync::mpsc::SyncSender<Result<(), String>>),
    Resume(std::sync::mpsc::SyncSender<Result<(), String>>),
    End(std::sync::mpsc::SyncSender<Result<(), String>>),
    FinishRecorder(std::sync::mpsc::SyncSender<Result<(), String>>),
    StopSources(std::sync::mpsc::SyncSender<Result<(), String>>),
    Shutdown,
}

#[cfg(target_os = "macos")]
impl NativeCaptureBackend {
    fn new(emit_levels: LevelEmitter, emit_failure: FailureEmitter) -> Result<Self, String> {
        let (commands, receiver) = std::sync::mpsc::channel();
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("lma-capture".to_owned())
            .spawn(move || NativeWorker::new(receiver, emit_levels, emit_failure).run(ready))
            .map_err(|error| format!("capture worker could not start: {error}"))?;
        started
            .recv()
            .map_err(|_| "capture worker stopped during startup".to_owned())??;
        Ok(Self { commands })
    }

    fn request<T>(
        &self,
        command: impl FnOnce(std::sync::mpsc::SyncSender<Result<T, String>>) -> NativeCommand,
    ) -> Result<T, String> {
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        self.commands
            .send(command(reply))
            .map_err(|_| "capture worker is unavailable".to_owned())?;
        result
            .recv()
            .map_err(|_| "capture worker stopped before replying".to_owned())?
    }
}

#[cfg(target_os = "macos")]
impl Drop for NativeCaptureBackend {
    fn drop(&mut self) {
        let _ = self.commands.send(NativeCommand::Shutdown);
    }
}

#[cfg(target_os = "macos")]
impl CaptureBackend for NativeCaptureBackend {
    fn permissions(&mut self) -> Result<PermissionSnapshot, String> {
        self.request(NativeCommand::Permissions)
    }

    fn devices(&mut self) -> Result<Vec<CaptureDevice>, String> {
        self.request(NativeCommand::Devices)
    }

    fn start_sources(
        &mut self,
        selection: &CaptureDeviceSelection,
        generation: u64,
    ) -> Result<SourceReadiness, String> {
        self.request(|reply| NativeCommand::StartSources(selection.clone(), generation, reply))
    }

    fn open_recorder(&mut self, path: &Path) -> Result<(), String> {
        self.request(|reply| NativeCommand::OpenRecorder(path.to_owned(), reply))
    }

    fn start_link(
        &mut self,
        call_id: &str,
        options: &LinkOptions,
        generation: u64,
    ) -> Result<(), String> {
        self.request(|reply| {
            NativeCommand::StartLink(call_id.to_owned(), options.clone(), generation, reply)
        })
    }

    fn pause_link(&mut self) -> Result<(), String> {
        self.request(NativeCommand::Pause)
    }

    fn resume_link(&mut self) -> Result<(), String> {
        self.request(NativeCommand::Resume)
    }

    fn end_link(&mut self) -> Result<(), String> {
        self.request(NativeCommand::End)
    }

    fn finish_recorder(&mut self) -> Result<(), String> {
        self.request(NativeCommand::FinishRecorder)
    }

    fn stop_sources(&mut self) -> Result<(), String> {
        self.request(NativeCommand::StopSources)
    }
}

#[cfg(target_os = "macos")]
struct NativeWorker {
    commands: std::sync::mpsc::Receiver<NativeCommand>,
    emit_levels: LevelEmitter,
    emit_failure: FailureEmitter,
    runtime: Option<tokio::runtime::Runtime>,
    system: Option<lma_capture::macos::SourceHandle>,
    microphone: Option<lma_capture::macos::SourceHandle>,
    system_frames: Option<std::sync::mpsc::Receiver<lma_capture::macos::MonoFrames>>,
    microphone_frames: Option<std::sync::mpsc::Receiver<lma_capture::macos::MonoFrames>>,
    source_events: Option<std::sync::mpsc::Receiver<lma_capture::macos::SourceEvent>>,
    active_generation: Option<u64>,
    selection: CaptureDeviceSelection,
    mixer: lma_capture::Mixer,
    recorder: Option<lma_capture::WavRecorder>,
    link: Option<lma_link::LinkClient>,
    levels: LevelMeters,
}

#[cfg(target_os = "macos")]
impl NativeWorker {
    fn new(
        commands: std::sync::mpsc::Receiver<NativeCommand>,
        emit_levels: LevelEmitter,
        emit_failure: FailureEmitter,
    ) -> Self {
        Self {
            commands,
            emit_levels,
            emit_failure,
            runtime: None,
            system: None,
            microphone: None,
            system_frames: None,
            microphone_frames: None,
            source_events: None,
            active_generation: None,
            selection: CaptureDeviceSelection::default(),
            mixer: lma_capture::Mixer::new(),
            recorder: None,
            link: None,
            levels: LevelMeters {
                system: 0.0,
                microphone: 0.0,
            },
        }
    }

    fn run(mut self, ready: std::sync::mpsc::SyncSender<Result<(), String>>) {
        match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => {
                self.runtime = Some(runtime);
                let _ = ready.send(Ok(()));
            }
            Err(error) => {
                let _ = ready.send(Err(format!("capture runtime could not start: {error}")));
                return;
            }
        }

        loop {
            match self
                .commands
                .recv_timeout(std::time::Duration::from_millis(5))
            {
                Ok(NativeCommand::Shutdown)
                | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.close_session();
                    return;
                }
                Ok(command) => self.handle(command),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
            self.process_source_events();
            self.process_audio();
        }
    }

    fn handle(&mut self, command: NativeCommand) {
        match command {
            NativeCommand::Permissions(reply) => {
                let _ = reply.send(Ok(PermissionSnapshot {
                    screen_recording: permission_status(
                        lma_capture::macos::MacPermissions::screen_recording().status(),
                    ),
                    microphone: permission_status(
                        lma_capture::macos::MacPermissions::microphone().status(),
                    ),
                }));
            }
            NativeCommand::Devices(reply) => {
                let devices = lma_capture::macos::MacDevices::new()
                    .list()
                    .into_iter()
                    .map(capture_device)
                    .collect();
                let _ = reply.send(Ok(devices));
            }
            NativeCommand::StartSources(selection, generation, reply) => {
                let _ = reply.send(self.start_sources(selection, generation));
            }
            NativeCommand::OpenRecorder(path, reply) => {
                let result = lma_capture::WavRecorder::create(path, 48_000)
                    .map(|recorder| self.recorder = Some(recorder))
                    .map_err(|error| error.to_string());
                let _ = reply.send(result);
            }
            NativeCommand::StartLink(call_id, options, generation, reply) => {
                if self.active_generation != Some(generation) {
                    let _ = reply.send(Err("capture start was superseded".to_owned()));
                    return;
                }
                let runtime = self.runtime.as_ref().expect("runtime is initialized");
                let link = runtime.block_on(async {
                    let link = lma_link::LinkClient::new();
                    link.start(
                        call_id,
                        options.port,
                        options.token,
                        48_000,
                        options.diarize_microphone,
                    )
                    .await?;
                    Ok::<_, lma_link::LinkError>(link)
                });
                let result = link
                    .map(|link| self.link = Some(link))
                    .map_err(|error| error.to_string());
                let _ = reply.send(result);
            }
            NativeCommand::Pause(reply) => {
                let result = self
                    .link
                    .as_ref()
                    .ok_or_else(|| "meeting link is not active".to_owned())
                    .and_then(|link| link.pause().map_err(|error| error.to_string()));
                if result.is_ok() {
                    self.mixer.pause();
                }
                let _ = reply.send(result);
            }
            NativeCommand::Resume(reply) => {
                let result = self
                    .link
                    .as_ref()
                    .ok_or_else(|| "meeting link is not active".to_owned())
                    .and_then(|link| link.resume().map_err(|error| error.to_string()));
                if result.is_ok() {
                    self.mixer.resume();
                }
                let _ = reply.send(result);
            }
            NativeCommand::End(reply) => {
                let result = self
                    .link
                    .take()
                    .map(|link| link.end().map_err(|error| error.to_string()))
                    .unwrap_or(Ok(()));
                self.mixer.pause();
                let _ = reply.send(result);
            }
            NativeCommand::FinishRecorder(reply) => {
                let result = self
                    .recorder
                    .as_mut()
                    .map(lma_capture::WavRecorder::finish)
                    .unwrap_or(Ok(()))
                    .map_err(|error| error.to_string());
                self.recorder = None;
                let _ = reply.send(result);
            }
            NativeCommand::StopSources(reply) => {
                let _ = reply.send(self.stop_sources());
            }
            NativeCommand::Shutdown => unreachable!(),
        }
    }

    fn start_sources(
        &mut self,
        selection: CaptureDeviceSelection,
        generation: u64,
    ) -> Result<SourceReadiness, String> {
        self.stop_sources()?;
        let (events, source_events) = std::sync::mpsc::channel();
        let (system_frames, system_receiver) = std::sync::mpsc::channel();
        let (microphone_frames, microphone_receiver) = std::sync::mpsc::channel();
        let system = lma_capture::macos::MacSource::system(events.clone())
            .start(native_selection(&selection.system_output_id), system_frames);
        let microphone = lma_capture::macos::MacSource::microphone(events).start(
            native_selection(&selection.microphone_id),
            microphone_frames,
        );
        let readiness = SourceReadiness {
            system: system.is_active(),
            microphone: microphone.is_active(),
        };
        if !readiness.both_active() {
            return Ok(readiness);
        }
        self.selection = selection;
        self.system = Some(system);
        self.microphone = Some(microphone);
        self.system_frames = Some(system_receiver);
        self.microphone_frames = Some(microphone_receiver);
        self.source_events = Some(source_events);
        self.active_generation = Some(generation);
        Ok(readiness)
    }

    fn stop_sources(&mut self) -> Result<(), String> {
        let system = self.system.as_mut().map(|source| source.stop());
        let microphone = self.microphone.as_mut().map(|source| source.stop());
        self.system = None;
        self.microphone = None;
        self.system_frames = None;
        self.microphone_frames = None;
        self.source_events = None;
        self.active_generation = None;
        self.mixer = lma_capture::Mixer::new();
        system
            .into_iter()
            .chain(microphone)
            .find_map(Result::err)
            .map_or(Ok(()), Err)
    }

    fn process_source_events(&mut self) {
        let events = self
            .source_events
            .as_ref()
            .map(|events| events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            match event {
                lma_capture::macos::SourceEvent::RebuildRequired(kind) => {
                    let result = match kind {
                        lma_capture::macos::SourceKind::System => {
                            self.system.as_mut().map(|source| {
                                source.rebuild(native_selection(&self.selection.system_output_id))
                            })
                        }
                        lma_capture::macos::SourceKind::Microphone => {
                            self.microphone.as_mut().map(|source| {
                                source.rebuild(native_selection(&self.selection.microphone_id))
                            })
                        }
                    };
                    if let Some(Err(error)) = result {
                        self.fail_session(error);
                        break;
                    }
                }
                lma_capture::macos::SourceEvent::Error(_, error) => {
                    self.fail_session(error);
                    break;
                }
                lma_capture::macos::SourceEvent::Started(_)
                | lma_capture::macos::SourceEvent::Stopped(_) => {}
            }
        }
    }

    fn process_audio(&mut self) {
        let system = self
            .system_frames
            .as_ref()
            .map(|frames| frames.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        let microphone = self
            .microphone_frames
            .as_ref()
            .map(|frames| frames.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for frames in system {
            self.levels.system = frames.level;
            (self.emit_levels)(self.levels.clone());
            self.push_frames(lma_capture::SourceChannel::System, &frames.samples);
        }
        for frames in microphone {
            self.levels.microphone = frames.level;
            (self.emit_levels)(self.levels.clone());
            self.push_frames(lma_capture::SourceChannel::Microphone, &frames.samples);
        }
    }

    fn push_frames(&mut self, channel: lma_capture::SourceChannel, frames: &[f32]) {
        for chunk in self.mixer.push(channel, frames) {
            if let Err(error) = self.write_chunk(&chunk) {
                self.fail_session(error);
                break;
            }
        }
    }

    fn write_chunk(&mut self, chunk: &lma_capture::StereoChunk) -> Result<(), String> {
        if let Some(recorder) = self.recorder.as_mut() {
            let samples = chunk
                .pcm
                .chunks_exact(2)
                .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
                .collect::<Vec<_>>();
            recorder
                .write(&samples)
                .map_err(|error| error.to_string())?;
        }
        if let Some(link) = self.link.as_ref() {
            link.send_chunk(chunk.clone())
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn fail_session(&mut self, error: String) {
        let generation = self.active_generation;
        self.close_session();
        if let Some(generation) = generation {
            (self.emit_failure)(generation, error);
        }
    }

    fn close_session(&mut self) {
        if let Some(link) = self.link.take() {
            let _ = link.end();
        }
        if let Some(recorder) = self.recorder.as_mut() {
            let _ = recorder.finish();
        }
        self.recorder = None;
        let _ = self.stop_sources();
    }
}

#[cfg(target_os = "macos")]
fn permission_status(state: lma_capture::PermissionState) -> PermissionStatus {
    match state {
        lma_capture::PermissionState::Unknown => PermissionStatus::Unknown,
        lma_capture::PermissionState::Denied => PermissionStatus::Denied,
        lma_capture::PermissionState::Granted => PermissionStatus::Granted,
    }
}

#[cfg(target_os = "macos")]
fn capture_device(device: lma_capture::DeviceInfo) -> CaptureDevice {
    CaptureDevice {
        id: device.id,
        name: device.name,
        is_default: device.is_default,
        kind: match device.kind {
            lma_capture::DeviceKind::SystemOutput => CaptureDeviceKind::SystemOutput,
            lma_capture::DeviceKind::Microphone => CaptureDeviceKind::Microphone,
        },
    }
}

#[cfg(target_os = "macos")]
fn native_selection(id: &Option<String>) -> lma_capture::macos::DeviceSelection {
    id.as_ref()
        .map(|id| lma_capture::macos::DeviceSelection::DeviceId(id.clone()))
        .unwrap_or(lma_capture::macos::DeviceSelection::Default)
}

#[cfg(not(target_os = "macos"))]
struct NativeCaptureBackend;

#[cfg(not(target_os = "macos"))]
impl NativeCaptureBackend {
    fn new(_emit_levels: LevelEmitter, _emit_failure: FailureEmitter) -> Result<Self, String> {
        Ok(Self)
    }
}

#[cfg(not(target_os = "macos"))]
impl CaptureBackend for NativeCaptureBackend {
    fn permissions(&mut self) -> Result<PermissionSnapshot, String> {
        Err("native capture is supported on macOS only".to_owned())
    }

    fn devices(&mut self) -> Result<Vec<CaptureDevice>, String> {
        Err("native capture is supported on macOS only".to_owned())
    }

    fn start_sources(
        &mut self,
        _selection: &CaptureDeviceSelection,
        _generation: u64,
    ) -> Result<SourceReadiness, String> {
        Err("native capture is supported on macOS only".to_owned())
    }

    fn open_recorder(&mut self, _path: &Path) -> Result<(), String> {
        Err("native capture is supported on macOS only".to_owned())
    }

    fn start_link(
        &mut self,
        _call_id: &str,
        _options: &LinkOptions,
        _generation: u64,
    ) -> Result<(), String> {
        Err("native capture is supported on macOS only".to_owned())
    }

    fn pause_link(&mut self) -> Result<(), String> {
        Err("native capture is supported on macOS only".to_owned())
    }

    fn resume_link(&mut self) -> Result<(), String> {
        Err("native capture is supported on macOS only".to_owned())
    }

    fn end_link(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn finish_recorder(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn stop_sources(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn validate_selection(
    devices: &[CaptureDevice],
    selection: &CaptureDeviceSelection,
) -> Result<(), String> {
    if selection.system_output_id.is_some() {
        return Err("system audio capture supports the default output only".to_owned());
    }
    for (id, kind) in [
        (
            selection.system_output_id.as_deref(),
            CaptureDeviceKind::SystemOutput,
        ),
        (
            selection.microphone_id.as_deref(),
            CaptureDeviceKind::Microphone,
        ),
    ] {
        if let Some(id) = id {
            if !devices
                .iter()
                .any(|device| device.id == id && device.kind == kind)
            {
                return Err(format!("selected capture device is unavailable: {id}"));
            }
        }
    }
    Ok(())
}

#[tauri::command(async)]
pub fn capture_permissions(
    state: tauri::State<'_, AppCapture>,
) -> Result<PermissionSnapshot, String> {
    state.permissions()
}

#[tauri::command(async)]
pub fn capture_devices(state: tauri::State<'_, AppCapture>) -> Result<Vec<CaptureDevice>, String> {
    state.devices()
}

#[tauri::command(async)]
pub fn set_capture_devices(
    selection: CaptureDeviceSelection,
    state: tauri::State<'_, AppCapture>,
) -> Result<CaptureDeviceSelection, String> {
    state.set_devices(selection)
}

#[tauri::command(async)]
pub fn start_meeting(
    options: LinkOptions,
    state: tauri::State<'_, AppCapture>,
) -> Result<CaptureSnapshot, String> {
    state.start(options)
}

#[tauri::command(async)]
pub fn pause_meeting(state: tauri::State<'_, AppCapture>) -> Result<CaptureSnapshot, String> {
    state.pause()
}

#[tauri::command(async)]
pub fn resume_meeting(state: tauri::State<'_, AppCapture>) -> Result<CaptureSnapshot, String> {
    state.resume()
}

#[tauri::command(async)]
pub fn stop_meeting(state: tauri::State<'_, AppCapture>) -> Result<CaptureSnapshot, String> {
    state.stop()
}

#[tauri::command]
pub fn capture_status(state: tauri::State<'_, AppCapture>) -> CaptureSnapshot {
    state.status()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use crate::capture_state::{CapturePhase, CaptureState, SourceReadiness};

    use super::{
        AppCapture, CaptureBackend, CaptureDeviceSelection, LinkOptions, PermissionSnapshot,
    };

    impl LinkOptions {
        fn test_default() -> Self {
            Self {
                port: 8765,
                token: "secret".into(),
                diarize_microphone: false,
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FailurePoint {
        StartSources,
        StartLink,
    }

    struct FakeBackend {
        permissions: PermissionSnapshot,
        readiness: SourceReadiness,
        actions: Arc<Mutex<Vec<String>>>,
        state: Arc<Mutex<CaptureState>>,
        failure_point: Option<FailurePoint>,
        pause_error: Option<String>,
        resume_error: Option<String>,
        devices: Vec<super::CaptureDevice>,
    }

    impl CaptureBackend for FakeBackend {
        fn permissions(&mut self) -> Result<PermissionSnapshot, String> {
            Ok(self.permissions.clone())
        }

        fn devices(&mut self) -> Result<Vec<super::CaptureDevice>, String> {
            Ok(self.devices.clone())
        }

        fn start_sources(
            &mut self,
            _selection: &CaptureDeviceSelection,
            generation: u64,
        ) -> Result<SourceReadiness, String> {
            self.actions.lock().unwrap().push("sources:start".into());
            if self.failure_point == Some(FailurePoint::StartSources) {
                self.state
                    .lock()
                    .unwrap()
                    .fail_if_current(generation, "source failed during startup");
            }
            Ok(self.readiness)
        }

        fn open_recorder(&mut self, path: &std::path::Path) -> Result<(), String> {
            self.actions
                .lock()
                .unwrap()
                .push(format!("recorder:open:{}", path.display()));
            Ok(())
        }

        fn start_link(
            &mut self,
            call_id: &str,
            _options: &LinkOptions,
            generation: u64,
        ) -> Result<(), String> {
            self.actions
                .lock()
                .unwrap()
                .push(format!("link:start:{call_id}"));
            if self.failure_point == Some(FailurePoint::StartLink) {
                self.state
                    .lock()
                    .unwrap()
                    .fail_if_current(generation, "source failed before activation");
            }
            Ok(())
        }

        fn pause_link(&mut self) -> Result<(), String> {
            self.actions.lock().unwrap().push("link:pause".into());
            self.pause_error.clone().map_or(Ok(()), Err)
        }

        fn resume_link(&mut self) -> Result<(), String> {
            self.actions.lock().unwrap().push("link:resume".into());
            self.resume_error.clone().map_or(Ok(()), Err)
        }

        fn end_link(&mut self) -> Result<(), String> {
            self.actions.lock().unwrap().push("link:end".into());
            Ok(())
        }

        fn finish_recorder(&mut self) -> Result<(), String> {
            self.actions.lock().unwrap().push("recorder:finish".into());
            Ok(())
        }

        fn stop_sources(&mut self) -> Result<(), String> {
            self.actions.lock().unwrap().push("sources:stop".into());
            Ok(())
        }
    }

    fn temporary_app_data(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oss-lma-app-{test_name}-{}", uuid::Uuid::new_v4()))
    }

    fn service(
        permissions: PermissionSnapshot,
        readiness: SourceReadiness,
        app_data: PathBuf,
    ) -> (AppCapture, Arc<Mutex<Vec<String>>>) {
        service_with_behavior(
            permissions,
            readiness,
            app_data,
            None,
            None,
            None,
            Vec::new(),
        )
    }

    fn service_with_behavior(
        permissions: PermissionSnapshot,
        readiness: SourceReadiness,
        app_data: PathBuf,
        failure_point: Option<FailurePoint>,
        pause_error: Option<String>,
        resume_error: Option<String>,
        devices: Vec<super::CaptureDevice>,
    ) -> (AppCapture, Arc<Mutex<Vec<String>>>) {
        let actions = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::new(Mutex::new(CaptureState::default()));
        let backend = FakeBackend {
            permissions,
            readiness,
            actions: actions.clone(),
            state: state.clone(),
            failure_point,
            pause_error,
            resume_error,
            devices,
        };
        (
            AppCapture {
                app_data_dir: app_data,
                backend: Mutex::new(Box::new(backend)),
                operation: Mutex::new(()),
                selection: Mutex::new(CaptureDeviceSelection::default()),
                state,
                emit_snapshot: Arc::new(|_| {}),
            },
            actions,
        )
    }

    #[test]
    fn denied_permissions_prevent_source_start_and_meeting_creation() {
        let app_data = temporary_app_data("denied");
        let (service, actions) = service(
            PermissionSnapshot {
                screen_recording: super::PermissionStatus::Denied,
                microphone: super::PermissionStatus::Granted,
            },
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data.clone(),
        );

        let error = service.start(LinkOptions::test_default()).unwrap_err();

        assert_eq!(error, "capture permissions are not granted");
        assert_eq!(service.status().phase, CapturePhase::Failed);
        assert!(actions.lock().unwrap().is_empty());
        assert!(!app_data.join("recordings").exists());
    }

    #[test]
    fn one_inactive_source_prevents_call_id_and_recording_creation() {
        let app_data = temporary_app_data("inactive");
        let (service, actions) = service(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: false,
            },
            app_data.clone(),
        );

        let error = service.start(LinkOptions::test_default()).unwrap_err();

        assert_eq!(error, "both capture sources must be active");
        assert_eq!(service.status().phase, CapturePhase::Failed);
        assert_eq!(*actions.lock().unwrap(), ["sources:start", "sources:stop"]);
        assert!(!app_data.join("recordings").exists());
    }

    #[test]
    fn undetermined_permissions_are_requested_by_starting_the_native_sources() {
        let app_data = temporary_app_data("undetermined");
        let (service, actions) = service(
            PermissionSnapshot {
                screen_recording: super::PermissionStatus::Unknown,
                microphone: super::PermissionStatus::Unknown,
            },
            SourceReadiness {
                system: false,
                microphone: false,
            },
            app_data.clone(),
        );

        let error = service.start(LinkOptions::test_default()).unwrap_err();

        assert_eq!(error, "both capture sources must be active");
        assert_eq!(*actions.lock().unwrap(), ["sources:start", "sources:stop"]);
        assert!(!app_data.join("recordings").exists());
    }

    #[test]
    fn valid_sources_create_one_call_id_and_recording_directory() {
        let app_data = temporary_app_data("valid");
        let (service, actions) = service(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data.clone(),
        );

        let snapshot = service.start(LinkOptions::test_default()).unwrap();

        let call_id = snapshot.meeting_id.as_deref().unwrap();
        assert!(uuid::Uuid::parse_str(call_id).is_ok());
        let recording_path = snapshot.recording_path.as_ref().unwrap();
        assert_eq!(
            recording_path,
            &app_data.join("recordings").join(call_id).join("audio.wav")
        );
        assert!(recording_path.parent().unwrap().is_dir());
        let actions = actions.lock().unwrap();
        assert_eq!(
            actions
                .iter()
                .filter(|action| action.starts_with("link:start:"))
                .count(),
            1
        );
        assert_eq!(actions[2], format!("link:start:{call_id}"));
        fs::remove_dir_all(app_data).unwrap();
    }

    #[test]
    fn source_failure_during_startup_stops_before_creating_meeting_resources() {
        let app_data = temporary_app_data("startup-source-failure");
        let (service, actions) = service_with_behavior(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data.clone(),
            Some(FailurePoint::StartSources),
            None,
            None,
            Vec::new(),
        );

        let error = service.start(LinkOptions::test_default()).unwrap_err();

        assert_eq!(error, "source failed during startup");
        assert_eq!(*actions.lock().unwrap(), ["sources:start", "sources:stop"]);
        assert!(!app_data.join("recordings").exists());
    }

    #[test]
    fn activation_race_closes_link_recorder_and_sources() {
        let app_data = temporary_app_data("activation-race");
        let (service, actions) = service_with_behavior(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data.clone(),
            Some(FailurePoint::StartLink),
            None,
            None,
            Vec::new(),
        );

        let error = service.start(LinkOptions::test_default()).unwrap_err();

        assert_eq!(error, "source failed before activation");
        let actions = actions.lock().unwrap();
        assert_eq!(
            &actions[3..],
            ["link:end", "recorder:finish", "sources:stop"]
        );
        fs::remove_dir_all(app_data).unwrap();
    }

    #[test]
    fn rejects_system_output_overrides_that_screen_capture_cannot_honor() {
        let app_data = temporary_app_data("system-override");
        let (service, _) = service_with_behavior(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data,
            None,
            None,
            None,
            vec![super::CaptureDevice {
                id: "external-output".into(),
                name: "External Output".into(),
                is_default: false,
                kind: super::CaptureDeviceKind::SystemOutput,
            }],
        );

        let error = service
            .set_devices(CaptureDeviceSelection {
                system_output_id: Some("external-output".into()),
                microphone_id: None,
            })
            .unwrap_err();

        assert_eq!(
            error,
            "system audio capture supports the default output only"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn active_native_source_error_closes_the_session_and_reports_failure() {
        let (_commands, receiver) = std::sync::mpsc::channel();
        let failures = Arc::new(Mutex::new(Vec::new()));
        let emitted = failures.clone();
        let mut worker = super::NativeWorker::new(
            receiver,
            Arc::new(|_| {}),
            Arc::new(move |generation, error| {
                emitted.lock().unwrap().push((generation, error));
            }),
        );
        worker.active_generation = Some(7);
        let (events, source_events) = std::sync::mpsc::channel();
        worker.source_events = Some(source_events);
        events
            .send(lma_capture::macos::SourceEvent::Error(
                lma_capture::macos::SourceKind::System,
                "ScreenCaptureKit stopped".into(),
            ))
            .unwrap();

        worker.process_source_events();

        assert_eq!(
            *failures.lock().unwrap(),
            [(7, "ScreenCaptureKit stopped".into())]
        );
        assert!(worker.source_events.is_none());
    }

    #[test]
    fn pause_forwards_pause_before_exposing_paused_state() {
        let app_data = temporary_app_data("pause");
        let (service, actions) = service(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data.clone(),
        );
        service.start(LinkOptions::test_default()).unwrap();

        let snapshot = service.pause().unwrap();

        assert_eq!(snapshot.phase, CapturePhase::Paused);
        assert_eq!(actions.lock().unwrap().last().unwrap(), "link:pause");
        fs::remove_dir_all(app_data).unwrap();
    }

    #[test]
    fn pause_failure_closes_the_active_session_and_emits_inactive_sources() {
        let app_data = temporary_app_data("pause-failure");
        let (mut service, actions) = service_with_behavior(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data.clone(),
            None,
            Some("pause transport failed".into()),
            None,
            Vec::new(),
        );
        let snapshots = Arc::new(Mutex::new(Vec::new()));
        let emitted = snapshots.clone();
        service.emit_snapshot = Arc::new(move |snapshot| {
            emitted.lock().unwrap().push(snapshot.clone());
        });
        service.start(LinkOptions::test_default()).unwrap();

        let error = service.pause().unwrap_err();

        assert_eq!(error, "pause transport failed");
        assert_eq!(service.status().phase, CapturePhase::Failed);
        assert_eq!(
            &actions.lock().unwrap()[3..],
            ["link:pause", "link:end", "recorder:finish", "sources:stop"]
        );
        let failed = snapshots.lock().unwrap().last().unwrap().clone();
        assert!(!failed.system_active);
        assert!(!failed.microphone_active);
        fs::remove_dir_all(app_data).unwrap();
    }

    #[test]
    fn resume_failure_closes_the_active_session() {
        let app_data = temporary_app_data("resume-failure");
        let (service, actions) = service_with_behavior(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data.clone(),
            None,
            None,
            Some("resume transport failed".into()),
            Vec::new(),
        );
        service.start(LinkOptions::test_default()).unwrap();
        service.pause().unwrap();

        let error = service.resume().unwrap_err();

        assert_eq!(error, "resume transport failed");
        assert_eq!(service.status().phase, CapturePhase::Failed);
        assert_eq!(
            &actions.lock().unwrap()[4..],
            ["link:resume", "link:end", "recorder:finish", "sources:stop"]
        );
        fs::remove_dir_all(app_data).unwrap();
    }

    #[test]
    fn stop_sends_end_before_closing_the_wav_writer() {
        let app_data = temporary_app_data("stop");
        let (service, actions) = service(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data.clone(),
        );
        service.start(LinkOptions::test_default()).unwrap();

        let stopped = service.stop().unwrap();

        assert_eq!(stopped.phase, CapturePhase::Idle);
        assert_eq!(
            &actions.lock().unwrap()[3..],
            ["link:end", "recorder:finish", "sources:stop"]
        );
        fs::remove_dir_all(app_data).unwrap();
    }
}
