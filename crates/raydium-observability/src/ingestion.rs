//! Bounded private JSON/NDJSON imports; one import or collector page commits atomically.
use super::persistence;
use anyhow::{anyhow, bail, Context};
use raydium_debugger::RpcCluster;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(feature = "collection")]
mod collector;
const MAX_OBSERVATIONS_PER_IMPORT: usize = 10_000;
const MAX_OBSERVATION_LOG_BYTES: usize = 64 * 1024;
const RECENT_OBSERVATION_RETENTION_SECS: i64 = 30 * 24 * 60 * 60;
const MAX_SEARCH_RESULTS: usize = 100;
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
struct RecentObservationInput {
    source: String,
    source_id: String,
    cluster: String,
    observed_at: i64,
    #[serde(default)]
    slot: Option<u64>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    program_id: Option<String>,
    #[serde(default)]
    instruction: Option<String>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    logs: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ObservationIngestStats {
    processed: usize,
    pruned: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct RecentObservationHit {
    source: String,
    source_id: String,
    cluster: String,
    observed_at: i64,
    slot: Option<u64>,
    signature: Option<String>,
    program_id: Option<String>,
    instruction: Option<String>,
    error_code: Option<String>,
    fingerprint: String,
    logs: Vec<String>,
}

#[derive(Debug)]
struct ObservationSearchOptions {
    database_path: PathBuf,
    cluster: Option<String>,
    window_seconds: i64,
    limit: usize,
}

/// Run.
pub fn run(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        #[cfg(feature = "collection")]
        Some("collect-rpc") => collector::run(&args[1..]),
        Some("migrate-legacy") => {
            anyhow::ensure!(args.len() == 3, "usage: observations migrate-legacy <support-db> <observations-db>");
            persistence::migrate_legacy(Path::new(&args[1]), Path::new(&args[2]))
        },
        Some("ingest-json") => {
            if args.len() < 2 || args.len() > 3 {
                bail!("usage: support-knowledge observations ingest-json <file.json|file.ndjson> [database-path]");
            }
            let input_path = PathBuf::from(&args[1]);
            let database_path = observation_database_arg(args.get(2))?;
            let mut connection = open_observation_database(&database_path)?;
            persistence::initialize_schema(&connection)?;
            let observations = read_observation_file(&input_path)?;
            let stats = ingest_recent_observations(&mut connection, &observations)?;
            println!(
                "Observations processed: {}; expired records pruned: {}.",
                stats.processed, stats.pruned
            );
            Ok(())
        }
        Some("search") => {
            let query = args
                .get(1)
                .ok_or_else(|| anyhow!("missing observation search query"))?;
            let options = parse_observation_search_options(&args[2..])?;
            let hits = search_recent_observations(query, &options)?;
            if hits.is_empty() {
                println!("No recent observations matched.");
                return Ok(());
            }
            for hit in hits {
                println!(
                    "[{}] {} {} slot={:?} {} {} fingerprint={}",
                    hit.cluster,
                    hit.source,
                    hit.source_id,
                    hit.slot,
                    hit.observation_status_label(),
                    hit.error_code.as_deref().unwrap_or_default(),
                    hit.fingerprint
                );
                if let Some(signature) = hit.signature {
                    println!("  signature {signature}");
                }
                if let Some(program_id) = hit.program_id {
                    println!("  program {program_id}");
                }
                println!("  {}", message_preview(&hit.logs.join(" ")));
            }
            Ok(())
        }
        _ => bail!(
            "usage: cargo run -p xtask -- support-knowledge observations <ingest-json|search|collect-rpc> ..."
        ),
    }
}

impl RecentObservationHit {
    fn observation_status_label(&self) -> &str {
        self.instruction.as_deref().unwrap_or("observation")
    }
}

fn parse_observation_search_options(args: &[String]) -> anyhow::Result<ObservationSearchOptions> {
    let mut options = ObservationSearchOptions {
        database_path: persistence::default_path(),
        cluster: None,
        window_seconds: RECENT_OBSERVATION_RETENTION_SECS,
        limit: 10,
    };
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        index += 1;
        let value = args
            .get(index)
            .ok_or_else(|| anyhow!("missing value for {option}"))?;
        match option {
            "--database" => options.database_path = PathBuf::from(value),
            "--cluster" => {
                if !matches!(value.as_str(), "mainnet" | "devnet") {
                    bail!("cluster must be mainnet or devnet");
                }
                options.cluster = Some(value.clone());
            }
            "--window-seconds" => {
                options.window_seconds = value
                    .parse::<i64>()
                    .context("window must be a positive number of seconds")?;
                if options.window_seconds <= 0
                    || options.window_seconds > RECENT_OBSERVATION_RETENTION_SECS
                {
                    bail!(
                        "window must be between 1 and {RECENT_OBSERVATION_RETENTION_SECS} seconds"
                    );
                }
            }
            "--limit" => {
                options.limit = value.parse::<usize>().context("invalid result limit")?;
                if options.limit == 0 || options.limit > MAX_SEARCH_RESULTS {
                    bail!("result limit must be between 1 and {MAX_SEARCH_RESULTS}");
                }
            }
            other => bail!("unknown observations search option {other:?}"),
        }
        index += 1;
    }
    Ok(options)
}

