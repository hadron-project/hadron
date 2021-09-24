-- This table is used to materialize events from the Hadron Stream.
--
-- The primary key on this table is used to guard against duplicates. All updates
-- involving application state should be made within a transaction, and the event's
-- `id` & `source` fields should be inserted into this table as a new row to indicate
-- that the event has been processed. Future attempts to process a duplicate will cause
-- a primary key violation.
CREATE TABLE IF NOT EXISTS in_table (
    id TEXT NOT NULL,
    source TEXT NOT NULL,
    PRIMARY KEY (id, source)
);
