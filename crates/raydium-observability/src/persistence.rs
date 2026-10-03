//! Isolated, versioned SQLite storage. Legacy retirement follows durable copy and verification.
use anyhow::{bail, Context};
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Operational database path; the private support setting is deliberately ignored.
pub fn default_path() -> PathBuf {
    std::env::var_os("RAYDIUM_DEBUGGER_OBSERVATIONS_DATABASE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".raydium-debugger/observations.sqlite"))
}

/// Creates the observation schema transactionally. Call only on a blocking thread.
/// Timestamps use Unix seconds; missing transaction metadata remains SQL NULL.
pub fn initialize_schema(connection: &Connection) -> anyhow::Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
    let transaction = connection.unchecked_transaction()?;
    let version: i64 = transaction.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    anyhow::ensure!(
        version <= 1,
        "observation database is newer than this application"
    );
    if version == 0 {
        transaction.execute_batch("CREATE TABLE IF NOT EXISTS recent_observations (
            observation_id INTEGER PRIMARY KEY, dedup_key TEXT NOT NULL UNIQUE,
            source TEXT NOT NULL, source_id TEXT NOT NULL,
            cluster TEXT NOT NULL CHECK(cluster IN ('mainnet','devnet')),
            observed_at INTEGER NOT NULL, slot INTEGER, signature TEXT, program_id TEXT,
            instruction TEXT, error_code TEXT, fingerprint TEXT NOT NULL,
            logs_json TEXT NOT NULL, logs_text TEXT NOT NULL, imported_at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS recent_observations_by_time ON recent_observations(cluster,observed_at DESC);
            CREATE INDEX IF NOT EXISTS recent_observations_by_fingerprint ON recent_observations(fingerprint,observed_at DESC);
            CREATE TABLE IF NOT EXISTS observation_collectors (
                collector_key TEXT PRIMARY KEY,state_json TEXT NOT NULL,updated_at INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS migration_receipts (
                source_path TEXT PRIMARY KEY,observation_count INTEGER NOT NULL,collector_count INTEGER NOT NULL);
            PRAGMA user_version=1;")?;
    }
    transaction.commit()?;
    Ok(())
}

/// Backs up a private legacy database, copies stable observation IDs and checkpoints,
/// verifies all columns in both directions, then retires only the legacy operational
/// tables. Each phase is restartable; review and archive tables are never changed.
/// Requires exclusive operator access to the source during migration. Blocking I/O.
pub fn migrate_legacy(source: &Path, destination: &Path) -> anyhow::Result<()> {
    migrate_with_hook(source, destination, |_| Ok(()))
}

fn migrate_with_hook(
    source: &Path,
    destination: &Path,
    hook: impl Fn(&str) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    anyhow::ensure!(source.is_file(), "legacy database does not exist");
    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let source = source.canonicalize()?;
    anyhow::ensure!(
        destination.canonicalize().ok().as_ref() != Some(&source),
        "source and destination must differ"
    );
    let mut legacy = Connection::open(&source)?;
    legacy.busy_timeout(Duration::from_secs(5))?;
    let has_legacy: bool = legacy.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='recent_observations')",
        [],
        |r| r.get(0),
    )?;
    let target = Connection::open(destination)?;
    initialize_schema(&target)?;
    if !has_legacy {
        let receipt: bool = target.query_row(
            "SELECT EXISTS(SELECT 1 FROM migration_receipts WHERE source_path=?1)",
            [source.to_string_lossy().as_ref()],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            receipt,
            "legacy tables missing without a verified migration receipt"
        );
        return Ok(());
    }
    let backup_path = source.with_extension("pre-observations-migration.sqlite");
    if !backup_path.exists() {
        legacy
            .backup("main", &backup_path, None)
            .context("creating private SQLite migration backup")?;
    }
    hook("backup")?;
    target.execute(
        "ATTACH DATABASE ?1 AS legacy",
        [source.to_string_lossy().as_ref()],
    )?;
    let has_collectors: bool = target.query_row(
        "SELECT EXISTS(SELECT 1 FROM legacy.sqlite_master WHERE name='observation_collectors')",
        [],
        |r| r.get(0),
    )?;
    let transaction = target.unchecked_transaction()?;
    transaction.execute_batch("INSERT INTO recent_observations SELECT * FROM legacy.recent_observations WHERE true ON CONFLICT DO NOTHING;")?;
    if has_collectors {
        transaction.execute_batch("INSERT INTO observation_collectors SELECT * FROM legacy.observation_collectors WHERE true ON CONFLICT DO NOTHING;")?;
    }
    for table in ["recent_observations", "observation_collectors"] {
        if table == "observation_collectors" && !has_collectors {
            continue;
        }
        verify(&transaction, table)?;
    }
    transaction.commit()?;
    hook("copy")?;
    target.execute_batch("DETACH DATABASE legacy;")?;
    // The source write lock prevents a collector moving a checkpoint between the
    // final verification and retirement. A crash before retirement keeps the source.
    let retirement = legacy.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    retirement.execute(
        "ATTACH DATABASE ?1 AS operational",
        [destination.to_string_lossy().as_ref()],
    )?;
    let observations: i64 =
        retirement.query_row("SELECT count(*) FROM recent_observations", [], |r| r.get(0))?;
    let collectors: i64 = if has_collectors {
        retirement.query_row("SELECT count(*) FROM observation_collectors", [], |r| {
            r.get(0)
        })?
    } else {
        0
    };
    for table in ["recent_observations", "observation_collectors"] {
        if table == "observation_collectors" && !has_collectors {
            continue;
        }
        let difference: bool = retirement.query_row(&format!("SELECT EXISTS(SELECT * FROM main.{table} EXCEPT SELECT * FROM operational.{table}) OR EXISTS(SELECT * FROM operational.{table} EXCEPT SELECT * FROM main.{table})"), [], |r| r.get(0))?;
        anyhow::ensure!(
            !difference,
            "legacy source changed during migration; rerun after stopping collectors"
        );
    }
    // Receipt is committed before destructive source retirement. A rerun verifies
    // the data again if legacy tables remain, rather than trusting this receipt.
    target.execute(
        "INSERT OR REPLACE INTO migration_receipts VALUES (?1,?2,?3)",
        rusqlite::params![source.to_string_lossy().as_ref(), observations, collectors],
    )?;
    retirement.execute_batch("DROP TABLE recent_observations;")?;
    if has_collectors {
        retirement.execute_batch("DROP TABLE observation_collectors;")?;
    }
    retirement.commit()?;
    Ok(())
}

