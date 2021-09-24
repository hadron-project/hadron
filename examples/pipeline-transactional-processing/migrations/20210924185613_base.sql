-- This table is used to materialize events from a Hadron Pipeline, including all stages.
--
-- The primary key on this table is used to guard against duplicates. All updates
-- involving application state should be made within a transaction, and the event's
-- `id` & `source` fields, and the stage name should be inserted into this table as a
-- new row to indicate that the event has been processed. Future attempts to process a
-- duplicate will cause a primary key violation.
CREATE TABLE IF NOT EXISTS in_table_pipelines (
    id TEXT NOT NULL,
    source TEXT NOT NULL,
    stage TEXT NOT NULL,
    time TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event BYTEA NOT NULL,
    PRIMARY KEY (id, source, stage)
);
