use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, Runtime};

use crate::capture_state::{CaptureSnapshot, CaptureState, SourceReadiness};
use crate::settings::{ProviderSettings, SettingsError};
use crate::sidecar::SidecarSupervisor;

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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CapturePermissionKind {
    ScreenRecording,
    Microphone,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkOptions {
    pub port: u16,
    pub token: String,
    pub diarize_microphone: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartMeetingOptions {
    #[serde(default)]
    pub diarize_microphone: bool,
}

impl LinkOptions {
    fn from_supervised_endpoint(
        endpoint: crate::sidecar::SidecarEndpoint,
        diarize_microphone: bool,
    ) -> Self {
        Self {
            port: endpoint.port(),
            token: endpoint.token().expose().to_owned(),
            diarize_microphone,
        }
    }

    pub fn with_provider_settings(
        port: u16,
        token: String,
        settings: &ProviderSettings,
    ) -> Result<Self, SettingsError> {
        settings.validate()?;
        Ok(Self {
            port,
            token,
            diarize_microphone: settings.diarize_mic,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelMeters {
    pub system: f32,
    pub microphone: f32,
}

pub trait CaptureBackend: Send {
    fn permissions(&mut self) -> Result<PermissionSnapshot, String>;
    fn open_permission_settings(&mut self, kind: CapturePermissionKind) -> Result<(), String>;
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
    backend: Arc<Mutex<Box<dyn CaptureBackend>>>,
    operation: Mutex<()>,
    selection: Mutex<CaptureDeviceSelection>,
    state: Arc<Mutex<CaptureState>>,
    emit_snapshot: SnapshotEmitter,
    sidecar: Option<Arc<SidecarSupervisor>>,
}

impl AppCapture {
    pub(crate) fn from_tauri<R: Runtime>(
        app: &tauri::AppHandle<R>,
        sidecar: Arc<SidecarSupervisor>,
    ) -> Result<Self, String> {
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
        let meeting_app = app.clone();
        let emit_meeting: MeetingEmitter = Arc::new(move |event| {
            let _ = meeting_app.emit("meeting-event", event);
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

        let backend = NativeCaptureBackend::new(emit_levels, emit_failure, emit_meeting)?;
        Ok(Self {
            app_data_dir,
            backend: Arc::new(Mutex::new(Box::new(backend))),
            operation: Mutex::new(()),
            selection: Mutex::new(CaptureDeviceSelection::default()),
            state,
            emit_snapshot,
            sidecar: Some(sidecar),
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
            backend: Arc::new(Mutex::new(Box::new(backend))),
            operation: Mutex::new(()),
            selection: Mutex::new(CaptureDeviceSelection::default()),
            state: Arc::new(Mutex::new(CaptureState::default())),
            emit_snapshot,
            sidecar: None,
        }
    }

    pub fn permissions(&self) -> Result<PermissionSnapshot, String> {
        self.backend.lock().unwrap().permissions()
    }

    pub fn open_permission_settings(&self, kind: CapturePermissionKind) -> Result<(), String> {
        self.backend.lock().unwrap().open_permission_settings(kind)
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
        let options = if let Some(sidecar) = &self.sidecar {
            let endpoint = sidecar.endpoint();
            self.state
                .lock()
                .unwrap()
                .set_sidecar_available(endpoint.is_some());
            let endpoint = match endpoint {
                Some(endpoint) => endpoint,
                None => return self.fail(lma_link::ErrorCode::SidecarUnavailable.as_str()),
            };
            LinkOptions::from_supervised_endpoint(endpoint, options.diarize_microphone)
        } else {
            options
        };
        self.update_state(|state| state.begin_preflight().map(|_| ()))?;
        let generation = self.state.lock().unwrap().generation();

        let permissions = match self.backend.lock().unwrap().permissions() {
            Ok(permissions) => permissions,
            Err(error) => return self.fail(error),
        };
        if permissions.has_denial() {
            return self.fail(lma_link::ErrorCode::CapturePermissionDenied.as_str());
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
            return match self.startup_error(generation) {
                Some(startup_error) => Err(startup_error),
                None => self.fail(error),
            };
        }

        let call_id = meeting_id.clone();
        match self.update_state(|state| {
            state.activate_if_current(generation, readiness, meeting_id, recording_path)
        }) {
            Ok(snapshot) => {
                if self.sidecar.is_some() {
                    self.watch_supervised_endpoint(generation, call_id, options);
                }
                Ok(snapshot)
            }
            Err(error) => {
                let mut backend = self.backend.lock().unwrap();
                let _ = backend.end_link();
                let _ = backend.finish_recorder();
                let _ = backend.stop_sources();
                Err(self.startup_error(generation).unwrap_or(error))
            }
        }
    }

    fn start_from_webview(&self, options: StartMeetingOptions) -> Result<CaptureSnapshot, String> {
        let endpoint = match self.sidecar.as_ref().and_then(|sidecar| sidecar.endpoint()) {
            Some(endpoint) => endpoint,
            None => return self.fail(lma_link::ErrorCode::SidecarUnavailable.as_str()),
        };
        self.start(LinkOptions::from_supervised_endpoint(
            endpoint,
            options.diarize_microphone,
        ))
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

    fn watch_supervised_endpoint(&self, generation: u64, call_id: String, options: LinkOptions) {
        let sidecar = self.sidecar.as_ref().expect("sidecar is present").clone();
        let backend = self.backend.clone();
        let state = self.state.clone();
        let emit_snapshot = self.emit_snapshot.clone();
        std::thread::spawn(move || {
            let mut options = options;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let phase = {
                    let state = state.lock().unwrap();
                    if state.generation() != generation {
                        return;
                    }
                    state.snapshot().phase
                };
                if !matches!(
                    phase,
                    crate::capture_state::CapturePhase::Active
                        | crate::capture_state::CapturePhase::Paused
                ) {
                    return;
                }

                let endpoint = sidecar.endpoint();
                let Some(endpoint) = endpoint else {
                    fail_watched_session(
                        &backend,
                        &state,
                        &emit_snapshot,
                        generation,
                        lma_link::ErrorCode::SidecarUnavailable.as_str(),
                    );
                    return;
                };
                let replacement =
                    LinkOptions::from_supervised_endpoint(endpoint, options.diarize_microphone);
                if replacement == options {
                    continue;
                }

                let result = backend
                    .lock()
                    .unwrap()
                    .start_link(&call_id, &replacement, generation);
                if result.is_err() {
                    fail_watched_session(
                        &backend,
                        &state,
                        &emit_snapshot,
                        generation,
                        lma_link::ErrorCode::SidecarUnavailable.as_str(),
                    );
                    return;
                }
                if phase == crate::capture_state::CapturePhase::Paused
                    && backend.lock().unwrap().pause_link().is_err()
                {
                    fail_watched_session(
                        &backend,
                        &state,
                        &emit_snapshot,
                        generation,
                        lma_link::ErrorCode::SidecarUnavailable.as_str(),
                    );
                    return;
                }
                options = replacement;
            }
        });
    }
}

fn fail_watched_session(
    backend: &Mutex<Box<dyn CaptureBackend>>,
    state: &Mutex<CaptureState>,
    emit_snapshot: &SnapshotEmitter,
    generation: u64,
    error: &str,
) {
    let snapshot = {
        let mut state = state.lock().unwrap();
        state.set_sidecar_available(false);
        state
            .fail_if_current(generation, error)
            .then(|| state.snapshot())
    };
    if let Some(snapshot) = snapshot {
        let mut backend = backend.lock().unwrap();
        let _ = backend.end_link();
        let _ = backend.finish_recorder();
        let _ = backend.stop_sources();
        emit_snapshot(&snapshot);
    }
}

type LevelEmitter = Arc<dyn Fn(LevelMeters) + Send + Sync>;
type FailureEmitter = Arc<dyn Fn(u64, String) + Send + Sync>;
type MeetingEmitter = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

fn bridge_link_event(
    event: lma_link::LinkEvent,
    emit: &MeetingEmitter,
) -> Option<lma_link::ErrorCode> {
    match event {
        lma_link::LinkEvent::MeetingEvent(envelope) => {
            emit(envelope);
            None
        }
        lma_link::LinkEvent::Error {
            call_id,
            code,
            context,
        } => {
            emit(
                serde_json::json!({"EventType":"ERROR", "CallId":call_id, "Code":code.as_str(), "Context":context}),
            );
            Some(code)
        }
        _ => None,
    }
}

#[cfg(target_os = "macos")]
struct NativeCaptureBackend {
    commands: std::sync::mpsc::Sender<NativeCommand>,
}

#[cfg(target_os = "macos")]
enum NativeCommand {
    Permissions(std::sync::mpsc::SyncSender<Result<PermissionSnapshot, String>>),
    OpenPermissionSettings(
        CapturePermissionKind,
        std::sync::mpsc::SyncSender<Result<(), String>>,
    ),
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
    fn new(
        emit_levels: LevelEmitter,
        emit_failure: FailureEmitter,
        emit_meeting: MeetingEmitter,
    ) -> Result<Self, String> {
        let (commands, receiver) = std::sync::mpsc::channel();
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("lma-capture".to_owned())
            .spawn(move || {
                NativeWorker::new(receiver, emit_levels, emit_failure, emit_meeting).run(ready)
            })
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

    fn open_permission_settings(&mut self, kind: CapturePermissionKind) -> Result<(), String> {
        self.request(|reply| NativeCommand::OpenPermissionSettings(kind, reply))
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
pub struct NativePipeline {
    system: Option<lma_capture::macos::SourceHandle>,
    microphone: Option<lma_capture::macos::SourceHandle>,
    source_events: Option<std::sync::mpsc::Receiver<lma_capture::macos::SourceEvent>>,
    selection: CaptureDeviceSelection,
    rebuild_retries: Vec<RebuildRetry>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct RebuildRetry {
    kind: lma_capture::macos::SourceKind,
    next_attempt: std::time::Instant,
    delay: std::time::Duration,
}

#[cfg(target_os = "macos")]
impl NativePipeline {
    fn new() -> Self {
        Self {
            system: None,
            microphone: None,
            source_events: None,
            selection: CaptureDeviceSelection::default(),
            rebuild_retries: Vec::new(),
        }
    }

    pub fn with_sources(
        selection: CaptureDeviceSelection,
        system: lma_capture::macos::SourceHandle,
        microphone: lma_capture::macos::SourceHandle,
        source_events: std::sync::mpsc::Receiver<lma_capture::macos::SourceEvent>,
    ) -> Self {
        Self {
            system: Some(system),
            microphone: Some(microphone),
            source_events: Some(source_events),
            selection,
            rebuild_retries: Vec::new(),
        }
    }

    pub fn process_source_events(&mut self) -> Result<(), String> {
        self.process_source_events_at(std::time::Instant::now())
    }

    fn process_source_events_at(&mut self, now: std::time::Instant) -> Result<(), String> {
        let events = self
            .source_events
            .as_ref()
            .map(|events| events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            match event {
                lma_capture::macos::SourceEvent::RebuildRequired(kind) => {
                    self.attempt_rebuild(kind, now)?;
                }
                lma_capture::macos::SourceEvent::Error(kind, error) => {
                    self.clear_rebuild(kind);
                    return Err(error);
                }
                lma_capture::macos::SourceEvent::Started(kind) => self.clear_rebuild(kind),
                lma_capture::macos::SourceEvent::Stopped(_)
                | lma_capture::macos::SourceEvent::CleanupWarning(_, _) => {}
            }
        }
        let due = self
            .rebuild_retries
            .iter()
            .filter(|retry| retry.next_attempt <= now)
            .map(|retry| retry.kind)
            .collect::<Vec<_>>();
        for kind in due {
            self.attempt_rebuild(kind, now)?;
        }
        Ok(())
    }

    fn attempt_rebuild(
        &mut self,
        kind: lma_capture::macos::SourceKind,
        now: std::time::Instant,
    ) -> Result<(), String> {
        let selection = match kind {
            lma_capture::macos::SourceKind::System => {
                native_selection(&self.selection.system_output_id)
            }
            lma_capture::macos::SourceKind::Microphone => {
                native_selection(&self.selection.microphone_id)
            }
        };
        let source = match kind {
            lma_capture::macos::SourceKind::System => self.system.as_mut(),
            lma_capture::macos::SourceKind::Microphone => self.microphone.as_mut(),
        };
        let Some(source) = source else {
            return Ok(());
        };
        match source.rebuild(selection) {
            Ok(()) => {
                self.clear_rebuild(kind);
                Ok(())
            }
            Err(error) if source.is_active() => Err(error),
            Err(_) => {
                self.schedule_rebuild(kind, now);
                Ok(())
            }
        }
    }

    fn schedule_rebuild(&mut self, kind: lma_capture::macos::SourceKind, now: std::time::Instant) {
        const MIN_DELAY: std::time::Duration = std::time::Duration::from_millis(100);
        const MAX_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
        if let Some(retry) = self
            .rebuild_retries
            .iter_mut()
            .find(|retry| retry.kind == kind)
        {
            retry.delay = (retry.delay * 2).min(MAX_DELAY);
            retry.next_attempt = now + retry.delay;
        } else {
            self.rebuild_retries.push(RebuildRetry {
                kind,
                next_attempt: now + MIN_DELAY,
                delay: MIN_DELAY,
            });
        }
    }

    fn clear_rebuild(&mut self, kind: lma_capture::macos::SourceKind) {
        self.rebuild_retries.retain(|retry| retry.kind != kind);
    }

    pub fn source_active(&self, kind: lma_capture::macos::SourceKind) -> bool {
        match kind {
            lma_capture::macos::SourceKind::System => self.system.as_ref(),
            lma_capture::macos::SourceKind::Microphone => self.microphone.as_ref(),
        }
        .is_some_and(lma_capture::macos::SourceHandle::is_active)
    }

    fn stop_sources(&mut self) -> Result<(), String> {
        let system = self.system.as_mut().map(|source| source.stop());
        let microphone = self.microphone.as_mut().map(|source| source.stop());
        self.system = None;
        self.microphone = None;
        self.source_events = None;
        self.rebuild_retries.clear();
        system
            .into_iter()
            .chain(microphone)
            .find_map(Result::err)
            .map_or(Ok(()), Err)
    }
}

#[cfg(target_os = "macos")]
struct NativeWorker {
    commands: std::sync::mpsc::Receiver<NativeCommand>,
    emit_levels: LevelEmitter,
    emit_failure: FailureEmitter,
    emit_meeting: MeetingEmitter,
    runtime: Option<tokio::runtime::Runtime>,
    system_frames: Option<std::sync::mpsc::Receiver<lma_capture::macos::MonoFrames>>,
    microphone_frames: Option<std::sync::mpsc::Receiver<lma_capture::macos::MonoFrames>>,
    active_generation: Option<u64>,
    pipeline: NativePipeline,
    mixer: lma_capture::Mixer,
    recorder: Option<lma_capture::WavRecorder>,
    link: Option<lma_link::LinkClient>,
    link_events: Option<tokio::sync::broadcast::Receiver<lma_link::LinkEvent>>,
    levels: LevelMeters,
}

#[cfg(target_os = "macos")]
impl NativeWorker {
    fn new(
        commands: std::sync::mpsc::Receiver<NativeCommand>,
        emit_levels: LevelEmitter,
        emit_failure: FailureEmitter,
        emit_meeting: MeetingEmitter,
    ) -> Self {
        Self {
            commands,
            emit_levels,
            emit_failure,
            emit_meeting,
            runtime: None,
            system_frames: None,
            microphone_frames: None,
            active_generation: None,
            pipeline: NativePipeline::new(),
            mixer: lma_capture::Mixer::new(),
            recorder: None,
            link: None,
            link_events: None,
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
            self.process_link_events();
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
            NativeCommand::OpenPermissionSettings(kind, reply) => {
                let permission = match kind {
                    CapturePermissionKind::ScreenRecording => {
                        lma_capture::macos::MacPermissions::screen_recording()
                    }
                    CapturePermissionKind::Microphone => {
                        lma_capture::macos::MacPermissions::microphone()
                    }
                };
                let _ = reply.send(permission.open_settings());
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
                    .map(|link| {
                        self.link_events = Some(link.subscribe());
                        self.link = Some(link);
                    })
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
        self.pipeline = NativePipeline::with_sources(selection, system, microphone, source_events);
        self.system_frames = Some(system_receiver);
        self.microphone_frames = Some(microphone_receiver);
        self.active_generation = Some(generation);
        Ok(readiness)
    }

    fn stop_sources(&mut self) -> Result<(), String> {
        self.system_frames = None;
        self.microphone_frames = None;
        self.active_generation = None;
        self.mixer = lma_capture::Mixer::new();
        self.pipeline.stop_sources()
    }

    fn process_source_events(&mut self) {
        if let Err(error) = self.pipeline.process_source_events() {
            self.fail_session(error);
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

    fn process_link_events(&mut self) {
        let error = self.link_events.as_mut().and_then(|events| loop {
            match events.try_recv() {
                Ok(event) => match bridge_link_event(event, &self.emit_meeting) {
                    Some(code) if code != lma_link::ErrorCode::SttStreamReset => break Some(code),
                    _ => continue,
                },
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break None,
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break None,
            }
        });
        if let Some(error) = error {
            self.fail_session(error.as_str().to_owned());
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
        self.link_events = None;
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

    fn open_permission_settings(&mut self, _kind: CapturePermissionKind) -> Result<(), String> {
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
pub fn open_capture_permission_settings(
    kind: CapturePermissionKind,
    state: tauri::State<'_, AppCapture>,
) -> Result<(), String> {
    state.open_permission_settings(kind)
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
    options: StartMeetingOptions,
    state: tauri::State<'_, AppCapture>,
) -> Result<CaptureSnapshot, String> {
    state.start_from_webview(options)
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
        bridge_link_event, AppCapture, CaptureBackend, CaptureDeviceSelection,
        CapturePermissionKind, LinkOptions, MeetingEmitter, PermissionSnapshot,
    };

    #[test]
    fn bridge_emits_transcript_partials_and_finals_as_meeting_events() {
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let captured = emitted.clone();
        let emitter: MeetingEmitter = Arc::new(move |event| captured.lock().unwrap().push(event));
        for (transcript, partial) in [("partial", true), ("final", false)] {
            let event = lma_link::LinkEvent::MeetingEvent(serde_json::json!({
                "EventType": "ADD_TRANSCRIPT_SEGMENT", "CallId": "call-1", "SegmentId": "s1",
                "Channel": "CALLER", "StartTime": 0.0, "EndTime": 1.0,
                "Transcript": transcript, "IsPartial": partial
            }));
            assert_eq!(bridge_link_event(event, &emitter), None);
        }
        let emitted = emitted.lock().unwrap();
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0]["Transcript"], "partial");
        assert_eq!(emitted[1]["Transcript"], "final");
        assert_eq!(emitted[1]["IsPartial"], false);
    }

    #[test]
    fn link_options_reject_invalid_provider_settings_before_capture_starts() {
        let settings = crate::settings::ProviderSettings {
            provider: crate::settings::ProviderKind::Deepgram,
            model: " ".to_owned(),
            language: None,
            azure_region: None,
            diarize_system: false,
            diarize_mic: false,
        };

        assert!(LinkOptions::with_provider_settings(8765, "token".to_owned(), &settings).is_err());
    }

    #[test]
    fn webview_start_without_a_supervised_endpoint_uses_the_catalog_code() {
        let (service, _) = service(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            temporary_app_data("missing-supervisor"),
        );

        let error = service
            .start_from_webview(super::StartMeetingOptions {
                diarize_microphone: false,
            })
            .expect_err("webview start requires the private supervisor endpoint");

        assert_eq!(error, lma_link::ErrorCode::SidecarUnavailable.as_str());
        assert_eq!(service.status().phase, CapturePhase::Failed);
    }

    #[test]
    fn webview_start_uses_the_supervised_endpoint_instead_of_payload_values() {
        let app_data = temporary_app_data("supervised-webview-options");
        let (service, starts) = supervised_service(&app_data);
        let options = serde_json::from_value(serde_json::json!({
            "diarizeMicrophone": true,
            "port": 1,
            "token": "webview-controlled",
        }))
        .expect("public capture options deserialize");

        let snapshot = service
            .start_from_webview(options)
            .expect("supervised capture starts");

        let starts = starts.lock().unwrap();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].0, snapshot.meeting_id.as_deref().unwrap());
        assert_eq!(starts[0].1.port, 43123);
        assert_eq!(starts[0].1.token, "a1b2c3");
        assert!(starts[0].1.diarize_microphone);
        fs::remove_dir_all(app_data).unwrap();
    }

    #[test]
    fn supervised_start_creates_one_recording_and_reconnects_after_endpoint_replacement() {
        let app_data = temporary_app_data("supervised-endpoint-replacement");
        let (service, starts) = supervised_service(&app_data);

        let snapshot = service
            .start_from_webview(super::StartMeetingOptions {
                diarize_microphone: false,
            })
            .expect("supervised capture starts");
        let call_id = snapshot.meeting_id.clone().expect("call ID is created");
        assert!(snapshot.recording_path.as_ref().unwrap().is_file());

        let sidecar = service.sidecar.as_ref().unwrap();
        sidecar
            .respawn(supervisor_config())
            .expect("supervisor replaces endpoint");
        wait_until("replacement START", || starts.lock().unwrap().len() == 2);

        let starts = starts.lock().unwrap();
        assert_eq!(starts[0].0, call_id);
        assert_eq!(starts[1].0, call_id);
        assert_eq!(starts[0].1.port, 43123);
        assert_eq!(starts[1].1.port, 43124);
        assert_eq!(starts[1].1.token, "d4e5f6");
        drop(starts);
        service.stop().expect("reconnected meeting stops");
        fs::remove_dir_all(app_data).unwrap();
    }

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
        Sources,
        LinkActivation,
        LinkRejectedAfterFailure,
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

        fn open_permission_settings(&mut self, kind: CapturePermissionKind) -> Result<(), String> {
            let kind = match kind {
                CapturePermissionKind::ScreenRecording => "screen-recording",
                CapturePermissionKind::Microphone => "microphone",
            };
            self.actions
                .lock()
                .unwrap()
                .push(format!("permissions:open:{kind}"));
            Ok(())
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
            if self.failure_point == Some(FailurePoint::Sources) {
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
            if matches!(
                self.failure_point,
                Some(FailurePoint::LinkActivation | FailurePoint::LinkRejectedAfterFailure)
            ) {
                self.state
                    .lock()
                    .unwrap()
                    .fail_if_current(generation, "source failed before activation");
            }
            if self.failure_point == Some(FailurePoint::LinkRejectedAfterFailure) {
                Err("capture start was superseded".into())
            } else {
                Ok(())
            }
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
                backend: Arc::new(Mutex::new(Box::new(backend))),
                operation: Mutex::new(()),
                selection: Mutex::new(CaptureDeviceSelection::default()),
                state,
                emit_snapshot: Arc::new(|_| {}),
                sidecar: None,
            },
            actions,
        )
    }

    type SupervisedStarts = Arc<Mutex<Vec<(String, LinkOptions)>>>;

    struct SupervisedBackend {
        starts: SupervisedStarts,
    }

    impl CaptureBackend for SupervisedBackend {
        fn permissions(&mut self) -> Result<PermissionSnapshot, String> {
            Ok(PermissionSnapshot::granted())
        }

        fn open_permission_settings(&mut self, _kind: CapturePermissionKind) -> Result<(), String> {
            Ok(())
        }

        fn devices(&mut self) -> Result<Vec<super::CaptureDevice>, String> {
            Ok(Vec::new())
        }

        fn start_sources(
            &mut self,
            _selection: &CaptureDeviceSelection,
            _generation: u64,
        ) -> Result<SourceReadiness, String> {
            Ok(SourceReadiness {
                system: true,
                microphone: true,
            })
        }

        fn open_recorder(&mut self, path: &std::path::Path) -> Result<(), String> {
            fs::File::create(path)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        fn start_link(
            &mut self,
            call_id: &str,
            options: &LinkOptions,
            _generation: u64,
        ) -> Result<(), String> {
            self.starts
                .lock()
                .unwrap()
                .push((call_id.to_owned(), options.clone()));
            Ok(())
        }

        fn pause_link(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn resume_link(&mut self) -> Result<(), String> {
            Ok(())
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

    fn supervisor_config() -> crate::sidecar::RuntimeConfig {
        crate::sidecar::SidecarSupervisor::runtime_config(
            crate::settings::ProviderSettings {
                provider: crate::settings::ProviderKind::AssemblyAi,
                model: "universal-streaming".to_owned(),
                language: Some("en".to_owned()),
                azure_region: None,
                diarize_system: false,
                diarize_mic: false,
            },
            "test-provider-key".to_owned(),
        )
    }

    fn supervised_service(app_data: &std::path::Path) -> (AppCapture, SupervisedStarts) {
        fs::create_dir_all(app_data).expect("test app data directory is created");
        let marker = app_data.join("sidecar-generation");
        let script = format!(
            "if [ -f '{marker}' ]; then printf 'SIDECAR_READY port=43124 token=d4e5f6\\n'; else : > '{marker}'; printf 'SIDECAR_READY port=43123 token=a1b2c3\\n'; fi; exec 1>&-; sleep 30",
            marker = marker.display(),
        );
        let sidecar = Arc::new(crate::sidecar::SidecarSupervisor::new(
            crate::sidecar::SidecarCommand::new("/bin/sh", ["-c", &script]),
        ));
        sidecar
            .spawn(supervisor_config())
            .expect("fake supervisor starts");
        let starts = Arc::new(Mutex::new(Vec::new()));
        let capture = AppCapture {
            app_data_dir: app_data.to_owned(),
            backend: Arc::new(Mutex::new(Box::new(SupervisedBackend {
                starts: starts.clone(),
            }))),
            operation: Mutex::new(()),
            selection: Mutex::new(CaptureDeviceSelection::default()),
            state: Arc::new(Mutex::new(CaptureState::default())),
            emit_snapshot: Arc::new(|_| {}),
            sidecar: Some(sidecar),
        };
        (capture, starts)
    }

    fn wait_until(description: &str, condition: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !condition() {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {description}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
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

        assert_eq!(error, lma_link::ErrorCode::CapturePermissionDenied.as_str());
        assert_eq!(service.status().phase, CapturePhase::Failed);
        assert!(actions.lock().unwrap().is_empty());
        assert!(!app_data.join("recordings").exists());
    }

    #[test]
    fn permission_settings_action_is_forwarded_to_the_native_backend() {
        let app_data = temporary_app_data("permission-settings");
        let (service, actions) = service(
            PermissionSnapshot {
                screen_recording: super::PermissionStatus::Denied,
                microphone: super::PermissionStatus::Granted,
            },
            SourceReadiness {
                system: false,
                microphone: false,
            },
            app_data,
        );

        service
            .open_permission_settings(CapturePermissionKind::ScreenRecording)
            .unwrap();

        assert_eq!(
            actions.lock().unwrap().last().map(String::as_str),
            Some("permissions:open:screen-recording")
        );
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
            Some(FailurePoint::Sources),
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
            Some(FailurePoint::LinkActivation),
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
    fn start_link_rejection_preserves_the_generation_failure() {
        let app_data = temporary_app_data("start-link-after-failure");
        let (service, actions) = service_with_behavior(
            PermissionSnapshot::granted(),
            SourceReadiness {
                system: true,
                microphone: true,
            },
            app_data.clone(),
            Some(FailurePoint::LinkRejectedAfterFailure),
            None,
            None,
            Vec::new(),
        );

        let error = service.start(LinkOptions::test_default()).unwrap_err();

        assert_eq!(error, "source failed before activation");
        assert_eq!(
            service.status().error.as_deref(),
            Some("source failed before activation")
        );
        assert_eq!(
            &actions.lock().unwrap()[3..],
            ["recorder:finish", "sources:stop"]
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
            Arc::new(|_| {}),
        );
        worker.active_generation = Some(7);
        let (events, source_events) = std::sync::mpsc::channel();
        worker.pipeline.source_events = Some(source_events);
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
        assert!(worker.pipeline.source_events.is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn successful_rebuild_survives_a_stopped_cleanup_error() {
        use std::{cell::Cell, rc::Rc};

        struct CleanupStream {
            cleanup_error: bool,
        }

        impl lma_capture::macos::NativeStream for CleanupStream {
            fn stop(&mut self) -> Result<(), lma_capture::macos::NativeStopError> {
                if self.cleanup_error {
                    Err(lma_capture::macos::NativeStopError::Stopped(
                        "old stream cleanup failed".into(),
                    ))
                } else {
                    Ok(())
                }
            }
        }

        #[derive(Clone)]
        struct CleanupProvider {
            starts: Rc<Cell<usize>>,
        }

        impl lma_capture::macos::NativeStreamProvider for CleanupProvider {
            fn start(
                &self,
                _kind: lma_capture::macos::SourceKind,
                _selection: &lma_capture::macos::DeviceSelection,
                _frames: std::sync::mpsc::Sender<lma_capture::macos::MonoFrames>,
                _events: Arc<dyn lma_capture::macos::NativeStreamEvents>,
            ) -> Result<Box<dyn lma_capture::macos::NativeStream>, String> {
                let start = self.starts.get();
                self.starts.set(start + 1);
                Ok(Box::new(CleanupStream {
                    cleanup_error: start == 0,
                }))
            }
        }

        let (_commands, receiver) = std::sync::mpsc::channel();
        let failures = Arc::new(Mutex::new(Vec::new()));
        let emitted = failures.clone();
        let mut worker = super::NativeWorker::new(
            receiver,
            Arc::new(|_| {}),
            Arc::new(move |generation, error| {
                emitted.lock().unwrap().push((generation, error));
            }),
            Arc::new(|_| {}),
        );
        worker.active_generation = Some(9);
        let starts = Rc::new(Cell::new(0));
        let (events, source_events) = std::sync::mpsc::channel();
        let (frames, _frame_receiver) = std::sync::mpsc::channel();
        worker.pipeline.microphone = Some(
            lma_capture::macos::MacSource::with_provider(
                lma_capture::macos::SourceKind::Microphone,
                CleanupProvider {
                    starts: starts.clone(),
                },
                events.clone(),
            )
            .start(lma_capture::macos::DeviceSelection::Default, frames),
        );
        worker.pipeline.source_events = Some(source_events);
        events
            .send(lma_capture::macos::SourceEvent::RebuildRequired(
                lma_capture::macos::SourceKind::Microphone,
            ))
            .unwrap();

        worker.process_source_events();
        worker.process_source_events();

        assert_eq!(starts.get(), 2);
        assert!(worker.pipeline.microphone.as_ref().unwrap().is_active());
        assert!(failures.lock().unwrap().is_empty());
        assert!(worker.pipeline.source_events.is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unavailable_selected_microphone_retries_without_failing_the_meeting() {
        use std::{cell::Cell, rc::Rc, time::Duration};

        struct RetryStream;

        impl lma_capture::macos::NativeStream for RetryStream {
            fn stop(&mut self) -> Result<(), lma_capture::macos::NativeStopError> {
                Ok(())
            }
        }

        #[derive(Clone)]
        struct RetryProvider {
            starts: Rc<Cell<usize>>,
        }

        impl lma_capture::macos::NativeStreamProvider for RetryProvider {
            fn start(
                &self,
                _kind: lma_capture::macos::SourceKind,
                _selection: &lma_capture::macos::DeviceSelection,
                _frames: std::sync::mpsc::Sender<lma_capture::macos::MonoFrames>,
                _events: Arc<dyn lma_capture::macos::NativeStreamEvents>,
            ) -> Result<Box<dyn lma_capture::macos::NativeStream>, String> {
                let attempt = self.starts.get();
                self.starts.set(attempt + 1);
                if attempt == 1 {
                    Err("selected microphone is unavailable".into())
                } else {
                    Ok(Box::new(RetryStream))
                }
            }
        }

        let starts = Rc::new(Cell::new(0));
        let (events, source_events) = std::sync::mpsc::channel();
        let (frames, _frame_receiver) = std::sync::mpsc::channel();
        let microphone = lma_capture::macos::MacSource::with_provider(
            lma_capture::macos::SourceKind::Microphone,
            RetryProvider {
                starts: starts.clone(),
            },
            events.clone(),
        )
        .start(
            lma_capture::macos::DeviceSelection::DeviceId("selected-mic".into()),
            frames,
        );
        let mut pipeline = super::NativePipeline::new();
        pipeline.selection.microphone_id = Some("selected-mic".into());
        pipeline.microphone = Some(microphone);
        pipeline.source_events = Some(source_events);
        events
            .send(lma_capture::macos::SourceEvent::RebuildRequired(
                lma_capture::macos::SourceKind::Microphone,
            ))
            .unwrap();
        let now = std::time::Instant::now();

        pipeline.process_source_events_at(now).unwrap();
        assert!(!pipeline.source_active(lma_capture::macos::SourceKind::Microphone));
        pipeline
            .process_source_events_at(now + Duration::from_millis(99))
            .unwrap();
        assert_eq!(starts.get(), 2);
        pipeline
            .process_source_events_at(now + Duration::from_millis(100))
            .unwrap();

        assert_eq!(starts.get(), 3);
        assert!(pipeline.source_active(lma_capture::macos::SourceKind::Microphone));
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
