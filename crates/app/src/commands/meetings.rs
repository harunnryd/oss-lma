use std::{path::PathBuf, sync::Mutex};

use serde::Serialize;
use tauri::{Manager, Runtime, State};

use crate::meetings::{MeetingSegment, MeetingSummary, MeetingsError, MeetingsRepository};

pub struct MeetingsState {
    db_path: PathBuf,
    repository: Mutex<Option<MeetingsRepository>>,
}

impl MeetingsState {
    pub fn from_tauri<R: Runtime>(app: &tauri::App<R>) -> Result<Self, String> {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        let db_path = app_data_dir.join("lma.db");
        let repository = if db_path.exists() {
            Some(MeetingsRepository::open(&db_path).map_err(|error| error.to_string())?)
        } else {
            None
        };
        Ok(Self {
            db_path,
            repository: Mutex::new(repository),
        })
    }

    fn ensure<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut MeetingsRepository) -> Result<R, MeetingsError>,
    {
        let mut guard = self.repository.lock().map_err(|error| error.to_string())?;
        if guard.is_none() {
            if !self.db_path.exists() {
                return Err("meetings database not yet available; start a meeting first".to_owned());
            }
            *guard =
                Some(MeetingsRepository::open(&self.db_path).map_err(|error| error.to_string())?);
        }
        let repo = guard.as_mut().expect("repository initialized");
        f(repo).map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub fn list_meetings(
    limit: Option<i64>,
    state: State<'_, MeetingsState>,
) -> Result<Vec<MeetingSummary>, String> {
    state.ensure(|repo| repo.list_meetings(limit.unwrap_or(50)))
}

#[tauri::command]
pub fn list_meeting_segments(
    meeting_id: String,
    state: State<'_, MeetingsState>,
) -> Result<Vec<MeetingSegment>, String> {
    state.ensure(|repo| repo.list_meeting_segments(&meeting_id))
}

#[tauri::command]
pub fn delete_meeting(meeting_id: String, state: State<'_, MeetingsState>) -> Result<(), String> {
    state.ensure(|repo| repo.delete_meeting(&meeting_id))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::MeetingsState;

    #[test]
    fn opens_the_database_when_the_sidecar_creates_it_after_startup() {
        let directory = tempdir().unwrap();
        let db_path = directory.path().join("lma.db");
        let state = MeetingsState {
            db_path: db_path.clone(),
            repository: Mutex::new(None),
        };

        let error = state
            .ensure(|repository| repository.list_meetings(10))
            .unwrap_err();
        assert_eq!(
            error,
            "meetings database not yet available; start a meeting first"
        );

        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE meetings (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    source TEXT NOT NULL,
                    platform TEXT NOT NULL,
                    started_at INTEGER NOT NULL,
                    ended_at INTEGER,
                    status TEXT NOT NULL,
                    duration_ms INTEGER
                );
                CREATE TABLE segments (
                    segment_id TEXT PRIMARY KEY,
                    meeting_id TEXT NOT NULL,
                    channel TEXT NOT NULL,
                    speaker TEXT,
                    start_ms INTEGER NOT NULL,
                    end_ms INTEGER NOT NULL,
                    text TEXT NOT NULL,
                    is_partial INTEGER NOT NULL
                );",
            )
            .unwrap();
        drop(connection);

        let meetings = state
            .ensure(|repository| repository.list_meetings(10))
            .unwrap();
        assert!(meetings.is_empty());
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingsStateStatus {
    pub available: bool,
}
