//! Shared server state for local auth, concurrency, and persistence.

use std::{path::PathBuf, sync::Arc};
use tokio::sync::Semaphore;

use crate::investigation::{InvestigationService, RuntimeLimits, RuntimePaths};
use crate::store::SignatureStore;
use raydium_knowledge_builder::ReviewStore;

const DEFAULT_MAX_CONCURRENT_DEBUGS: usize = 4;

/// Shared Axum state for local API security and provider configuration.
#[derive(Clone)]
pub struct AppState {
    /// Per-server token required by same-origin API clients.
    pub api_token: String,
    /// Semaphore limiting concurrent blocking Solana debug jobs.
    pub debug_limit: Arc<Semaphore>,
    /// Local JSON store for integrator-owned transaction examples.
    pub store: SignatureStore,
    /// Shared lifecycle and bounded progress replay.
    pub investigation_service: InvestigationService,
    /// Operational DB used only for redacted recent-observation summaries.
    pub observations_database_path: PathBuf,
    /// Private support archive review and sanitized publication paths.
    pub review_store: ReviewStore,
}

impl AppState {
    /// Builds state from environment variables and validates local persistence.
    pub fn from_env() -> anyhow::Result<Self> {
        let api_token = std::env::var("RAYDIUM_DEBUGGER_API_TOKEN")
            .ok()
            .filter(|token| !token.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let max_concurrent = std::env::var("RAYDIUM_DEBUGGER_MAX_CONCURRENT_DEBUGS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_CONCURRENT_DEBUGS);
        let paths = RuntimePaths::from_env(std::path::Path::new(".raydium-debugger"));
        let observations_database_path = paths.observations.clone();
        let investigation_service = InvestigationService::new(
            paths,
            RuntimeLimits {
                rpc_jobs: max_concurrent,
                ..RuntimeLimits::default()
            },
        )?;
        Ok(Self {
            api_token,
            debug_limit: investigation_service.rpc_capacity(),
            store: SignatureStore::from_env()?,
            investigation_service,
            observations_database_path,
            review_store: ReviewStore::from_env(),
        })
    }
}
