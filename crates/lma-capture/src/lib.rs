pub mod mixer;
pub mod recorder;

pub use mixer::{Mixer, SourceChannel};
pub use recorder::WavRecorder;

/// A fixed-duration interleaved stereo PCM audio chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StereoChunk {
    pub pcm: Vec<u8>,
    pub frames: usize,
}

impl StereoChunk {
    /// Number of bytes in a 100 ms stereo, 16-bit PCM chunk at `rate` Hz.
    pub const fn byte_len(rate: usize) -> usize {
        rate * 2 * 2 / 10
    }
}

/// Current authorization state for a capture capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Unknown,
    Denied,
    Granted,
}

/// A capture device exposed to the shell for selection and display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Platform-neutral notifications emitted by the capture layer.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureEvent {
    PermissionChanged(PermissionState),
    DeviceAdded(DeviceInfo),
    DeviceRemoved(String),
    Started,
    Stopped,
    Chunk(StereoChunk),
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::StereoChunk;

    #[test]
    fn stereo_chunk_has_the_documented_wire_size() {
        assert_eq!(StereoChunk::byte_len(48_000), 19_200);
    }
}
