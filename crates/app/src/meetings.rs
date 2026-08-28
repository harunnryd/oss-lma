//! Read-only access to the sidecar's meetings + segments SQLite tables.
//!
//! The Python sidecar owns this database and is the only writer. The
//! Rust app reads from it via the same SQLite file in `app_data_dir`
//! to power the History page in the desktop UI. All queries are
//! SELECTs; deletes go through `delete_meeting` which performs a
//! cascading delete inside a single transaction.

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};

#[derive(Debug)]
pub enum MeetingsError {
    DatabaseMissing(PathBuf),
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
}

impl fmt::Display for MeetingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatabaseMissing(path) => write!(
                formatter,
                "meetings database not found at {}; start a meeting first to create it",
                path.display()
            ),
            Self::Sqlite(error) => write!(formatter, "sqlite error: {error}"),
            Self::Io(error) => write!(formatter, "io error: {error}"),
        }
    }
}

impl std::error::Error for MeetingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::DatabaseMissing(_) => None,
        }
    }
}

impl From<rusqlite::Error> for MeetingsError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<std::io::Error> for MeetingsError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_ms: Option<i64>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSegment {
    pub segment_id: String,
    pub meeting_id: String,
    pub channel: String,
    pub speaker: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub transcript: String,
    pub is_partial: bool,
}

pub struct MeetingsRepository {
    connection: Connection,
}

