//! Bounded finalized-RPC polling. A page's observations and cursor commit together.
use super::*;
use serde_json::{json, Value};
use std::{io::Read, time::Duration};

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
struct Cursor {
    head: Option<String>,
    pending_head: Option<String>,
    before: Option<String>,
    backfill_complete: bool,
}

struct Options {
    address: String,
    cluster: RpcCluster,
    database: PathBuf,
    since: i64,
    end: Option<i64>,
    page_size: usize,
    max_pages: usize,
    poll_seconds: Option<u64>,
}

pub(super) fn run(args: &[String]) -> anyhow::Result<()> {
    let options = parse_options(args)?;
    let config = raydium_debugger::RpcConfig::from_env(true, Some(options.cluster))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let mut connection = open_observation_database(&options.database)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    // Endpoint identity is hashed: neither cursor keys nor errors expose credentials.
    let key = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}|{}|{}|{}|{:?}",
                config.primary_url,
                options.cluster.as_str(),
                options.address,
                options.since,
                options.end
            )
            .as_bytes()
        )
    );
    let mut failures = 0u32;
    loop {
        let outcome = collect_pages(&mut connection, &options, &key, |method, params| {
            let response = client
                .post(&config.primary_url)
                .json(&json!({"jsonrpc":"2.0", "id":1, "method":method, "params":params}))
                .send()
                .map_err(|_| anyhow!("collector RPC transport failed"))?;
            if !response.status().is_success() {
                bail!("collector RPC returned HTTP {}", response.status().as_u16());
            }
            let mut bytes = Vec::new();
            response.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 4 * 1024 * 1024 {
                bail!("collector RPC response exceeds 4 MiB");
            }
            let payload: Value = serde_json::from_slice(&bytes)?;
            if payload.get("error").is_some() {
                bail!("collector RPC provider rejected {method}");
            }
            payload
                .get("result")
                .cloned()
                .context("collector RPC response is missing result")
        });
        match outcome {
            Ok((pages, processed, done)) => {
                println!("RPC collector: {pages} pages, {processed} observations; range complete: {done}.");
                failures = 0;
                if options.end.is_some() && done {
                    break;
                }
            }
            Err(error) => {
                if options.poll_seconds.is_none() {
                    return Err(error);
                }
                failures = (failures + 1).min(6);
                eprintln!("RPC collector paused: {error}. Durable cursor retained for retry.");
            }
        }
        let Some(interval) = options.poll_seconds else {
            break;
        };
        std::thread::sleep(Duration::from_secs((interval * (1u64 << failures)).min(60)));
    }
    Ok(())
}

