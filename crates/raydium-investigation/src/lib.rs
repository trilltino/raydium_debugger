//! Shared blocking evidence orchestration and durable lifecycle for HTTP and desktop.
#![warn(missing_docs)]
use anyhow::Context;
pub use raydium_knowledge::CuratedIncident;
use raydium_observability::recent_observation_matches;
pub use raydium_observability::{recent_observation_groups, RecentObservationSummary};

use matching::{MatchAssessment, MatchFeatures};
use raydium_debugger::{
    run_diagnostic_request_blocking, DebugDataMode, DebugRequest, DiagnosticResponse, RpcCluster,
};
pub use raydium_knowledge::matching;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Signature;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

mod contracts;
pub use contracts::*;

/// Durable ledger persistence; every API blocks and must run off executor threads.
mod persistence;
pub use persistence::InvestigationStore;
/// Shared asynchronous lifecycle, limits and replay.
mod service;
pub use service::*;
fn now_seconds() -> anyhow::Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs() as i64)
}

mod orchestration;
pub use orchestration::execute_investigation;

/// Common validation for HTTP and desktop callers.
pub fn validate_request(request: &InvestigationRequest) -> anyhow::Result<()> {
    if request
        .signature
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
        && request
            .symptom
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && request.recent_fingerprint.is_none()
    {
        anyhow::bail!("provide a transaction signature, a symptom, or a recent observation group");
    }
    if let Some(signature) = request
        .signature
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        signature
            .trim()
            .parse::<Signature>()
            .context("invalid transaction signature")?;
    }
    if request
        .symptom
        .as_ref()
        .is_some_and(|value| value.chars().count() > 1000)
    {
        anyhow::bail!("symptom must be 1,000 characters or fewer");
    }
    if let Some(fingerprint) = &request.recent_fingerprint {
        if fingerprint.len() != 64
            || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
            || request.cluster.is_none()
        {
            anyhow::bail!("recent fingerprint must be 64 hex characters with an explicit cluster");
        }
    }
    Ok(())
}

/// Redacted provider capabilities; never returns credentials.
pub fn provider_status() -> serde_json::Value {
    let endpoint = |key: &str| {
        std::env::var(key)
            .ok()
            .map(|url| raydium_debugger::redact_url(&url))
    };
    let configured = |key: &str| std::env::var(key).is_ok_and(|value| !value.trim().is_empty());
    serde_json::json!({"name":"triton_one", "triton": {
        "devnet_rpc": endpoint("TRITON_DEVNET_RPC_URL"), "mainnet_rpc": endpoint("TRITON_MAINNET_RPC_URL"),
        "devnet_fallback_rpc": endpoint("TRITON_DEVNET_FALLBACK_RPC_URL"), "mainnet_fallback_rpc": endpoint("TRITON_MAINNET_FALLBACK_RPC_URL"),
        "devnet_configured": configured("TRITON_DEVNET_RPC_URL"), "mainnet_configured": configured("TRITON_MAINNET_RPC_URL"),
        "devnet_grpc_available": configured("TRITON_DEVNET_GRPC_URL"), "mainnet_grpc_available": configured("TRITON_MAINNET_GRPC_URL")
    }})
}

/// Enables structured operator diagnostics only for approved application targets.
/// Dependency request tracing is disabled so credentials and RPC bodies stay excluded.
pub fn init_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    let filter = tracing_subscriber::filter::Targets::new()
        .with_default(tracing::level_filters::LevelFilter::OFF)
        .with_target(
            "raydium_investigation",
            tracing::level_filters::LevelFilter::INFO,
        )
        .with_target(
            "raydium_observability",
            tracing::level_filters::LevelFilter::INFO,
        );
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_writer(std::io::stderr),
        )
        .try_init();
}

#[cfg(test)]
mod tests;