fn verify(connection: &Connection, table: &str) -> anyhow::Result<()> {
    let different: bool = connection.query_row(&format!("SELECT EXISTS(SELECT * FROM legacy.{table} EXCEPT SELECT * FROM main.{table}) OR EXISTS(SELECT * FROM main.{table} EXCEPT SELECT * FROM legacy.{table})"), [], |r| r.get(0))?;
    if different {
        bail!("migration identity/count verification failed for {table}; legacy tables retained");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_copy_reruns_preserve_ids_checkpoints_and_reviews() {
        let directory = std::env::temp_dir().join(format!(
            "observation-migration-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("private.sqlite");
        let target = directory.join("operational.sqlite");
        let legacy = Connection::open(&source).unwrap();
        initialize_schema(&legacy).unwrap();
        legacy.execute_batch("CREATE TABLE review_events(value TEXT); INSERT INTO review_events VALUES('preserved');
            INSERT INTO recent_observations VALUES(42,'dedup','rpc','source','devnet',1,NULL,NULL,NULL,NULL,NULL,'fingerprint','[]','',1);
            INSERT INTO observation_collectors VALUES('checkpoint','{\"before\":\"stable\"}',1);").unwrap();
        drop(legacy);
        assert!(migrate_with_hook(&source, &target, |stage| {
            anyhow::ensure!(stage != "copy", "simulated interruption");
            Ok(())
        })
        .is_err());
        migrate_legacy(&source, &target).unwrap();
        migrate_legacy(&source, &target).unwrap();
        let destination = Connection::open(&target).unwrap();
        assert_eq!(
            destination
                .query_row("SELECT observation_id FROM recent_observations", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            42
        );
        assert_eq!(
            destination
                .query_row("SELECT state_json FROM observation_collectors", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "{\"before\":\"stable\"}"
        );
        let legacy = Connection::open(&source).unwrap();
        assert_eq!(
            legacy
                .query_row("SELECT value FROM review_events", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "preserved"
        );
        drop(destination);
        drop(legacy);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
