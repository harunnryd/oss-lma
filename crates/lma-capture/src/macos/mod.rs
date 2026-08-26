mod devices;
mod microphone;
mod permissions;
mod screen_capture;

use std::{
    rc::Rc,
    sync::{mpsc::Sender, Arc},
};

pub use crate::DeviceKind;
pub use devices::{DeviceSelection, MacDevices};
pub use permissions::{MacPermissions, PermissionKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    System,
    Microphone,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonoFrames {
    pub samples: Vec<f32>,
    pub level: f32,
}

impl MonoFrames {
    pub fn new(samples: Vec<f32>) -> Self {
        let level = if samples.is_empty() {
            0.0
        } else {
            (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32)
                .sqrt()
        };
        Self { samples, level }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceEvent {
    Started(SourceKind),
    Stopped(SourceKind),
    RebuildRequired(SourceKind),
    Error(SourceKind, String),
}

pub trait NativeStream {
    fn stop(&mut self) -> Result<(), String>;
}

pub trait NativeStreamEvents: Send + Sync {
    fn disconnected(&self);
    fn error(&self, error: String);
}

pub trait NativeStreamProvider {
    fn start(
        &self,
        kind: SourceKind,
        selection: &DeviceSelection,
        frames: Sender<MonoFrames>,
        events: Arc<dyn NativeStreamEvents>,
    ) -> Result<Box<dyn NativeStream>, String>;
}

#[derive(Clone, Copy)]
pub struct NativeStreams;

impl NativeStreamProvider for NativeStreams {
    fn start(
        &self,
        kind: SourceKind,
        selection: &DeviceSelection,
        frames: Sender<MonoFrames>,
        events: Arc<dyn NativeStreamEvents>,
    ) -> Result<Box<dyn NativeStream>, String> {
        match kind {
            SourceKind::System => screen_capture::start(selection, frames, events),
            SourceKind::Microphone => microphone::start(selection, frames, events),
        }
    }
}

struct EventForwarder {
    kind: SourceKind,
    sender: Sender<SourceEvent>,
}

impl NativeStreamEvents for EventForwarder {
    fn disconnected(&self) {
        let _ = self.sender.send(SourceEvent::RebuildRequired(self.kind));
    }

    fn error(&self, error: String) {
        let _ = self.sender.send(SourceEvent::Error(self.kind, error));
    }
}

pub struct MacSource<P = NativeStreams> {
    kind: SourceKind,
    provider: P,
    events: Sender<SourceEvent>,
}

impl MacSource {
    pub fn system(events: Sender<SourceEvent>) -> Self {
        Self {
            kind: SourceKind::System,
            provider: NativeStreams,
            events,
        }
    }

    pub fn microphone(events: Sender<SourceEvent>) -> Self {
        Self {
            kind: SourceKind::Microphone,
            provider: NativeStreams,
            events,
        }
    }
}

impl<P: NativeStreamProvider + 'static> MacSource<P> {
    pub fn with_provider(kind: SourceKind, provider: P, events: Sender<SourceEvent>) -> Self {
        Self {
            kind,
            provider,
            events,
        }
    }

    pub fn start(self, selection: DeviceSelection, frames: Sender<MonoFrames>) -> SourceHandle {
        let provider: Rc<dyn NativeStreamProvider> = Rc::new(self.provider);
        let stream = provider
            .start(
                self.kind,
                &selection,
                frames.clone(),
                Arc::new(EventForwarder {
                    kind: self.kind,
                    sender: self.events.clone(),
                }),
            )
            .inspect_err(|error| {
                let _ = self
                    .events
                    .send(SourceEvent::Error(self.kind, error.clone()));
            })
            .ok();
        if stream.is_some() {
            let _ = self.events.send(SourceEvent::Started(self.kind));
        }
        SourceHandle {
            kind: self.kind,
            selection,
            frames,
            events: self.events,
            provider,
            stream,
        }
    }
}

pub struct SourceHandle {
    kind: SourceKind,
    selection: DeviceSelection,
    frames: Sender<MonoFrames>,
    events: Sender<SourceEvent>,
    provider: Rc<dyn NativeStreamProvider>,
    stream: Option<Box<dyn NativeStream>>,
}

impl SourceHandle {
    pub fn is_active(&self) -> bool {
        self.stream.is_some()
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut stream) = self.stream.take() {
            if let Err(error) = stream.stop() {
                let _ = self
                    .events
                    .send(SourceEvent::Error(self.kind, error.clone()));
                return Err(error);
            }
            let _ = self.events.send(SourceEvent::Stopped(self.kind));
        }
        Ok(())
    }

    pub fn rebuild(&mut self, selection: DeviceSelection) -> Result<(), String> {
        let _ = self.stop();
        match self.provider.start(
            self.kind,
            &selection,
            self.frames.clone(),
            Arc::new(EventForwarder {
                kind: self.kind,
                sender: self.events.clone(),
            }),
        ) {
            Ok(stream) => {
                self.selection = selection;
                self.stream = Some(stream);
                let _ = self.events.send(SourceEvent::Started(self.kind));
                Ok(())
            }
            Err(error) => {
                let _ = self
                    .events
                    .send(SourceEvent::Error(self.kind, error.clone()));
                Err(error)
            }
        }
    }
}

impl Drop for SourceHandle {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{mpsc, mpsc::Sender, Arc},
    };

    use super::{
        DeviceSelection, MacSource, MonoFrames, NativeStream, NativeStreamEvents,
        NativeStreamProvider, SourceEvent, SourceKind,
    };

    type StreamAction = (SourceKind, &'static str, DeviceSelection);
    type StreamActions = Rc<RefCell<Vec<StreamAction>>>;
    type NativeObservers = Rc<RefCell<Vec<(SourceKind, Arc<dyn NativeStreamEvents>)>>>;

    #[derive(Clone)]
    struct FakeStreams {
        actions: StreamActions,
        observers: NativeObservers,
        stop_error: Option<String>,
    }

    struct FakeStream {
        kind: SourceKind,
        selection: DeviceSelection,
        actions: StreamActions,
        stop_error: Option<String>,
    }

    impl NativeStream for FakeStream {
        fn stop(&mut self) -> Result<(), String> {
            self.actions
                .borrow_mut()
                .push((self.kind, "stop", self.selection.clone()));
            match &self.stop_error {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }
    }

    impl NativeStreamProvider for FakeStreams {
        fn start(
            &self,
            kind: SourceKind,
            selection: &DeviceSelection,
            _frames: Sender<MonoFrames>,
            events: Arc<dyn NativeStreamEvents>,
        ) -> Result<Box<dyn NativeStream>, String> {
            self.actions
                .borrow_mut()
                .push((kind, "start", selection.clone()));
            self.observers.borrow_mut().push((kind, events));
            Ok(Box::new(FakeStream {
                kind,
                selection: selection.clone(),
                actions: self.actions.clone(),
                stop_error: self.stop_error.clone(),
            }))
        }
    }

    #[test]
    fn rebuilding_a_disconnected_microphone_leaves_system_audio_running() {
        let actions = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeStreams {
            actions: actions.clone(),
            observers: Rc::new(RefCell::new(Vec::new())),
            stop_error: None,
        };
        let (frames_tx, _frames_rx) = mpsc::channel();
        let (events_tx, _events_rx) = mpsc::channel();
        let system =
            MacSource::with_provider(SourceKind::System, provider.clone(), events_tx.clone());
        let microphone = MacSource::with_provider(SourceKind::Microphone, provider, events_tx);
        let system_handle = system.start(DeviceSelection::Default, frames_tx.clone());
        let mut microphone_handle =
            microphone.start(DeviceSelection::DeviceId("external-mic".into()), frames_tx);
        actions.borrow_mut().clear();

        microphone_handle
            .rebuild(DeviceSelection::DeviceId("replacement-mic".into()))
            .unwrap();

        assert_eq!(
            *actions.borrow(),
            [
                (
                    SourceKind::Microphone,
                    "stop",
                    DeviceSelection::DeviceId("external-mic".into())
                ),
                (
                    SourceKind::Microphone,
                    "start",
                    DeviceSelection::DeviceId("replacement-mic".into())
                )
            ]
        );
        assert!(system_handle.is_active());
        assert!(microphone_handle.is_active());
    }

    #[test]
    fn native_disconnect_emits_rebuild_event_for_that_source() {
        let provider = FakeStreams {
            actions: Rc::new(RefCell::new(Vec::new())),
            observers: Rc::new(RefCell::new(Vec::new())),
            stop_error: None,
        };
        let observers = provider.observers.clone();
        let (frames_tx, _frames_rx) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::channel();
        let microphone = MacSource::with_provider(SourceKind::Microphone, provider, events_tx);
        let _handle = microphone.start(DeviceSelection::Default, frames_tx);
        assert_eq!(
            events_rx.recv().unwrap(),
            SourceEvent::Started(SourceKind::Microphone)
        );

        observers.borrow()[0].1.disconnected();

        assert_eq!(
            events_rx.recv().unwrap(),
            SourceEvent::RebuildRequired(SourceKind::Microphone)
        );
    }

    #[test]
    fn mono_frames_report_root_mean_square_level() {
        let frames = MonoFrames::new(vec![1.0, -1.0, 0.0, 0.0]);

        assert_eq!(frames.samples, vec![1.0, -1.0, 0.0, 0.0]);
        assert!((frames.level - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.000_001);
    }

    #[test]
    fn failed_stop_emits_an_error_without_marking_the_source_stopped() {
        let provider = FakeStreams {
            actions: Rc::new(RefCell::new(Vec::new())),
            observers: Rc::new(RefCell::new(Vec::new())),
            stop_error: Some("native stop failed".into()),
        };
        let (frames_tx, _frames_rx) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::channel();
        let source = MacSource::with_provider(SourceKind::System, provider, events_tx);
        let mut handle = source.start(DeviceSelection::Default, frames_tx);
        assert_eq!(
            events_rx.recv().unwrap(),
            SourceEvent::Started(SourceKind::System)
        );

        assert_eq!(handle.stop().unwrap_err(), "native stop failed");

        assert!(!handle.is_active());
        assert_eq!(
            events_rx.recv().unwrap(),
            SourceEvent::Error(SourceKind::System, "native stop failed".into())
        );
        assert!(events_rx.try_recv().is_err());
    }

    #[test]
    fn rebuild_starts_a_new_stream_after_native_stop_fails() {
        let actions = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeStreams {
            actions: actions.clone(),
            observers: Rc::new(RefCell::new(Vec::new())),
            stop_error: Some("native stop failed".into()),
        };
        let (frames_tx, _frames_rx) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::channel();
        let source = MacSource::with_provider(SourceKind::Microphone, provider, events_tx);
        let mut handle = source.start(DeviceSelection::Default, frames_tx);
        assert_eq!(
            events_rx.recv().unwrap(),
            SourceEvent::Started(SourceKind::Microphone)
        );
        actions.borrow_mut().clear();

        handle
            .rebuild(DeviceSelection::DeviceId("replacement-mic".into()))
            .unwrap();

        assert!(handle.is_active());
        assert_eq!(
            *actions.borrow(),
            [
                (SourceKind::Microphone, "stop", DeviceSelection::Default),
                (
                    SourceKind::Microphone,
                    "start",
                    DeviceSelection::DeviceId("replacement-mic".into())
                )
            ]
        );
        assert_eq!(
            events_rx.recv().unwrap(),
            SourceEvent::Error(SourceKind::Microphone, "native stop failed".into())
        );
        assert_eq!(
            events_rx.recv().unwrap(),
            SourceEvent::Started(SourceKind::Microphone)
        );
    }
}