fn read_observation_file(path: &Path) -> anyhow::Result<Vec<RecentObservationInput>> {
    const MAX_IMPORT_FILE_BYTES: u64 = 50 * 1024 * 1024;
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?;
    if metadata.len() > MAX_IMPORT_FILE_BYTES {
        bail!("observation input exceeds {MAX_IMPORT_FILE_BYTES} bytes");
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read observation file {}", path.display()))?;
    let input = raw.trim();
    let observations = if input.starts_with('[') {
        serde_json::from_str::<Vec<RecentObservationInput>>(input)?
    } else {
        input
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str::<RecentObservationInput>)
            .collect::<Result<Vec<_>, _>>()?
    };
    if observations.len() > MAX_OBSERVATIONS_PER_IMPORT {
        bail!("an import may contain at most {MAX_OBSERVATIONS_PER_IMPORT} observations");
    }
    Ok(observations)
}

fn open_observation_database(database_path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create database directory {}", parent.display()))?;
    }
    let connection = Connection::open(database_path)
        .with_context(|| format!("failed to open database {}", database_path.display()))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    persistence::initialize_schema(&connection)?;
    Ok(connection)
}

fn ingest_recent_observations(
    connection: &mut Connection,
    observations: &[RecentObservationInput],
) -> anyhow::Result<ObservationIngestStats> {
    let transaction = connection.transaction()?;
    let stats = ingest_recent_observations_into(&transaction, observations)?;
    transaction.commit()?;
    Ok(stats)
}

