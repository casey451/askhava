-- privacy (T092): retention holds and the purge's deletes (PRIV-007, PRIV-009, PRIV-011; D-013, D-079, D-491).
-- privacy.retention_hold is one row per dispute or complaint that keeps a closed job's contact
-- details past 180 days: payments places a dispute's hold at purge time, and businesses' hold and
-- release events place and release a complaint's (D-491). A hold is released, never deleted;
-- IDs and times only, no contact value (PRIV-002). The table is new and empty, so nothing blocks.
-- The photo purge (30 days) and "delete my data" remove privacy.photo rows, whose links go with
-- them (ON DELETE CASCADE), so the app role gets DELETE on both photo tables here, as 0125 said.

CREATE TABLE privacy.retention_hold (
  source text NOT NULL CHECK (source IN ('dispute', 'complaint')),
  source_id uuid NOT NULL,
  job_id uuid NOT NULL,
  placed_at timestamptz NOT NULL,
  released_at timestamptz CHECK (released_at IS NULL OR released_at >= placed_at),
  PRIMARY KEY (source, source_id)
);
CREATE INDEX retention_hold_open_idx ON privacy.retention_hold (job_id) WHERE released_at IS NULL;

GRANT UPDATE (released_at) ON privacy.retention_hold TO askhava_app;
GRANT DELETE ON privacy.photo, privacy.photo_link TO askhava_app;
