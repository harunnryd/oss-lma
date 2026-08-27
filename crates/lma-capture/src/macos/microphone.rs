use std::{
    ffi::c_void,
    sync::{mpsc::Sender, Arc},
};

use cidre::{av, core_audio::Device, define_obj_type, ns, objc};

use super::{
    devices::DeviceWatcher, DeviceSelection, MacPermissions, MonoFrames, NativeStopError,
    NativeStream, NativeStreamEvents, SourceKind,
};

const TARGET_SAMPLE_RATE: f64 = 48_000.0;

define_obj_type!(InputNodeAccess(ns::Id));

impl InputNodeAccess {
    #[objc::msg_send(audioUnit)]
    fn raw_audio_unit(&self) -> *mut c_void;
}

struct MicrophoneStream {
    engine: cidre::arc::R<av::AudioEngine>,
    input: cidre::arc::R<av::AudioInputNode>,
    _watcher: DeviceWatcher,
}

impl NativeStream for MicrophoneStream {
    fn stop(&mut self) -> Result<(), NativeStopError> {
        let result = self
            .input
            .remove_tap_on_bus(0)
            .map_err(|error| NativeStopError::Stopped(format!("{error:?}")));
        self.engine.stop();
        result
    }
}

pub(super) fn start(
    selection: &DeviceSelection,
    frames: Sender<MonoFrames>,
    events: Arc<dyn NativeStreamEvents>,
) -> Result<Box<dyn NativeStream>, String> {
    MacPermissions::microphone().ensure_access()?;
    let mut engine = av::AudioEngine::new();
    let mut input = engine.input_node();
    if let DeviceSelection::DeviceId(id) = selection {
        select_input_device(&input, id)?;
    }
    let watcher = DeviceWatcher::new(SourceKind::Microphone, selection, events.clone())?;
    let mut resampler = None::<StreamingResampler>;
    let callback_events = events.clone();
    input
        .install_tap_on_bus(0, 2_048, None, move |buffer, _time| {
            let format = buffer.format();
            let channel_count = format.channel_count() as usize;
            let mono = if buffer.stride() > 1 {
                buffer
                    .data_f32_at(0)
                    .map(|samples| downmix_interleaved(samples, channel_count))
            } else {
                let channels = (0..channel_count)
                    .filter_map(|channel| buffer.data_f32_at(channel))
                    .collect::<Vec<_>>();
                (!channels.is_empty()).then(|| downmix(&channels))
            };
            let Some(mono) = mono else {
                callback_events.error("AVAudioEngine returned a non-Float32 buffer".to_owned());
                return;
            };
            let sample_rate = format.absd().sample_rate;
            if resampler
                .as_ref()
                .is_none_or(|current| current.source_rate != sample_rate)
            {
                resampler = Some(StreamingResampler::new(sample_rate));
            }
            let normalized = resampler.as_mut().unwrap().process(&mono);
            if !normalized.is_empty() {
                let _ = frames.send(MonoFrames::new(normalized));
            }
        })
        .map_err(|error| format!("{error:?}"))?;
    engine.prepare();
    if let Err(error) = engine.start() {
        let _ = input.remove_tap_on_bus(0);
        return Err(format!("{error:?}"));
    }
    Ok(Box::new(MicrophoneStream {
        engine,
        input,
        _watcher: watcher,
    }))
}

fn select_input_device(input: &av::AudioInputNode, id: &str) -> Result<(), String> {
    let uid = cidre::cf::String::from_str(id);
    let device = Device::with_uid(&uid).map_err(|error| format!("{error:?}"))?;
    let access = unsafe { &*(input as *const av::AudioInputNode).cast::<InputNodeAccess>() };
    let audio_unit = access.raw_audio_unit();
    if audio_unit.is_null() {
        return Err("AVAudioEngine input node has no AudioUnit".to_owned());
    }
    let device_id = device.0 .0;
    let status = unsafe {
        AudioUnitSetProperty(
            audio_unit,
            2_000,
            0,
            0,
            (&device_id as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(format!("AudioUnit rejected input device {id}: {status}"))
    }
}

fn downmix(channels: &[&[f32]]) -> Vec<f32> {
    let frames = channels
        .iter()
        .map(|channel| channel.len())
        .min()
        .unwrap_or(0);
    (0..frames)
        .map(|frame| {
            channels.iter().map(|channel| channel[frame]).sum::<f32>() / channels.len() as f32
        })
        .collect()
}

fn downmix_interleaved(samples: &[f32], channel_count: usize) -> Vec<f32> {
    if channel_count == 0 {
        return Vec::new();
    }
    samples
        .chunks_exact(channel_count)
        .map(|frame| frame.iter().sum::<f32>() / channel_count as f32)
        .collect()
}

struct StreamingResampler {
    source_rate: f64,
    position: f64,
    previous: Option<f32>,
}

impl StreamingResampler {
    fn new(source_rate: f64) -> Self {
        Self {
            source_rate,
            position: 0.0,
            previous: None,
        }
    }

    fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        if samples.is_empty() || self.source_rate <= 0.0 {
            return Vec::new();
        }
        let step = self.source_rate / TARGET_SAMPLE_RATE;
        let mut output = Vec::with_capacity(
            ((samples.len() as f64 * TARGET_SAMPLE_RATE / self.source_rate).ceil() as usize)
                .saturating_add(1),
        );
        while self.position < samples.len() as f64 {
            let base = self.position.floor() as isize;
            let fraction = (self.position - base as f64) as f32;
            let current = if base < 0 {
                self.previous.unwrap_or(samples[0])
            } else {
                samples[base as usize]
            };
            if fraction == 0.0 {
                output.push(current);
            } else {
                let next_index = base + 1;
                if next_index < 0 {
                    break;
                }
                let Some(next) = samples.get(next_index as usize) else {
                    break;
                };
                output.push(current + (*next - current) * fraction);
            }
            self.position += step;
        }
        self.position -= samples.len() as f64;
        self.previous = samples.last().copied();
        output
    }
}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioUnitSetProperty(
        audio_unit: *mut c_void,
        property_id: u32,
        scope: u32,
        element: u32,
        data: *const c_void,
        data_size: u32,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::{downmix, StreamingResampler};

    #[test]
    fn downmixes_channels_without_changing_frame_count() {
        assert_eq!(downmix(&[&[1.0, -1.0], &[0.0, 0.5]]), vec![0.5, -0.25]);
    }

    #[test]
    fn resamples_continuously_across_callback_boundaries() {
        let mut resampler = StreamingResampler::new(24_000.0);

        assert_eq!(
            resampler.process(&[0.0, 1.0, 0.0]),
            vec![0.0, 0.5, 1.0, 0.5, 0.0]
        );
        assert_eq!(resampler.process(&[1.0]), vec![0.5, 1.0]);
    }
}
