ALTER TABLE meetings
  ADD COLUMN time_offset_ms INTEGER NOT NULL DEFAULT 0;

ALTER TABLE meetings
  ADD COLUMN reconnect_attempts INTEGER NOT NULL DEFAULT 0;

ALTER TABLE meetings
  ADD COLUMN last_reconnect_at INTEGER;
