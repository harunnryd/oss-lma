pub use lma_capture::StereoChunk;

mod client;
mod protocol;
mod reconnect;

pub use client::{LinkClient, LinkError, Result};
pub use reconnect::ReconnectBuffer;

#[derive(Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    SttProviderAuth,
    SttStreamReset,
    LinkDisconnected,
    CaptureDeviceLost,
    CapturePermissionDenied,
    VpContainerFailed,
    VpManualActionRequired,
    AgentToolFailure,
    RagEmbeddingUnavailable,
    DbWriteConflict,
    SidecarUnavailable,
    PortBindFailed,
}

impl ErrorCode {
    pub const ALL: [Self; 12] = [
        Self::SttProviderAuth,
        Self::SttStreamReset,
        Self::LinkDisconnected,
        Self::CaptureDeviceLost,
        Self::CapturePermissionDenied,
        Self::VpContainerFailed,
        Self::VpManualActionRequired,
        Self::AgentToolFailure,
        Self::RagEmbeddingUnavailable,
        Self::DbWriteConflict,
        Self::SidecarUnavailable,
        Self::PortBindFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SttProviderAuth => "STT_PROVIDER_AUTH",
            Self::SttStreamReset => "STT_STREAM_RESET",
            Self::LinkDisconnected => "LINK_DISCONNECTED",
            Self::CaptureDeviceLost => "CAPTURE_DEVICE_LOST",
            Self::CapturePermissionDenied => "CAPTURE_PERMISSION_DENIED",
            Self::VpContainerFailed => "VP_CONTAINER_FAILED",
            Self::VpManualActionRequired => "VP_MANUAL_ACTION_REQUIRED",
            Self::AgentToolFailure => "AGENT_TOOL_FAILURE",
            Self::RagEmbeddingUnavailable => "RAG_EMBEDDING_UNAVAILABLE",
            Self::DbWriteConflict => "DB_WRITE_CONFLICT",
            Self::SidecarUnavailable => "SIDECAR_UNAVAILABLE",
            Self::PortBindFailed => "PORT_BIND_FAILED",
        }
    }
}

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
    Error {
        call_id: String,
        code: ErrorCode,
        context: serde_json::Value,
    },
}

#[cfg(test)]
mod tests {
    use super::{ErrorCode, LinkCommand, LinkEvent};

    #[test]
    fn link_commands_and_events_are_exposed() {
        let _ = LinkCommand::Pause;
        let _ = LinkCommand::End;
        let _ = LinkEvent::Connected;
        let _ = LinkEvent::Disconnected;
        let _ = LinkEvent::Error {
            call_id: String::new(),
            code: ErrorCode::LinkDisconnected,
            context: serde_json::Value::Null,
        };
    }
}
