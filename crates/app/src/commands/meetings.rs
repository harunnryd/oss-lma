use std::sync::Mutex;

use serde::Serialize;
use tauri::{Manager, Runtime, State};

use crate::meetings::{MeetingSegment, MeetingSummary, MeetingsError, MeetingsRepository};

pub struct MeetingsState {
    repository: Mutex<Option<MeetingsRepository>>,
}

impl MeetingsState {
    pub fn from_tauri<R: Runtime>(app: &tauri::App<R>) -> Result<Self, String> {
        let app_data_dir = app.path().app_data_dir().map_err(|error| error.to_string())?;
        let db_path = app_data_dir.join("lma.db");
        let repository = if db_path.exists() {
            Some(MeetingsRepository::open(&db_path).map_err(|error| error.to_string())?)
        } else {
            None
        };
        Ok(Self {
            repository: Mutex::new(repository),
        })
    }

    fn ensure<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut MeetingsRepository) -> Result<R, MeetingsError>,
    {
        let mut guard = self.repository.lock().map_err(|error| error.to_string())?;
        let repo = guard
            .as_mut()
            .ok_or_else(|| "meetings database not yet available; start a meeting first".to_owned())?;
        f(repo).map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub fn list_meetings(limit: Option<i64>, state: State<'_, MeetingsState>) -> Result<Vec<MeetingSummary>, String> {
    state.ensure(|repo| repo.list_meetings(limit.unwrap_or(50)))
}

#[tauri::command]
pub fn list_meeting_segments(meeting_id: String, state: State<'_, MeetingsState>) -> Result<Vec<MeetingSegment>, String> {
    state.ensure(|repo| repo.list_meeting_segments(&meeting_id))
}

#[tauri::command]
pub fn delete_meeting(meeting_id: String, state: State<'_, MeetingsState>) -> Result<(), String> {
    state.ensure(|repo| repo.delete_meeting(&meeting_id))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingsStateStatus {
    pub available: bool,
}
