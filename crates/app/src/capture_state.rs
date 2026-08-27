use std::path::PathBuf;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CapturePhase {
    #[default]
    Idle,
    Preflight,
    Starting,
    Active,
    Paused,
    Stopping,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceReadiness {
    pub system: bool,
    pub microphone: bool,
}

impl SourceReadiness {
    pub fn both_active(self) -> bool {
        self.system && self.microphone
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSnapshot {
    pub phase: CapturePhase,
    pub meeting_id: Option<String>,
    pub recording_path: Option<PathBuf>,
    pub system_active: bool,
    pub microphone_active: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct CaptureState {
    snapshot: CaptureSnapshot,
}

impl Default for CaptureSnapshot {
    fn default() -> Self {
        Self {
            phase: CapturePhase::Idle,
            meeting_id: None,
            recording_path: None,
            system_active: false,
            microphone_active: false,
            error: None,
        }
    }
}

impl CaptureState {
    pub fn snapshot(&self) -> CaptureSnapshot {
        self.snapshot.clone()
    }

    pub fn begin_preflight(&mut self) -> Result<(), String> {
        self.require_phase(&[CapturePhase::Idle, CapturePhase::Failed])?;
        self.snapshot = CaptureSnapshot {
            phase: CapturePhase::Preflight,
            ..CaptureSnapshot::default()
        };
        Ok(())
    }

    pub fn begin_starting(&mut self) -> Result<(), String> {
        self.transition(CapturePhase::Preflight, CapturePhase::Starting)
    }

    pub fn activate(
        &mut self,
        readiness: SourceReadiness,
        meeting_id: String,
        recording_path: PathBuf,
    ) -> Result<(), String> {
        self.require_phase(&[CapturePhase::Starting])?;
        if !readiness.both_active() {
            return Err("both capture sources must be active".to_owned());
        }
        self.snapshot.phase = CapturePhase::Active;
        self.snapshot.meeting_id = Some(meeting_id);
        self.snapshot.recording_path = Some(recording_path);
        self.snapshot.system_active = readiness.system;
        self.snapshot.microphone_active = readiness.microphone;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        self.transition(CapturePhase::Active, CapturePhase::Paused)
    }

    pub fn resume(&mut self) -> Result<(), String> {
        self.transition(CapturePhase::Paused, CapturePhase::Active)
    }

    pub fn begin_stopping(&mut self) -> Result<(), String> {
        self.require_phase(&[
            CapturePhase::Active,
            CapturePhase::Paused,
            CapturePhase::Failed,
        ])?;
        self.snapshot.phase = CapturePhase::Stopping;
        Ok(())
    }

    pub fn finish_stopping(&mut self) -> Result<(), String> {
        self.require_phase(&[CapturePhase::Stopping])?;
        self.snapshot = CaptureSnapshot::default();
        Ok(())
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        self.snapshot.phase = CapturePhase::Failed;
        self.snapshot.error = Some(error.into());
    }

    fn transition(&mut self, expected: CapturePhase, next: CapturePhase) -> Result<(), String> {
        self.require_phase(&[expected])?;
        self.snapshot.phase = next;
        Ok(())
    }

    fn require_phase(&self, expected: &[CapturePhase]) -> Result<(), String> {
        if expected.contains(&self.snapshot.phase) {
            Ok(())
        } else {
            Err(format!(
                "capture cannot transition from {:?}",
                self.snapshot.phase
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{CapturePhase, CaptureState, SourceReadiness};

    #[test]
    fn follows_the_meeting_lifecycle() {
        let mut state = CaptureState::default();

        state.begin_preflight().unwrap();
        assert_eq!(state.snapshot().phase, CapturePhase::Preflight);
        state.begin_starting().unwrap();
        assert_eq!(state.snapshot().phase, CapturePhase::Starting);
        state
            .activate(
                SourceReadiness {
                    system: true,
                    microphone: true,
                },
                "meeting-1".into(),
                PathBuf::from("/recordings/meeting-1/audio.wav"),
            )
            .unwrap();
        assert_eq!(state.snapshot().phase, CapturePhase::Active);
        state.pause().unwrap();
        assert_eq!(state.snapshot().phase, CapturePhase::Paused);
        state.resume().unwrap();
        assert_eq!(state.snapshot().phase, CapturePhase::Active);
        state.begin_stopping().unwrap();
        assert_eq!(state.snapshot().phase, CapturePhase::Stopping);
        state.finish_stopping().unwrap();
        assert_eq!(state.snapshot().phase, CapturePhase::Idle);
    }

    #[test]
    fn refuses_activation_until_both_sources_are_ready() {
        let mut state = CaptureState::default();
        state.begin_preflight().unwrap();
        state.begin_starting().unwrap();

        let error = state
            .activate(
                SourceReadiness {
                    system: true,
                    microphone: false,
                },
                "meeting-1".into(),
                PathBuf::from("/recordings/meeting-1/audio.wav"),
            )
            .unwrap_err();

        assert_eq!(error, "both capture sources must be active");
        assert_eq!(state.snapshot().phase, CapturePhase::Starting);
    }

    #[test]
    fn failure_is_terminal_until_a_new_start_attempt() {
        let mut state = CaptureState::default();
        state.begin_preflight().unwrap();
        state.fail("microphone permission denied");

        assert_eq!(state.snapshot().phase, CapturePhase::Failed);
        assert_eq!(
            state.snapshot().error.as_deref(),
            Some("microphone permission denied")
        );
        assert!(state.pause().is_err());

        state.begin_preflight().unwrap();
        assert_eq!(state.snapshot().phase, CapturePhase::Preflight);
        assert!(state.snapshot().error.is_none());
    }
}