fn parse_options(args: &[String]) -> anyhow::Result<Options> {
    let mut options = Options {
        address: args.first().context("usage: observations collect-rpc <address> --cluster <devnet|mainnet> --since <unix-seconds> [--end <unix-seconds>] [--database <path>] [--poll-seconds <5..60>] [--page-size <1..100>] [--max-pages <1..100>]")?.clone(),
        cluster: RpcCluster::Devnet, database: persistence::default_path(), since: 0,
        end: None, page_size: 25, max_pages: 4, poll_seconds: None,
    };
    if bs58::decode(&options.address).into_vec()?.len() != 32 {
        bail!("collector address must decode to 32 bytes");
    }
    let mut cluster_supplied = false;
    let mut index = 1;
    while index < args.len() {
        let value = args
            .get(index + 1)
            .context("collector option requires a value")?;
        match args[index].as_str() {
            "--cluster" => {
                options.cluster = match value.as_str() {
                    "devnet" => RpcCluster::Devnet,
                    "mainnet" => RpcCluster::Mainnet,
                    _ => bail!("cluster must be devnet or mainnet"),
                };
                cluster_supplied = true;
            }
            "--since" => options.since = value.parse()?,
            "--end" => options.end = Some(value.parse()?),
            "--database" => options.database = PathBuf::from(value),
            "--page-size" => options.page_size = value.parse()?,
            "--max-pages" => options.max_pages = value.parse()?,
            "--poll-seconds" => options.poll_seconds = Some(value.parse()?),
            other => bail!("unknown collector option {other:?}"),
        }
        index += 2;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    if !cluster_supplied
        || options.since <= 0
        || (options.end.is_some() && options.since < now - RECENT_OBSERVATION_RETENTION_SECS)
        || options.since > now
        || options
            .end
            .is_some_and(|end| end < options.since || end > now)
    {
        bail!("provide an explicit cluster and valid since/end Unix timestamps; bounded backfill must lie within the 30-day retention window");
    }
    if !(1..=100).contains(&options.page_size)
        || !(1..=100).contains(&options.max_pages)
        || options
            .poll_seconds
            .is_some_and(|seconds| !(5..=60).contains(&seconds))
    {
        bail!("page-size/max-pages must be 1..100; poll-seconds must be 5..60");
    }
    Ok(options)
}

fn load_cursor(connection: &Connection, key: &str) -> anyhow::Result<Cursor> {
    let raw: Option<String> = connection
        .query_row(
            "SELECT state_json FROM observation_collectors WHERE collector_key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(raw
        .map(|raw| serde_json::from_str(&raw))
        .transpose()?
        .unwrap_or_default())
}

fn collect_pages(
    connection: &mut Connection,
    options: &Options,
    key: &str,
    mut rpc: impl FnMut(&str, Value) -> anyhow::Result<Value>,
) -> anyhow::Result<(usize, usize, bool)> {
    let mut cursor = load_cursor(connection, key)?;
    if cursor.backfill_complete && options.end.is_some() {
        return Ok((0, 0, true));
    }
    let mut processed = 0;
    // Keep live collector identity stable across long outages, with bounded retention.
    let floor = if options.end.is_none() {
        options.since.max(
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64
                - RECENT_OBSERVATION_RETENTION_SECS,
        )
    } else {
        options.since
    };
    for page in 0..options.max_pages {
        let mut config = json!({"commitment":"finalized", "limit":options.page_size});
        if let Some(before) = &cursor.before {
            config["before"] = json!(before);
        }
        if let Some(head) = &cursor.head {
            config["until"] = json!(head);
        }
        let raw = rpc("getSignaturesForAddress", json!([options.address, config]))?;
        let entries = raw.as_array().context("signature page must be an array")?;
        if entries.len() > options.page_size {
            bail!("RPC signature page exceeds requested limit");
        }
        let mut next = cursor.clone();
        let mut observations = Vec::new();
        let mut reached_floor = false;
        for entry in entries {
            let signature = entry["signature"]
                .as_str()
                .context("signature entry is missing signature")?;
            if bs58::decode(signature).into_vec()?.len() != 64 {
                bail!("RPC signature is malformed");
            }
            if next.pending_head.is_none() {
                next.pending_head = Some(signature.to_string());
            }
            let time = entry["blockTime"].as_i64();
            if time.is_some_and(|time| time < floor) {
                reached_floor = true;
                break;
            }
            if time.is_some_and(|time| options.end.is_some_and(|end| time > end)) {
                continue;
            }
            let transaction = rpc(
                "getTransaction",
                json!([signature, {
                    "encoding":"json", "commitment":"finalized",
                    "maxSupportedTransactionVersion":raydium_debugger::MAX_SUPPORTED_TX_VERSION
                }]),
            )?;
            // Missing transaction or logs must retry this page, never move past missing evidence.
            let meta = transaction
                .get("meta")
                .filter(|meta| meta.is_object())
                .context("transaction evidence unavailable; cursor unchanged")?;
            let logs: Vec<String> = serde_json::from_value(
                meta.get("logMessages")
                    .cloned()
                    .context("transaction logs unavailable; cursor unchanged")?,
            )?;
            let slot = transaction["slot"]
                .as_u64()
                .context("transaction slot unavailable")?;
            if slot > i64::MAX as u64 || entry["slot"].as_u64() != Some(slot) {
                bail!("transaction slot does not match signature entry");
            }
            let observed_at = transaction["blockTime"]
                .as_i64()
                .or(time)
                .context("transaction timestamp unavailable; cursor unchanged")?;
            if observed_at < floor || options.end.is_some_and(|end| observed_at > end) {
                continue;
            }
            // Attribute a custom error only to the program that actually failed, not the watched address.
            let failure = logs.iter().find_map(|line| {
                let rest = line.strip_prefix("Program ")?;
                let (program, error) = rest.split_once(" failed: ")?;
                Some((
                    program.to_string(),
                    raydium_debugger::parse_custom_error_code(error).map(|code| code.to_string()),
                ))
            });
            observations.push(RecentObservationInput {
                source: "rpc".into(),
                source_id: signature.into(),
                cluster: options.cluster.as_str().into(),
                observed_at,
                slot: Some(slot),
                signature: Some(signature.into()),
                program_id: failure.as_ref().map(|failure| failure.0.clone()),
                // Instruction names are unscoped log text; retain them in logs, not as structured facts.
                instruction: None,
                error_code: failure.and_then(|failure| failure.1),
                logs,
            });
        }
        let done = reached_floor || entries.len() < options.page_size;
        if done {
            next.head = next.pending_head.take().or(next.head);
            next.before = None;
            next.backfill_complete = options.end.is_some();
        } else {
            let before = entries
                .last()
                .and_then(|entry| entry["signature"].as_str())
                .context("missing last signature")?
                .to_string();
            if cursor.before.as_ref() == Some(&before) {
                bail!("RPC pagination did not advance");
            }
            next.before = Some(before);
        }
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // Refuse a concurrent collector's stale page rather than overwrite its checkpoint.
        if load_cursor(&transaction, key)? != cursor {
            bail!("another collector advanced this cursor; retry");
        }
        let stats = ingest_recent_observations_into(&transaction, &observations)?;
        transaction.execute("INSERT INTO observation_collectors VALUES (?1, ?2, ?3) ON CONFLICT(collector_key) DO UPDATE SET state_json=excluded.state_json, updated_at=excluded.updated_at",
            params![key, serde_json::to_string(&next)?, SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64])?;
        transaction.commit()?;
        processed += stats.processed;
        cursor = next;
        if done {
            return Ok((page + 1, processed, true));
        }
    }
    Ok((options.max_pages, processed, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(index: u8) -> String {
        bs58::encode([index; 64]).into_string()
    }
    fn setup() -> (PathBuf, Connection, Options, i64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let path = std::env::temp_dir().join(format!(
            "collector-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let connection = open_observation_database(&path).unwrap();
        let options = Options {
            address: bs58::encode([1; 32]).into_string(),
            cluster: RpcCluster::Devnet,
            database: path.clone(),
            since: now - 3600,
            end: None,
            page_size: 2,
            max_pages: 1,
            poll_seconds: None,
        };
        (path, connection, options, now)
    }
    fn entry(index: u8, time: i64) -> Value {
        json!({"signature":signature(index), "slot":index, "blockTime":time})
    }
    fn tx(index: u8, time: i64) -> Value {
        json!({"slot":index, "blockTime":time, "meta":{"err":null,"logMessages":["Program 11111111111111111111111111111111 success"]}})
    }

    #[test]
    fn restart_drains_backlog_before_advancing_live_head_and_replay_deduplicates() {
        let (path, mut connection, options, now) = setup();
        let first = collect_pages(&mut connection, &options, "test", |method, params| {
            if method == "getSignaturesForAddress" {
                assert!(params[1].get("before").is_none());
                Ok(json!([entry(4, now), entry(3, now)]))
            } else {
                Ok(tx(if params[0] == signature(4) { 4 } else { 3 }, now))
            }
        })
        .unwrap();
        assert_eq!(first, (1, 2, false));
        assert_eq!(load_cursor(&connection, "test").unwrap().head, None);
        drop(connection);
        let mut connection = open_observation_database(&path).unwrap();
        collect_pages(&mut connection, &options, "test", |method, params| {
            if method == "getSignaturesForAddress" {
                assert_eq!(params[1]["before"], signature(3));
                Ok(json!([entry(2, now)]))
            } else {
                Ok(tx(2, now))
            }
        })
        .unwrap();
        assert_eq!(
            load_cursor(&connection, "test").unwrap().head,
            Some(signature(4))
        );
        collect_pages(&mut connection, &options, "replay", |method, _| {
            if method == "getSignaturesForAddress" {
                Ok(json!([entry(2, now)]))
            } else {
                Ok(tx(2, now))
            }
        })
        .unwrap();
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM recent_observations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            3
        );
        collect_pages(&mut connection, &options, "test", |method, params| {
            assert_eq!(method, "getSignaturesForAddress");
            assert_eq!(params[1]["until"], signature(4));
            Ok(json!([]))
        })
        .unwrap();
        drop(connection);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn failed_page_does_not_commit_observations_or_cursor() {
        let (path, mut connection, options, now) = setup();
        let result = collect_pages(&mut connection, &options, "test", |method, params| {
            if method == "getSignaturesForAddress" {
                Ok(json!([entry(4, now), entry(3, now)]))
            } else if params[0] == signature(4) {
                Ok(tx(4, now))
            } else {
                Ok(Value::Null)
            }
        });
        assert!(result.is_err());
        assert_eq!(load_cursor(&connection, "test").unwrap(), Cursor::default());
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM recent_observations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(connection);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn bounded_backfill_filters_time_range_and_stays_complete_after_restart() {
        let (path, mut connection, mut options, now) = setup();
        options.end = Some(now - 60);
        options.max_pages = 3;
        let mut calls = 0;
        let result = collect_pages(&mut connection, &options, "range", |method, params| {
            if method == "getSignaturesForAddress" {
                calls += 1;
                if calls == 1 {
                    Ok(json!([entry(4, now), entry(3, now - 120)]))
                } else {
                    assert_eq!(params[1]["before"], signature(3));
                    Ok(json!([entry(2, now - 7200)]))
                }
            } else {
                assert_eq!(params[0], signature(3));
                Ok(tx(3, now - 120))
            }
        })
        .unwrap();
        assert_eq!(result, (2, 1, true));
        drop(connection);
        let mut connection = open_observation_database(&path).unwrap();
        assert_eq!(
            collect_pages(&mut connection, &options, "range", |_, _| panic!(
                "completed backfill must not refetch"
            ))
            .unwrap(),
            (0, 0, true)
        );
        drop(connection);
        let _ = fs::remove_file(path);
    }
}
