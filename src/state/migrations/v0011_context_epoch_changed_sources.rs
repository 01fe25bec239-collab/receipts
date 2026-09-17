//! Ordered caller-supplied changed sources, with explicit capture/presence.
//! The default serves future four-field appends only. All existing rows are
//! explicitly marked uncaptured in this same migration transaction.

use super::Migration;

pub(crate) const MIGRATION: Migration = Migration {
    version: 11,
    name: "context_epoch_changed_sources",
    sql: "ALTER TABLE context_epoch ADD COLUMN changed_sources_present INTEGER
    DEFAULT 0 CHECK (changed_sources_present IN (0, 1));
UPDATE context_epoch SET changed_sources_present = NULL;
CREATE TABLE context_epoch_changed_source (
    project_id TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    source_ordinal INTEGER NOT NULL CHECK (source_ordinal >= 0),
    ref_type TEXT NOT NULL CHECK (ref_type IN ('REPO_PATH', 'STATE_QUERY', 'ARTIFACT_ID', 'URL')),
    target TEXT NOT NULL CHECK (length(CAST(target AS BLOB)) > 0),
    digest TEXT,
    section TEXT,
    PRIMARY KEY (project_id, epoch, source_ordinal),
    FOREIGN KEY (project_id, epoch) REFERENCES context_epoch (project_id, epoch)
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (11, 'context_epoch_changed_sources');",
};
