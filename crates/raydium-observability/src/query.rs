//! Read-only grouping excludes private source contents and limits every result.
use super::*;
/// Recent observation matches.
pub fn recent_observation_matches(
    database_path: &Path,
    query: &str,
    cluster: Option<RpcCluster>,
) -> anyhow::Result<Vec<RecentObservationSummary>> {
    if !database_path.is_file() {
        return Ok(Vec::new());
    }
    let connection =
        Connection::open_with_flags(database_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    let has_table: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'recent_observations')",
        [],
        |row| row.get(0),
    )?;
    if !has_table {
        return Ok(Vec::new());
    }
    let since = now_seconds()? - 60 * 60;
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let normalized_error = normalize_error_query(query);
    let mut statement = connection.prepare(
        "SELECT source, cluster, max(observed_at), max(slot), program_id, instruction,
                error_code, fingerprint
         FROM recent_observations
         WHERE observed_at >= ?1
           AND (?2 IS NULL OR cluster = ?2)
           AND (instr(lower(logs_text), lower(?3)) > 0
                OR instr(lower(coalesce(instruction, '')), lower(?3)) > 0
             OR error_code = ?3
             OR (?4 IS NOT NULL AND error_code = ?4))
         GROUP BY fingerprint, cluster, source, program_id, instruction, error_code
         ORDER BY max(observed_at) DESC LIMIT 10",
    )?;
    let cluster = cluster.map(|cluster| cluster.as_str().to_string());
    let rows = statement.query_map(params![since, cluster, query, normalized_error], |row| {
        Ok(RecentObservationSummary {
            source: row.get(0)?,
            cluster: row.get(1)?,
            observed_at: row.get(2)?,
            slot: row.get::<_, Option<i64>>(3)?.map(|slot| slot as u64),
            program_id: row.get(4)?,
            instruction: row.get(5)?,
            error_code: row.get(6)?,
            fingerprint: row.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn normalize_error_query(query: &str) -> Option<String> {
    let value = query.trim().to_ascii_lowercase();
    if let Some(code) = value
        .strip_prefix("custom(")
        .and_then(|code| code.strip_suffix(')'))
    {
        return code.parse::<u32>().ok().map(|code| code.to_string());
    }
    if let Some(code) = value.strip_prefix("0x") {
        return u32::from_str_radix(code, 16)
            .ok()
            .map(|code| code.to_string());
    }
    value.parse::<u32>().ok().map(|code| code.to_string())
}

/// Redacted observation groups, never signatures or raw logs.
pub fn recent_observation_groups(
    path: &Path,
    cluster: Option<RpcCluster>,
    fingerprint: Option<&str>,
) -> anyhow::Result<Vec<RecentObservationSummary>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='recent_observations')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare("SELECT source, cluster, max(observed_at), max(slot), program_id, instruction, error_code, fingerprint
        FROM recent_observations WHERE observed_at >= ?1 AND (?2 IS NULL OR cluster = ?2) AND (?3 IS NULL OR fingerprint = ?3)
        GROUP BY fingerprint, cluster, source, program_id, instruction, error_code ORDER BY max(observed_at) DESC LIMIT 20")?;
    let rows = statement.query_map(
        params![
            now_seconds()? - 30 * 24 * 60 * 60,
            cluster.map(RpcCluster::as_str),
            fingerprint
        ],
        |row| {
            Ok(RecentObservationSummary {
                source: row.get(0)?,
                cluster: row.get(1)?,
                observed_at: row.get(2)?,
                slot: row.get::<_, Option<i64>>(3)?.map(|value| value as u64),
                program_id: row.get(4)?,
                instruction: row.get(5)?,
                error_code: row.get(6)?,
                fingerprint: row.get(7)?,
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
