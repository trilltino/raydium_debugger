//! Shared server state for local auth, concurrency, and persistence.

use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::store::SignatureStore;

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

        Ok(Self {
            api_token,
            debug_limit: Arc::new(Semaphore::new(max_concurrent)),
            store: SignatureStore::from_env()?,
        })
    }
}