fn ingest_recent_observations_into(
    connection: &Connection,
    observations: &[RecentObservationInput],
) -> anyhow::Result<ObservationIngestStats> {
    if observations.len() > MAX_OBSERVATIONS_PER_IMPORT {
        bail!("an import may contain at most {MAX_OBSERVATIONS_PER_IMPORT} observations");
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs() as i64;
    let validated = observations
        .iter()
        .map(|observation| validate_observation(observation, now))
        .collect::<anyhow::Result<Vec<_>>>()?;

    for ValidatedObservation {
        observation,
        fingerprint,
        dedup_key,
        error_code,
        logs_text,
    } in &validated
    {
        connection
            .prepare_cached(
                "INSERT INTO recent_observations (
                dedup_key, source, source_id, cluster, observed_at, slot,
                signature, program_id, instruction, error_code, fingerprint,
                logs_json, logs_text, imported_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(dedup_key) DO UPDATE SET
                observed_at = excluded.observed_at,
                slot = excluded.slot,
                signature = excluded.signature,
                program_id = excluded.program_id,
                instruction = excluded.instruction,
                error_code = excluded.error_code,
                fingerprint = excluded.fingerprint,
                logs_json = excluded.logs_json,
                logs_text = excluded.logs_text,
                imported_at = excluded.imported_at",
            )?
            .execute(params![
                dedup_key,
                observation.source,
                observation.source_id,
                observation.cluster,
                observation.observed_at,
                observation.slot.map(|slot| slot as i64),
                observation.signature,
                observation.program_id,
                observation.instruction,
                error_code,
                fingerprint,
                serde_json::to_string(&observation.logs)?,
                logs_text,
                now
            ])?;
    }
    let cutoff = now.saturating_sub(RECENT_OBSERVATION_RETENTION_SECS);
    let pruned = connection.execute(
        "DELETE FROM recent_observations WHERE observed_at < ?1",
        [cutoff],
    )?;
    Ok(ObservationIngestStats {
        processed: validated.len(),
        pruned,
    })
}

struct ValidatedObservation {
    observation: RecentObservationInput,
    fingerprint: String,
    dedup_key: String,
    error_code: Option<String>,
    logs_text: String,
}
fn validate_observation(
    observation: &RecentObservationInput,
    now: i64,
) -> anyhow::Result<ValidatedObservation> {
    let source = observation.source.trim();
    let source_id = observation.source_id.trim();
    if source.is_empty() || source.len() > 120 {
        bail!("observation source must contain 1-120 characters");
    }
    if !matches!(source, "rpc" | "raydium_api" | "indexer" | "partner_api") {
        bail!("observation source must be rpc, raydium_api, indexer, or partner_api");
    }
    if source_id.is_empty() || source_id.len() > 256 {
        bail!("observation source_id must contain 1-256 characters");
    }
    if !matches!(observation.cluster.as_str(), "mainnet" | "devnet") {
        bail!("observation cluster must be mainnet or devnet");
    }
    if observation.observed_at <= 0 || observation.observed_at > now.saturating_add(300) {
        bail!("observation observed_at must be a valid recent Unix timestamp");
    }
    anyhow::ensure!(
        observation.slot.is_none_or(|slot| slot <= i64::MAX as u64),
        "observation slot exceeds SQLite range"
    );
    let total_log_bytes = observation.logs.iter().map(String::len).sum::<usize>();
    if total_log_bytes > MAX_OBSERVATION_LOG_BYTES {
        bail!("observation logs exceed {MAX_OBSERVATION_LOG_BYTES} bytes");
    }
    if let Some(signature) = &observation.signature {
        let decoded = bs58::decode(signature)
            .into_vec()
            .context("observation signature is not valid Base58")?;
        if decoded.len() != 64 {
            bail!("observation signature must decode to 64 bytes");
        }
    }
    if let Some(program_id) = &observation.program_id {
        let decoded = bs58::decode(program_id)
            .into_vec()
            .context("observation program_id is not valid Base58")?;
        if decoded.len() != 32 {
            bail!("observation program_id must decode to 32 bytes");
        }
    }
    let error_code = observation
        .error_code
        .as_deref()
        .map(parse_numeric_error_code)
        .transpose()?;
    let logs_text = observation.logs.join(" ");
    let normalized_logs = normalize_observation_logs(&observation.logs);
    let mut fingerprint_hash = Sha256::new();
    for value in [
        observation.cluster.as_str(),
        observation.program_id.as_deref().unwrap_or_default(),
        observation.instruction.as_deref().unwrap_or_default(),
        error_code.as_deref().unwrap_or_default(),
        normalized_logs.as_str(),
    ] {
        fingerprint_hash.update((value.len() as u64).to_le_bytes());
        fingerprint_hash.update(value.as_bytes());
    }
    let fingerprint = format!("{:x}", fingerprint_hash.finalize());
    let mut dedup_hash = Sha256::new();
    for value in [source, source_id, observation.cluster.as_str()] {
        dedup_hash.update((value.len() as u64).to_le_bytes());
        dedup_hash.update(value.as_bytes());
    }
    let dedup_key = format!("{:x}", dedup_hash.finalize());
    let mut normalized = observation.clone();
    normalized.source = source.to_string();
    normalized.source_id = source_id.to_string();
    Ok(ValidatedObservation {
        observation: normalized,
        fingerprint,
        dedup_key,
        error_code,
        logs_text,
    })
}

fn parse_numeric_error_code(value: &str) -> anyhow::Result<String> {
    let lowercase = value.trim().to_ascii_lowercase();
    let parsed = if let Some(code) = lowercase
        .strip_prefix("custom(")
        .and_then(|code| code.strip_suffix(')'))
    {
        code.parse::<u32>()?
    } else if let Some(code) = lowercase.strip_prefix("0x") {
        u32::from_str_radix(code, 16)?
    } else {
        lowercase.parse::<u32>()?
    };
    Ok(parsed.to_string())
}

fn normalize_observation_logs(logs: &[String]) -> String {
    const ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut normalized = logs
        .iter()
        .map(|line| {
            let mut masked = line.clone();
            for token in line.split(|c: char| !ALPHABET.contains(c)) {
                if (32..=100).contains(&token.len())
                    && bs58::decode(token)
                        .into_vec()
                        .is_ok_and(|v| matches!(v.len(), 32 | 64))
                {
                    masked = masked.replace(token, "<entity>");
                }
            }
            normalize_text(&masked).to_ascii_lowercase()
        })
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.join(" ")
}
fn search_recent_observations(
    query: &str,
    options: &ObservationSearchOptions,
) -> anyhow::Result<Vec<RecentObservationHit>> {
    let query = query.trim();
    if query.is_empty() {
        bail!("observation search query must not be empty");
    }
    let connection = open_observation_database(&options.database_path)?;
    persistence::initialize_schema(&connection)?;
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs() as i64
        - options.window_seconds;
    let mut statement = connection.prepare(
        "SELECT source, source_id, cluster, observed_at, slot, signature,
                program_id, instruction, error_code, fingerprint, logs_json
         FROM recent_observations
         WHERE observed_at >= ?1
           AND (?2 IS NULL OR cluster = ?2)
           AND (lower(logs_text) LIKE '%' || lower(?3) || '%'
                OR lower(coalesce(program_id, '')) LIKE '%' || lower(?3) || '%'
                OR lower(coalesce(instruction, '')) LIKE '%' || lower(?3) || '%'
                OR coalesce(error_code, '') LIKE '%' || ?3 || '%')
         ORDER BY observed_at DESC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![since, options.cluster, query, options.limit as i64],
        |row| {
            Ok(RecentObservationHit {
                source: row.get(0)?,
                source_id: row.get(1)?,
                cluster: row.get(2)?,
                observed_at: row.get(3)?,
                slot: row.get::<_, Option<i64>>(4)?.map(|slot| slot as u64),
                signature: row.get(5)?,
                program_id: row.get(6)?,
                instruction: row.get(7)?,
                error_code: row.get(8)?,
                fingerprint: row.get(9)?,
                logs: serde_json::from_str(&row.get::<_, String>(10)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        10,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn observation_database_arg(value: Option<&String>) -> anyhow::Result<PathBuf> {
    Ok(value
        .map(PathBuf::from)
        .unwrap_or_else(persistence::default_path))
}
fn message_preview(value: &str) -> String {
    value.chars().take(500).collect()
}
fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_ingestion_deduplicates_prunes_and_searches_recent_errors() {
        let directory = std::env::temp_dir().join(format!(
            "recent-observation-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let database_path = directory.join("support.sqlite");
        let mut connection = open_observation_database(&database_path).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let current = observation_fixture("event-1", now, "Custom(38)");
        let expired = observation_fixture(
            "event-old",
            now - RECENT_OBSERVATION_RETENTION_SECS - 10,
            "Custom(38)",
        );
        let first_pass =
            ingest_recent_observations(&mut connection, &[current.clone(), expired]).unwrap();
        assert_eq!(first_pass.processed, 2);
        assert_eq!(first_pass.pruned, 1);

        let replay =
            ingest_recent_observations(&mut connection, std::slice::from_ref(&current)).unwrap();
        assert_eq!(replay.processed, 1);
        let count: i64 = connection
            .query_row("SELECT count(*) FROM recent_observations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        drop(connection);

        let options = ObservationSearchOptions {
            database_path: database_path.clone(),
            cluster: Some("mainnet".to_string()),
            window_seconds: 600,
            limit: 10,
        };
        let hits = search_recent_observations("Custom(38)", &options).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].error_code.as_deref(), Some("38"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn observation_fingerprint_masks_signature_and_address_values() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let first_address = bs58::encode([21u8; 32]).into_string();
        let second_address = bs58::encode([22u8; 32]).into_string();
        let first = observation_fixture("one", now, &format!("owner {first_address}"));
        let second = observation_fixture("two", now, &format!("owner {second_address}"));
        let first_fingerprint = validate_observation(&first, now).unwrap().fingerprint;
        let second_fingerprint = validate_observation(&second, now).unwrap().fingerprint;
        assert_eq!(first_fingerprint, second_fingerprint);
    }

    fn observation_fixture(source_id: &str, observed_at: i64, log: &str) -> RecentObservationInput {
        RecentObservationInput {
            source: "rpc".to_string(),
            source_id: source_id.to_string(),
            cluster: "mainnet".to_string(),
            observed_at,
            slot: Some(123),
            signature: Some(bs58::encode([31u8; 64]).into_string()),
            program_id: Some(bs58::encode([32u8; 32]).into_string()),
            instruction: Some("swap".to_string()),
            error_code: Some("Custom(38)".to_string()),
            logs: vec![log.to_string()],
        }
    }
}
