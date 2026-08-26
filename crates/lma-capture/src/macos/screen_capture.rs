#![allow(clippy::useless_transmute)]

use std::sync::{
    mpsc::{self, Sender},
    Arc,
};

use cidre::sc::{StreamDelegate as _, StreamOutput as _};
use cidre::{arc, cm, define_obj_type, dispatch, ns, objc, sc};

use super::{
    devices::DeviceWatcher, DeviceSelection, MonoFrames, NativeStream, NativeStreamEvents,
    SourceKind,
};

struct ScreenOutputInner {
    frames: Sender<MonoFrames>,
    events: Arc<dyn NativeStreamEvents>,
}

define_obj_type!(
    ScreenOutput + sc::stream::OutputImpl + sc::stream::DelegateImpl,
    ScreenOutputInner,
    SCREEN_OUTPUT
);

impl sc::StreamOutput for ScreenOutput {}
impl sc::StreamDelegate for ScreenOutput {}

#[objc::add_methods]
impl sc::stream::OutputImpl for ScreenOutput {
    extern "C" fn impl_stream_did_output_sample_buf(
        &mut self,
        _cmd: Option<&cidre::objc::Sel>,
        _stream: &sc::Stream,
        sample_buf: &mut cm::SampleBuf,
        kind: sc::OutputType,
    ) {
        if kind != sc::OutputType::Audio || !sample_buf.data_is_ready() {
            return;
        }
        let result = sample_buf
            .audio_buf_list::<1>()
            .map_err(|error| format!("{error:?}"))
            .and_then(|buffers| {
                let buffer = buffers
                    .list()
                    .as_slice()
                    .first()
                    .ok_or_else(|| "ScreenCaptureKit returned no audio buffer".to_owned())?;
                if buffer.data.is_null() || buffer.data_bytes_size == 0 {
                    return Ok(Vec::new());
                }
                let len = buffer.data_bytes_size as usize / std::mem::size_of::<f32>();
                let samples = unsafe { std::slice::from_raw_parts(buffer.data.cast::<f32>(), len) };
                Ok(samples.to_vec())
            });
        match result {
            Ok(samples) if !samples.is_empty() => {
                let _ = self.inner().frames.send(MonoFrames::new(samples));
            }
            Ok(_) => {}
            Err(error) => {
                self.inner().events.error(error);
            }
        }
    }
}

#[objc::add_methods]
impl sc::stream::DelegateImpl for ScreenOutput {
    extern "C" fn impl_stream_did_stop_with_err(
        &mut self,
        _cmd: Option<&cidre::objc::Sel>,
        _stream: &sc::Stream,
        _error: &ns::Error,
    ) {
        self.inner().events.disconnected();
    }
}

struct ScreenCaptureStream {
    stream: arc::R<sc::Stream>,
    _output: arc::R<ScreenOutput>,
    _queue: arc::R<dispatch::Queue>,
    _watcher: DeviceWatcher,
}

impl NativeStream for ScreenCaptureStream {
    fn stop(&mut self) {
        let _ = wait_for_stream(|completion| self.stream.stop_with_ch(completion));
    }
}

pub(super) fn start(
    selection: &DeviceSelection,
    frames: Sender<MonoFrames>,
    events: Arc<dyn NativeStreamEvents>,
) -> Result<Box<dyn NativeStream>, String> {
    if !matches!(selection, DeviceSelection::Default) {
        return Err("ScreenCaptureKit captures the active default output only".to_owned());
    }
    let content = current_content()?;
    let displays = content.displays();
    let display = displays
        .first()
        .ok_or_else(|| "ScreenCaptureKit found no display".to_owned())?;
    let windows = ns::Array::<sc::Window>::new();
    let filter = sc::ContentFilter::with_display_excluding_windows(display, &windows);
    let mut configuration = sc::StreamCfg::new();
    configuration.set_width(display.width() as usize);
    configuration.set_height(display.height() as usize);
    configuration.set_captures_audio(true);
    configuration.set_excludes_current_process_audio(false);
    configuration.set_sample_rate(48_000);
    configuration.set_channel_count(1);

    let output = ScreenOutput::with(ScreenOutputInner {
        frames,
        events: events.clone(),
    });
    let stream = sc::Stream::with_delegate(&filter, &configuration, output.as_ref());
    let queue = dispatch::Queue::serial_with_ar_pool();
    stream
        .add_stream_output(output.as_ref(), sc::OutputType::Audio, Some(&queue))
        .map_err(|error| format!("{error:?}"))?;
    let watcher = DeviceWatcher::new(SourceKind::System, selection, events)?;
    wait_for_stream(|completion| stream.start_with_ch(completion))?;
    Ok(Box::new(ScreenCaptureStream {
        stream,
        _output: output,
        _queue: queue,
        _watcher: watcher,
    }))
}

fn current_content() -> Result<arc::R<sc::ShareableContent>, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    sc::ShareableContent::current_with_ch(move |content, error| {
        let result = match (content, error) {
            (Some(content), _) => Ok(content.retained()),
            (_, Some(error)) => Err(format!("{error:?}")),
            _ => Err("ScreenCaptureKit returned no shareable content".to_owned()),
        };
        let _ = sender.send(result);
    });
    receiver
        .recv()
        .map_err(|_| "ScreenCaptureKit content request was cancelled".to_owned())?
}

fn wait_for_stream(action: impl FnOnce(Box<dyn FnMut(Option<&ns::Error>)>)) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    action(Box::new(move |error| {
        let _ = sender.send(error.map(|error| format!("{error:?}")));
    }));
    match receiver
        .recv()
        .map_err(|_| "ScreenCaptureKit operation was cancelled".to_owned())?
    {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
