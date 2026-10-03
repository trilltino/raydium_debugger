//! Private enrichment operations; blocking operator tooling.
use super::*;
pub(super) struct EnrichmentOptions {
    pub(super) cluster: RpcCluster,
    pub(super) limit: usize,
    pub(super) database_path: PathBuf,
}
pub(super) fn parse_enrichment_options(args: &[String]) -> anyhow::Result<EnrichmentOptions> {
    let mut cluster = RpcCluster::Mainnet;
    let mut limit = MAX_ENRICHMENT_SIGNATURES;
    let mut database_path = support_database_path();
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        index += 1;
        match option {
            "--cluster" => {
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("missing value for --cluster"))?;
                cluster = match value.as_str() {
                    "mainnet" => RpcCluster::Mainnet,
                    "devnet" => RpcCluster::Devnet,
                    _ => bail!("cluster must be mainnet or devnet"),
                };
            }
            "--limit" => {
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("missing value for --limit"))?;
                limit = value.parse::<usize>().context("invalid enrichment limit")?;
                if limit == 0 || limit > MAX_ENRICHMENT_SIGNATURES {
                    bail!("enrichment limit must be between 1 and {MAX_ENRICHMENT_SIGNATURES}");
                }
            }
            "--database" => {
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("missing value for --database"))?;
                database_path = PathBuf::from(value);
            }
            other => bail!("unknown enrichment option {other:?}"),
        }
        index += 1;
    }
    Ok(EnrichmentOptions {
        cluster,
        limit,
        database_path,
    })
}

pub(super) fn enrich_candidate(
    connection: &mut Connection,
    case_id: &str,
    cluster: RpcCluster,
    limit: usize,
) -> anyhow::Result<EnrichmentStats> {
    if limit == 0 || limit > MAX_ENRICHMENT_SIGNATURES {
        bail!("enrichment limit must be between 1 and {MAX_ENRICHMENT_SIGNATURES}");
    }
    let signatures = {
        let mut statement = connection.prepare(
            "SELECT entities.canonical_value, min(entities.revision_id)
             FROM candidate_case_messages
             JOIN entities ON entities.revision_id = candidate_case_messages.revision_id
             WHERE candidate_case_messages.case_id = ?1
               AND entities.entity_type = 'transaction_signature_candidate'
             GROUP BY entities.canonical_value
             ORDER BY entities.canonical_value",
        )?;
        let rows = statement.query_map([case_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if signatures.is_empty() {
        bail!("candidate case contains no transaction signature candidates");
    }
    let mut stats = EnrichmentStats {
        signatures_found: signatures.len(),
        ..EnrichmentStats::default()
    };
    for (signature, source_revision_id) in signatures.into_iter().take(limit) {
        let cached: bool = connection.query_row(
            "SELECT EXISTS (
                SELECT 1 FROM transaction_snapshots
                WHERE signature = ?1 AND cluster = ?2
             )",
            params![signature, cluster.as_str()],
            |row| row.get(0),
        )?;
        if cached {
            stats.cached += 1;
            continue;
        }

        let request = DebugRequest {
            signature: signature.clone(),
            cluster: Some(cluster),
            data_mode: Some(DebugDataMode::RpcOnly),
            ..DebugRequest::default()
        };
        let (status, diagnostic_json, error) = match run_diagnostic_request_blocking(request) {
            Ok(diagnostic) => {
                let payload = serde_json::to_string(&diagnostic)?;
                (diagnostic.observation.status, Some(payload), None)
            }
            Err(error) => {
                stats.errors += 1;
                ("fetch_error".to_string(), None, Some(error.to_string()))
            }
        };
        let captured_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before Unix epoch")?
            .as_secs() as i64;
        store_transaction_snapshot(
            connection,
            TransactionSnapshotWrite {
                signature: &signature,
                cluster,
                source_revision_id,
                captured_at,
                observation_status: &status,
                diagnostic_json: diagnostic_json.as_deref(),
                error: error.as_deref(),
            },
        )?;
        stats.fetched += 1;
    }
    Ok(stats)
}

pub(super) fn store_transaction_snapshot(
    connection: &Connection,
    snapshot: TransactionSnapshotWrite<'_>,
) -> anyhow::Result<()> {
    connection.execute(
        "INSERT INTO transaction_snapshots (
            signature, cluster, source_revision_id, captured_at,
            observation_status, diagnostic_json, error
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(signature, cluster) DO UPDATE SET
            source_revision_id = excluded.source_revision_id,
            captured_at = excluded.captured_at,
            observation_status = excluded.observation_status,
            diagnostic_json = excluded.diagnostic_json,
            error = excluded.error",
        params![
            snapshot.signature,
            snapshot.cluster.as_str(),
            snapshot.source_revision_id,
            snapshot.captured_at,
            snapshot.observation_status,
            snapshot.diagnostic_json,
            snapshot.error
        ],
    )?;
    Ok(())
}
