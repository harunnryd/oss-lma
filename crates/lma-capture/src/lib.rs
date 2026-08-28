#[cfg(target_os = "macos")]
pub mod macos;
pub mod mixer;
pub mod recorder;

pub use mixer::{Mixer, SourceChannel};
pub use recorder::WavRecorder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StereoChunk {
    pub pcm: Vec<u8>,
    pub frames: usize,
}

impl StereoChunk {
    pub const fn byte_len(rate: usize) -> usize {
        rate * 2 * 2 / 10
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Unknown,
    Denied,
    Granted,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum DeviceKind {
    SystemOutput,
    Microphone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub kind: DeviceKind,
}

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
