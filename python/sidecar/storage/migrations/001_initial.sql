CREATE TABLE meetings (
  id          TEXT    PRIMARY KEY,
  title       TEXT    NOT NULL DEFAULT '',
  source      TEXT    NOT NULL CHECK (source IN ('LOCAL', 'VP')),
  platform    TEXT    NOT NULL DEFAULT 'local'
                      CHECK (platform IN ('local', 'zoom', 'meet')),
  status      TEXT    NOT NULL DEFAULT 'RECORDING'
                      CHECK (status IN ('RECORDING', 'FINALIZING',
                                        'COMPLETED', 'FAILED')),
  started_at  INTEGER NOT NULL,
  ended_at    INTEGER,
  duration_ms INTEGER,
  audio_path  TEXT,
  video_path  TEXT,
  CHECK (ended_at IS NULL OR ended_at >= started_at)
);

CREATE INDEX idx_meetings_started_at
  ON meetings (started_at DESC);

CREATE TABLE segments (
  segment_id      TEXT    PRIMARY KEY,
  meeting_id      TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  channel         TEXT    NOT NULL
                          CHECK (channel IN ('CALLER', 'AGENT', 'AGENT_ASSISTANT')),
  speaker         TEXT,
  start_ms        INTEGER NOT NULL,
  end_ms          INTEGER NOT NULL,
  text            TEXT    NOT NULL,
  original_text   TEXT    NOT NULL,
  is_partial      INTEGER NOT NULL CHECK (is_partial IN (0, 1)),
  sentiment_score REAL,
  CHECK (end_ms >= start_ms)
);

CREATE INDEX idx_segments_meeting_id_end_ms
  ON segments (meeting_id, end_ms);

CREATE TABLE summaries (
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  section    TEXT NOT NULL,
  content    TEXT NOT NULL,
  PRIMARY KEY (meeting_id, section)
);

CREATE TABLE action_items (
  action_item_id    TEXT    PRIMARY KEY,
  meeting_id        TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  description       TEXT    NOT NULL,
  detail            TEXT    NOT NULL DEFAULT '',
  owner             TEXT    NOT NULL DEFAULT 'TBD',
  due_date          TEXT    CHECK (due_date IS NULL
                                    OR date(due_date) = due_date),
  status            TEXT    NOT NULL DEFAULT 'open'
                             CHECK (status IN ('open', 'done')),
  source_segment_id TEXT    REFERENCES segments(segment_id) ON DELETE SET NULL
);

CREATE INDEX idx_action_items_meeting_id_status
  ON action_items (meeting_id, status);

CREATE INDEX idx_action_items_source_segment_id
  ON action_items (source_segment_id);

CREATE VIRTUAL TABLE rag_chunks_vec USING vec0(
  chunk_id  TEXT PRIMARY KEY,
  embedding FLOAT[384] distance_metric=cosine
);

CREATE TABLE rag_chunks (
  id         TEXT    PRIMARY KEY,
  meeting_id TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  text       TEXT    NOT NULL,
  start_ms   INTEGER,
  end_ms     INTEGER,
  channel    TEXT    NOT NULL
                     CHECK (channel IN ('CALLER', 'AGENT', 'AGENT_ASSISTANT',
                                        'DOC')),
  speaker    TEXT,
  created_at INTEGER NOT NULL,
  CHECK ((start_ms IS NULL AND end_ms IS NULL)
         OR (start_ms IS NOT NULL AND end_ms IS NOT NULL
             AND end_ms >= start_ms))
);

CREATE INDEX idx_rag_chunks_meeting_id_channel
  ON rag_chunks (meeting_id, channel);

CREATE TABLE vp_tasks (
  id           TEXT    PRIMARY KEY,
  schedule_id  TEXT    REFERENCES vp_schedules(id) ON DELETE SET NULL,
  meeting_url  TEXT    NOT NULL,
  state        TEXT    NOT NULL DEFAULT 'PENDING'
                       CHECK (state IN ('PENDING', 'LAUNCHING', 'JOINING',
                                        'IN_MEETING', 'AWAITING_ACTION',
                                        'FINALIZING', 'DONE', 'FAILED')),
  container_id TEXT,
  started_at   INTEGER NOT NULL,
  ended_at     INTEGER,
  CHECK (ended_at IS NULL OR ended_at >= started_at)
);

CREATE INDEX idx_vp_tasks_state_started_at
  ON vp_tasks (state, started_at);

CREATE INDEX idx_vp_tasks_schedule_id
  ON vp_tasks (schedule_id);
