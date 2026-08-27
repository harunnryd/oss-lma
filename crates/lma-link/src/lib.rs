pub use lma_capture::StereoChunk;

mod client;
mod protocol;
mod reconnect;

pub use client::{LinkClient, LinkError, Result};
pub use reconnect::ReconnectBuffer;

/// Commands controlling the sidecar audio link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkCommand {
    Start,
    Pause,
    Resume,
    End,
}

/// State and telemetry notifications emitted by the sidecar audio link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkEvent {
    Connected,
    Disconnected,
    BufferDropped,
    Error,
}

#[cfg(test)]
mod tests {
    use super::{LinkCommand, LinkEvent};

    #[test]
    fn link_commands_and_events_are_exposed() {
        let _ = LinkCommand::Pause;
        let _ = LinkCommand::End;
        let _ = LinkEvent::Connected;
        let _ = LinkEvent::Disconnected;
    }
}
