use std::collections::VecDeque;

use lma_capture::StereoChunk;

pub struct ReconnectBuffer {
    chunks: VecDeque<StereoChunk>,
    capacity_bytes: usize,
    buffered_bytes: usize,
    dropped_frames: usize,
}

impl ReconnectBuffer {
    pub fn new(rate: usize) -> Self {
        Self {
            chunks: VecDeque::new(),
            capacity_bytes: rate * 2 * 2 * 3,
            buffered_bytes: 0,
            dropped_frames: 0,
        }
    }

    pub fn push(&mut self, chunk: StereoChunk) {
        while self.buffered_bytes + chunk.pcm.len() > self.capacity_bytes {
            let Some(dropped) = self.chunks.pop_front() else {
                self.dropped_frames += chunk.frames;
                return;
            };
            self.buffered_bytes -= dropped.pcm.len();
            self.dropped_frames += dropped.frames;
        }
        self.buffered_bytes += chunk.pcm.len();
        self.chunks.push_back(chunk);
    }

    pub fn drain(&mut self) -> Vec<StereoChunk> {
        self.buffered_bytes = 0;
        self.chunks.drain(..).collect()
    }

    pub fn dropped_frames(&self) -> usize {
        self.dropped_frames
    }
}

#[cfg(test)]
mod tests {
    use lma_capture::StereoChunk;

    use super::ReconnectBuffer;

    fn chunk(frames: usize) -> StereoChunk {
        StereoChunk {
            pcm: vec![frames as u8; frames * 4],
            frames,
        }
    }

    #[test]
    fn drops_oldest_chunks_after_three_seconds_of_stereo_audio() {
        let mut buffer = ReconnectBuffer::new(10);
        buffer.push(chunk(10));
        buffer.push(chunk(20));
        buffer.push(chunk(10));
        let drained = buffer.drain();
        assert_eq!(
            drained.iter().map(|chunk| chunk.frames).collect::<Vec<_>>(),
            [20, 10]
        );
        assert_eq!(buffer.dropped_frames(), 10);
    }
}