impl MeetingsRepository {
    pub fn open(db_path: &Path) -> Result<Self, MeetingsError> {
        if !db_path.exists() {
            return Err(MeetingsError::DatabaseMissing(db_path.to_path_buf()));
        }
        let connection = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self { connection })
    }

    pub fn list_meetings(&self, limit: i64) -> Result<Vec<MeetingSummary>, MeetingsError> {
        let limit = limit.clamp(1, 500);
        let mut statement = self.connection.prepare(
            "SELECT id, title, status, started_at, ended_at, duration_ms
             FROM meetings
             ORDER BY started_at DESC
             LIMIT ?",
        )?;
        let rows = statement.query_map(params![limit], |row| {
            Ok(MeetingSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                duration_ms: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn list_meeting_segments(&self, meeting_id: &str) -> Result<Vec<MeetingSegment>, MeetingsError> {
        let mut statement = self.connection.prepare(
            "SELECT segment_id, meeting_id, channel, speaker, start_ms, end_ms, transcript, is_partial
             FROM segments
             WHERE meeting_id = ?
             ORDER BY start_ms ASC",
        )?;
        let rows = statement.query_map(params![meeting_id], |row| {
            let is_partial_int: i64 = row.get(7)?;
            Ok(MeetingSegment {
                segment_id: row.get(0)?,
                meeting_id: row.get(1)?,
                channel: row.get(2)?,
                speaker: row.get(3)?,
                start_ms: row.get(4)?,
                end_ms: row.get(5)?,
                transcript: row.get(6)?,
                is_partial: is_partial_int != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn delete_meeting(&mut self, meeting_id: &str) -> Result<(), MeetingsError> {
        // Every child table declared in python/sidecar/storage/migrations
        // has ON DELETE CASCADE on meetings(id), so a single DELETE
        // clears segments, summaries, action_items, rag_chunks,
        // agent_assists, agent_tokens and thinking_steps without
        // having to name them here.
        self.connection.execute("DELETE FROM meetings WHERE id = ?", params![meeting_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bootstrap() -> Connection {
        let connection = Connection::open_in_memory().expect("in-memory db");
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
                    duration_ms INTEGER,
                    time_offset_ms INTEGER NOT NULL DEFAULT 0,
                    reconnect_attempts INTEGER NOT NULL DEFAULT 0,
                    last_reconnect_at INTEGER
                );
                CREATE TABLE segments (
                    segment_id TEXT PRIMARY KEY,
                    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                    channel TEXT NOT NULL,
                    speaker TEXT,
                    start_ms INTEGER NOT NULL,
                    end_ms INTEGER NOT NULL,
                    transcript TEXT NOT NULL,
                    is_partial INTEGER NOT NULL
                );",
            )
            .expect("schema");
        connection
    }

    #[test]
    fn lists_meetings_in_started_at_desc_order() {
        let connection = bootstrap();
        connection
            .execute(
                "INSERT INTO meetings (id, title, source, platform, started_at, status)
                 VALUES ('m1', 'first', 'LOCAL', 'local', 1000, 'COMPLETED'),
                        ('m2', 'second', 'LOCAL', 'local', 2000, 'COMPLETED')",
                [],
            )
            .unwrap();
        // Open via repository to exercise the same code path.
        let tmp = tempfile_like_path(&connection);
        let repo = MeetingsRepository::open(&tmp).unwrap();
        let meetings = repo.list_meetings(10).unwrap();
        assert_eq!(meetings.len(), 2);
        assert_eq!(meetings[0].id, "m2");
        assert_eq!(meetings[1].id, "m1");
    }

    #[test]
    fn delete_meeting_cascades_to_segments() {
        let connection = bootstrap();
        connection
            .execute(
                "INSERT INTO meetings (id, title, source, platform, started_at, status)
                 VALUES ('m1', 'first', 'LOCAL', 'local', 1000, 'COMPLETED')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO segments (segment_id, meeting_id, channel, start_ms, end_ms, transcript, is_partial)
                 VALUES ('s1', 'm1', 'CALLER', 0, 1000, 'hello', 0)",
                [],
            )
            .unwrap();
        let tmp = tempfile_like_path(&connection);
        let mut repo = MeetingsRepository::open(&tmp).unwrap();
        repo.delete_meeting("m1").unwrap();
        let meetings = repo.list_meetings(10).unwrap();
        let segments = repo.list_meeting_segments("m1").unwrap();
        assert_eq!(meetings.len(), 0);
        assert_eq!(segments.len(), 0);
    }

    /// Writes the seed connection's inserts into a fresh on-disk file
    /// with the same schema so the repository (which opens by path)
    /// can read the data without requiring the rusqlite `backup` feature.
    fn tempfile_like_path(seed: &Connection) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir()
            .join(format!("oss-lma-meetings-{}-{}-{}", std::process::id(), n, line!()))
            .join("lma.db");
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = Connection::open(&path).unwrap();
        file.execute_batch(
            "CREATE TABLE meetings (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, source TEXT NOT NULL,
                platform TEXT NOT NULL, started_at INTEGER NOT NULL,
                ended_at INTEGER, status TEXT NOT NULL, duration_ms INTEGER,
                time_offset_ms INTEGER NOT NULL DEFAULT 0,
                reconnect_attempts INTEGER NOT NULL DEFAULT 0, last_reconnect_at INTEGER
            );
            CREATE TABLE segments (
                segment_id TEXT PRIMARY KEY,
                meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                channel TEXT NOT NULL, speaker TEXT,
                start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                transcript TEXT NOT NULL, is_partial INTEGER NOT NULL
            );",
        )
        .unwrap();
        let mut stmt = seed.prepare("SELECT id, title, source, platform, started_at, ended_at, status, duration_ms FROM meetings").unwrap();
        let meetings: Vec<_> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for (id, title, source, platform, started_at, ended_at, status, duration_ms) in &meetings {
            file.execute(
                "INSERT INTO meetings (id, title, source, platform, started_at, ended_at, status, duration_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![id, title, source, platform, started_at, ended_at, status, duration_ms],
            )
            .unwrap();
        }
        let mut stmt = seed.prepare("SELECT segment_id, meeting_id, channel, speaker, start_ms, end_ms, transcript, is_partial FROM segments").unwrap();
        let segments: Vec<_> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for (segment_id, meeting_id, channel, speaker, start_ms, end_ms, transcript, is_partial) in &segments {
            file.execute(
                "INSERT INTO segments (segment_id, meeting_id, channel, speaker, start_ms, end_ms, transcript, is_partial) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![segment_id, meeting_id, channel, speaker, start_ms, end_ms, transcript, is_partial],
            )
            .unwrap();
        }
        path
    }
}
