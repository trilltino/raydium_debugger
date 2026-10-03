//! Private migrations operations; blocking operator tooling.
use super::*;
pub(super) fn initialize_schema(connection: &Connection) -> anyhow::Result<()> {
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
    let transaction = connection.unchecked_transaction()?;
    initialize_schema_inner(&transaction)?;
    transaction.commit()?;
    Ok(())
}
pub(super) fn initialize_schema_inner(connection: &Connection) -> anyhow::Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        bail!("support database schema {version} is newer than supported {SCHEMA_VERSION}");
    }
    if version == 0 {
        connection.execute_batch(
            "
            CREATE TABLE corpora (
                corpus_id TEXT PRIMARY KEY,
                source_type TEXT NOT NULL
            );
            CREATE TABLE source_files (
                source_file_id INTEGER PRIMARY KEY,
                corpus_id TEXT NOT NULL REFERENCES corpora(corpus_id),
                file_name TEXT NOT NULL,
                sha256 TEXT NOT NULL,
                imported_at INTEGER NOT NULL,
                UNIQUE (corpus_id, file_name, sha256)
            );
            CREATE TABLE message_revisions (
                revision_id INTEGER PRIMARY KEY,
                corpus_id TEXT NOT NULL REFERENCES corpora(corpus_id),
                source_file_id INTEGER NOT NULL REFERENCES source_files(source_file_id),
                source_message_id TEXT NOT NULL,
                sender TEXT,
                date_title TEXT,
                body TEXT NOT NULL,
                reply_to_message_id TEXT,
                media_refs_json TEXT NOT NULL,
                UNIQUE (source_file_id, source_message_id)
            );
            CREATE INDEX message_revisions_by_corpus_message
                ON message_revisions(corpus_id, source_message_id);
            INSERT INTO corpora (corpus_id, source_type) VALUES
                ('support', 'private_support'),
                ('announcements', 'public_announcement');
            PRAGMA user_version = 1;
            ",
        )?;
    }

    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 1 {
        connection.execute_batch(
            "
            CREATE TABLE reply_edges (
                source_revision_id INTEGER PRIMARY KEY
                    REFERENCES message_revisions(revision_id) ON DELETE CASCADE,
                corpus_id TEXT NOT NULL REFERENCES corpora(corpus_id),
                target_source_message_id TEXT NOT NULL,
                target_revision_id INTEGER REFERENCES message_revisions(revision_id),
                status TEXT NOT NULL CHECK (status IN ('resolved', 'unresolved', 'ambiguous'))
            );
            CREATE INDEX reply_edges_by_target
                ON reply_edges(corpus_id, target_source_message_id);
            PRAGMA user_version = 2;
            ",
        )?;
    }
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 2 {
        connection.execute_batch(
            "
            CREATE TABLE entities (
                entity_id INTEGER PRIMARY KEY,
                revision_id INTEGER NOT NULL
                    REFERENCES message_revisions(revision_id) ON DELETE CASCADE,
                entity_type TEXT NOT NULL,
                canonical_value TEXT NOT NULL,
                surface_form TEXT NOT NULL,
                decoded_length INTEGER,
                extraction_version INTEGER NOT NULL,
                UNIQUE (revision_id, entity_type, canonical_value, surface_form)
            );
            CREATE INDEX entities_by_value
                ON entities(entity_type, canonical_value);
            CREATE TABLE knowledge_metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            PRAGMA user_version = 3;
            ",
        )?;
    }
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 3 {
        connection.execute_batch(
            "
            CREATE TABLE candidate_cases (
                case_id TEXT PRIMARY KEY,
                corpus_id TEXT NOT NULL REFERENCES corpora(corpus_id),
                review_status TEXT NOT NULL
                    CHECK (review_status IN ('candidate', 'approved', 'rejected')),
                is_current INTEGER NOT NULL CHECK (is_current IN (0, 1)),
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE candidate_case_messages (
                case_id TEXT NOT NULL REFERENCES candidate_cases(case_id),
                revision_id INTEGER NOT NULL REFERENCES message_revisions(revision_id),
                link_reason TEXT NOT NULL,
                PRIMARY KEY (case_id, revision_id)
            );
            CREATE INDEX candidate_case_messages_by_revision
                ON candidate_case_messages(revision_id);
            CREATE TABLE review_events (
                review_event_id INTEGER PRIMARY KEY,
                case_id TEXT NOT NULL REFERENCES candidate_cases(case_id),
                action TEXT NOT NULL CHECK (action IN ('approve', 'reject')),
                reviewer TEXT NOT NULL,
                rationale TEXT NOT NULL,
                reviewed_at INTEGER NOT NULL
            );
            PRAGMA user_version = 4;
            ",
        )?;
    }
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 4 {
        connection.execute_batch(
            "
            ALTER TABLE candidate_cases
                ADD COLUMN case_origin TEXT NOT NULL DEFAULT 'automatic'
                CHECK (case_origin IN ('automatic', 'manual'));
            CREATE TABLE manual_case_memberships (
                corpus_id TEXT NOT NULL REFERENCES corpora(corpus_id),
                file_name TEXT NOT NULL,
                source_message_id TEXT NOT NULL,
                case_id TEXT NOT NULL REFERENCES candidate_cases(case_id),
                assigned_at INTEGER NOT NULL,
                PRIMARY KEY (corpus_id, file_name, source_message_id)
            );
            CREATE INDEX manual_case_memberships_by_case
                ON manual_case_memberships(case_id);
            CREATE TABLE case_override_events (
                override_event_id INTEGER PRIMARY KEY,
                action TEXT NOT NULL CHECK (action IN ('merge', 'split')),
                source_case_ids_json TEXT NOT NULL,
                target_case_ids_json TEXT NOT NULL,
                reviewer TEXT NOT NULL,
                rationale TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            PRAGMA user_version = 5;
            ",
        )?;
    }
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 5 {
        connection.execute_batch(
            "
            CREATE TABLE transaction_snapshots (
                snapshot_id INTEGER PRIMARY KEY,
                signature TEXT NOT NULL,
                cluster TEXT NOT NULL CHECK (cluster IN ('mainnet', 'devnet')),
                source_revision_id INTEGER NOT NULL
                    REFERENCES message_revisions(revision_id),
                captured_at INTEGER NOT NULL,
                observation_status TEXT NOT NULL,
                diagnostic_json TEXT,
                error TEXT,
                UNIQUE (signature, cluster)
            );
            CREATE INDEX transaction_snapshots_by_source
                ON transaction_snapshots(source_revision_id);
            PRAGMA user_version = 6;
            ",
        )?;
    }
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 6 {
        connection.execute_batch(
            "
            CREATE TABLE case_annotations (
                case_id TEXT PRIMARY KEY REFERENCES candidate_cases(case_id),
                product TEXT NOT NULL,
                failure_domain TEXT NOT NULL,
                summary TEXT NOT NULL,
                resolution TEXT NOT NULL,
                symptom_tags_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            PRAGMA user_version = 7;
            ",
        )?;
    }
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 7 || version == 8 {
        // Existing operational tables are retired only by explicit backed-up migration.
        connection.execute_batch("PRAGMA user_version=9;")?;
    }
    if connection.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? == 9 {
        connection.execute_batch(
            "ALTER TABLE case_annotations ADD COLUMN curated_json TEXT;
            ALTER TABLE case_annotations ADD COLUMN annotation_reviewer TEXT;
            PRAGMA user_version=10;",
        )?;
    }

    Ok(())
}
