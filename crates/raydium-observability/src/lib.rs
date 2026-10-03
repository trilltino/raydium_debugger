//! Operational evidence, bounded redacted queries and isolated collector storage.
#![warn(missing_docs)]
use raydium_debugger::RpcCluster;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
/// Bounded redacted queries that never return raw logs or signatures.
mod query;
pub use query::{recent_observation_groups, recent_observation_matches};
/// Private ingestion adapter; absent from runtime builds.
#[cfg(feature = "ingestion")]
pub mod ingestion;
/// Versioned observation persistence and explicit legacy migration.
pub mod persistence;
fn now_seconds() -> anyhow::Result<i64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64)
}
#[derive(Debug, Clone, Deserialize, Serialize)]
/// Recentobservationsummary.
pub struct RecentObservationSummary {
    /// Source.
    pub source: String,
    /// Cluster.
    pub cluster: String,
    /// Observed at.
    pub observed_at: i64,
    /// Observed chain slot; None means unavailable.
    pub slot: Option<u64>,
    /// Observed Base58 program identity; None means unknown.
    pub program_id: Option<String>,
    /// Instruction.
    pub instruction: Option<String>,
    /// Observed numeric custom error; None means unavailable.
    pub error_code: Option<String>,
    /// Stable SHA-256 grouping key from normalized operational evidence.
    pub fingerprint: String,
}

mod health;
pub use health::{observation_health, ObservationHealth};
