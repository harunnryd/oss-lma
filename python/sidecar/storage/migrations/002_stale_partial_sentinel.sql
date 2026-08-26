CREATE TABLE segments_new (
  segment_id      TEXT    PRIMARY KEY,
  meeting_id      TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  channel         TEXT    NOT NULL
                          CHECK (channel IN ('CALLER', 'AGENT', 'AGENT_ASSISTANT')),
  speaker         TEXT,
  start_ms        INTEGER NOT NULL,
  end_ms          INTEGER NOT NULL,
  text            TEXT    NOT NULL,
  original_text   TEXT    NOT NULL,
  is_partial      INTEGER NOT NULL CHECK (is_partial IN (0, 1, -1)),
  sentiment_score REAL,
  CHECK (end_ms >= start_ms)
);

INSERT INTO segments_new SELECT * FROM segments;

DROP TABLE segments;

ALTER TABLE segments_new RENAME TO segments;

CREATE INDEX idx_segments_meeting_id_end_ms
  ON segments (meeting_id, end_ms);