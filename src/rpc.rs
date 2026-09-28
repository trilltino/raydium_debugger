//! Triton-only RPC configuration, redaction, and client construction.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use std::time::Duration;

pub const RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Runtime RPC endpoint selection, including optional fallback behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcConfig {
    pub primary_url: String,
    pub fallback_url: Option<String>,
    pub fallback_enabled: bool,
    pub cluster: Option<String>,
    pub provider_name: String,
    pub grpc_available: bool,
}

/// Redacted RPC metadata included in debug output.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct RpcDebugInfo {
    pub endpoint: String,
    pub fallback_endpoint: Option<String>,
    pub fallback_used: bool,
}

impl RpcConfig {
    /// Resolves CLI/env RPC settings while keeping fallback opt-in by env var.
    pub fn from_env(no_fallback: bool, cluster: Option<RpcCluster>) -> anyhow::Result<Self> {
        let cluster = cluster.unwrap_or_else(default_cluster);
        let primary_url = triton_rpc_url(cluster).with_context(|| {
            format!(
                "{} is required for Triton-only debugging",
                cluster.rpc_env_key()
            )
        })?;
        let fallback_url = triton_fallback_url(cluster).filter(|url| !url.trim().is_empty());
        let grpc_available = triton_grpc_url(cluster).is_some();
        let provider_name = provider_name(&primary_url).to_string();

        Ok(Self {
            primary_url,
            fallback_url,
            fallback_enabled: !no_fallback,
            cluster: Some(cluster.as_str().to_string()),
            provider_name,
            grpc_available,
        })
    }
}

/// Supported cluster selection for Triton profile resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcCluster {
    Devnet,
    Mainnet,
}

impl RpcCluster {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Devnet => "devnet",
            Self::Mainnet => "mainnet",
        }
    }

    pub fn rpc_env_key(self) -> &'static str {
        match self {
            Self::Devnet => "TRITON_DEVNET_RPC_URL",
            Self::Mainnet => "TRITON_MAINNET_RPC_URL",
        }
    }
}

fn default_cluster() -> RpcCluster {
    match std::env::var("TRITON_DEFAULT_CLUSTER")
        .unwrap_or_else(|_| "devnet".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "mainnet" | "mainnet-beta" => RpcCluster::Mainnet,
        _ => RpcCluster::Devnet,
    }
}

fn triton_rpc_url(cluster: RpcCluster) -> Option<String> {
    std::env::var(cluster.rpc_env_key())
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn triton_fallback_url(cluster: RpcCluster) -> Option<String> {
    let key = match cluster {
        RpcCluster::Devnet => "TRITON_DEVNET_FALLBACK_RPC_URL",
        RpcCluster::Mainnet => "TRITON_MAINNET_FALLBACK_RPC_URL",
    };
    std::env::var(key).ok()
}

fn triton_grpc_url(cluster: RpcCluster) -> Option<String> {
    let key = match cluster {
        RpcCluster::Devnet => "TRITON_DEVNET_GRPC_URL",
        RpcCluster::Mainnet => "TRITON_MAINNET_GRPC_URL",
    };
    std::env::var(key).ok().filter(|url| !url.trim().is_empty())
}

fn provider_name(_url: &str) -> &'static str {
    "triton_one"
}

/// Creates a confirmed-commitment RPC client with the debugger timeout.
pub fn make_rpc(url: &str) -> RpcClient {
    RpcClient::new_with_timeout_and_commitment(
        url.to_string(),
        RPC_TIMEOUT,
        CommitmentConfig::confirmed(),
    )
}

/// Removes token paths and query API keys before serializing endpoint metadata.
pub fn redact_url(url: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    match base.split_once("://") {
        Some((scheme, rest)) => {
            let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
            if path.is_empty() {
                format!("{scheme}://{host}")
            } else {
                format!("{scheme}://{host}/***")
            }
        }
        None => "***".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn triton_cluster_profile_is_used() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_rpc_env();
        std::env::set_var(
            "TRITON_MAINNET_RPC_URL",
            "https://mainnet.rpcpool.com/token",
        );
        std::env::set_var("SOLANA_RPC_URL", "https://api.devnet.solana.com");

        let cfg = RpcConfig::from_env(false, Some(RpcCluster::Mainnet)).unwrap();

        clear_rpc_env();
        assert_eq!(cfg.primary_url, "https://mainnet.rpcpool.com/token");
        assert_eq!(cfg.provider_name, "triton_one");
        assert_eq!(cfg.cluster.as_deref(), Some("mainnet"));
    }

    #[test]
    fn solana_rpc_url_is_ignored() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_rpc_env();
        std::env::set_var("SOLANA_RPC_URL", "https://env.devnet.rpcpool.com/env-token");

        let err = RpcConfig::from_env(false, Some(RpcCluster::Devnet)).unwrap_err();

        clear_rpc_env();
        assert!(err.to_string().contains("TRITON_DEVNET_RPC_URL"));
    }

    #[test]
    fn no_fallback_disables_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_rpc_env();
        std::env::set_var("TRITON_DEVNET_RPC_URL", "https://devnet.rpcpool.com/token");
        std::env::set_var(
            "TRITON_DEVNET_FALLBACK_RPC_URL",
            "https://devnet-fallback.rpcpool.com/token",
        );

        let cfg = RpcConfig::from_env(true, Some(RpcCluster::Devnet)).unwrap();

        clear_rpc_env();
        assert!(!cfg.fallback_enabled);
        assert_eq!(
            cfg.fallback_url.as_deref(),
            Some("https://devnet-fallback.rpcpool.com/token")
        );
    }

    #[test]
    fn redact_strips_rpcpool_token_path() {
        assert_eq!(
            redact_url("https://xfsoluti-solanad-d155.devnet.rpcpool.com/df802762-token"),
            "https://xfsoluti-solanad-d155.devnet.rpcpool.com/***"
        );
    }

    #[test]
    fn redact_strips_query_apikey() {
        assert_eq!(
            redact_url("https://mainnet.helius-rpc.com/?api-key=secret"),
            "https://mainnet.helius-rpc.com"
        );
    }

    fn clear_rpc_env() {
        for key in [
            "TRITON_DEFAULT_CLUSTER",
            "TRITON_DEVNET_RPC_URL",
            "TRITON_MAINNET_RPC_URL",
            "TRITON_DEVNET_FALLBACK_RPC_URL",
            "TRITON_MAINNET_FALLBACK_RPC_URL",
            "SOLANA_RPC_URL",
            "SOLANA_RPC_FALLBACK_URL",
        ] {
            std::env::remove_var(key);
        }
    }
}
