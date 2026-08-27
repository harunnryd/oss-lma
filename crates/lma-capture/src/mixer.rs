use std::collections::VecDeque;

use crate::StereoChunk;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceChannel {
    System,
    Microphone,
}

pub struct Mixer {
    system: VecDeque<f32>,
    microphone: VecDeque<f32>,
    system_muted: bool,
    microphone_muted: bool,
    paused: bool,
}

impl Mixer {
    pub const TICK_FRAMES: usize = 4_800;
    pub const MAX_BUFFERED_FRAMES: usize = 144_000;

    pub fn new() -> Self {
        Self {
            system: VecDeque::new(),
            microphone: VecDeque::new(),
            system_muted: false,
            microphone_muted: false,
            paused: false,
        }
    }

    pub fn push(&mut self, channel: SourceChannel, frames: &[f32]) -> Vec<StereoChunk> {
        if self.paused {
            return Vec::new();
        }

        let buffer = self.buffer_mut(channel);
        buffer.extend(frames.iter().copied());
        let overflow = buffer.len().saturating_sub(Self::MAX_BUFFERED_FRAMES);
        buffer.drain(..overflow);
        self.drain_chunks()
    }

    pub fn set_muted(&mut self, channel: SourceChannel, muted: bool) {
        match channel {
            SourceChannel::System => self.system_muted = muted,
            SourceChannel::Microphone => self.microphone_muted = muted,
        }
    }

    pub fn pause(&mut self) {
        self.paused = true;
        self.system.clear();
        self.microphone.clear();
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    fn buffer_mut(&mut self, channel: SourceChannel) -> &mut VecDeque<f32> {
        match channel {
            SourceChannel::System => &mut self.system,
            SourceChannel::Microphone => &mut self.microphone,
        }
    }

    fn drain_chunks(&mut self) -> Vec<StereoChunk> {
        let available = self.system.len().min(self.microphone.len());
        let chunk_count = available / Self::TICK_FRAMES;
        let mut chunks = Vec::with_capacity(chunk_count);

        for _ in 0..chunk_count {
            let mut pcm = Vec::with_capacity(StereoChunk::byte_len(48_000));
            for _ in 0..Self::TICK_FRAMES {
                let system = self.system.pop_front().expect("paired frame is available");
                let microphone = self
                    .microphone
                    .pop_front()
                    .expect("paired frame is available");
                let system = if self.system_muted { 0.0 } else { system };
                let microphone = if self.microphone_muted {
                    0.0
                } else {
                    microphone
                };
                pcm.extend_from_slice(&sample_to_i16(system).to_le_bytes());
                pcm.extend_from_slice(&sample_to_i16(microphone).to_le_bytes());
            }
            chunks.push(StereoChunk {
                pcm,
                frames: Self::TICK_FRAMES,
            });
        }

        chunks
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

fn sample_to_i16(sample: f32) -> i16 {
    if sample >= 1.0 {
        i16::MAX
    } else if sample <= -1.0 {
        i16::MIN
    } else {
        (sample * 32_768.0) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::{Mixer, SourceChannel};

    const TICK_FRAMES: usize = 4_800;

    fn samples(chunk: &crate::StereoChunk) -> Vec<i16> {
        chunk
            .pcm
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
            .collect()
    }

    #[test]
    fn drains_only_paired_ticks_and_carries_the_remainder() {
        let mut mixer = Mixer::new();

        assert!(mixer
            .push(SourceChannel::System, &vec![0.25; TICK_FRAMES + 1])
            .is_empty());
        let chunks = mixer.push(SourceChannel::Microphone, &vec![-0.25; TICK_FRAMES]);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].frames, TICK_FRAMES);
        assert_eq!(chunks[0].pcm.len(), 19_200);
        assert_eq!(samples(&chunks[0])[0..4], [8192, -8192, 8192, -8192]);

        assert!(mixer.push(SourceChannel::Microphone, &[0.0]).is_empty());
    }

    #[test]
    fn clamps_samples_at_the_i16_bounds() {
        let mut mixer = Mixer::new();
        mixer.push(SourceChannel::System, &vec![1.0; TICK_FRAMES]);

        let chunks = mixer.push(SourceChannel::Microphone, &vec![-1.0; TICK_FRAMES]);

        assert_eq!(
            samples(&chunks[0])[0..4],
            [32_767, -32_768, 32_767, -32_768]
        );
    }

    #[test]
    fn mute_emits_silence_while_consuming_the_muted_source() {
        let mut mixer = Mixer::new();
        mixer.set_muted(SourceChannel::System, true);
        mixer.push(SourceChannel::System, &vec![0.75; TICK_FRAMES]);

        let muted = mixer.push(SourceChannel::Microphone, &vec![0.5; TICK_FRAMES]);
        assert_eq!(samples(&muted[0])[0..4], [0, 16_384, 0, 16_384]);

        mixer.set_muted(SourceChannel::System, false);
        mixer.push(SourceChannel::System, &vec![0.25; TICK_FRAMES]);
        let unmuted = mixer.push(SourceChannel::Microphone, &vec![0.0; TICK_FRAMES]);
        assert_eq!(samples(&unmuted[0])[0..2], [8192, 0]);
    }

    #[test]
    fn pause_discards_audio_and_resume_starts_on_a_fresh_aligned_tick() {
        let mut mixer = Mixer::new();
        mixer.pause();
        assert!(mixer
            .push(SourceChannel::System, &vec![1.0; TICK_FRAMES])
            .is_empty());
        assert!(mixer
            .push(SourceChannel::Microphone, &vec![1.0; TICK_FRAMES])
            .is_empty());

        mixer.resume();
        assert!(mixer
            .push(SourceChannel::System, &vec![0.5; TICK_FRAMES])
            .is_empty());
        let chunks = mixer.push(SourceChannel::Microphone, &vec![-0.5; TICK_FRAMES]);
        assert_eq!(chunks.len(), 1);
        assert_eq!(samples(&chunks[0])[0..2], [16_384, -16_384]);
    }

    #[test]
    fn stalled_peer_keeps_only_the_latest_three_seconds_for_alignment() {
        let mut mixer = Mixer::new();
        let mut system = vec![0.25; 48_000];
        system.extend(vec![0.5; 144_000]);

        assert!(mixer.push(SourceChannel::System, &system).is_empty());
        let chunks = mixer.push(SourceChannel::Microphone, &vec![0.0; 144_000]);

        assert_eq!(chunks.len(), 30);
        assert_eq!(samples(&chunks[0])[0..2], [16_384, 0]);
    }
}
