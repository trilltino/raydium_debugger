//! Shared request/response orchestration used by CLI, Axum, and Tauri.
//!
//! This layer owns the app-level request types and the common debug flow so each
//! shell calls the same Triton-backed debugger behavior. It also centralizes
//! formatted-text generation, response shaping, and optional AI handoff inputs.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Signature;

use crate::debug::{debug_transaction, format_debug_info, TransactionDebugInfo};
use crate::debug::{ProviderDebugInfo, RateLimitDebugInfo};
use crate::rpc::{make_rpc, redact_url, RpcCluster, RpcConfig};

/// Request accepted by CLI, HTTP, and Tauri frontends.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DebugRequest {
    /// Transaction signature to fetch and decode.
    pub signature: String,
    /// Deprecated. RPC overrides are rejected; configure Triton server-side.
    pub rpc_url: Option<String>,
    /// Disables the environment fallback RPC when true.
    #[serde(default)]
    pub no_fallback: bool,
    /// Optional cluster profile for Triton endpoint selection.
    pub cluster: Option<RpcCluster>,
    /// Optional data mode; currently `auto` and `rpc_only` use deterministic RPC.
    pub data_mode: Option<DebugDataMode>,
}

/// Data-source strategy requested by the caller.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DebugDataMode {
    Auto,
    RpcOnly,
    RpcPlusGrpc,
}

/// Shared debugger response shape used by every frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugResponse {
    /// Structured debugger output for UI rendering.
    pub info: TransactionDebugInfo,
    /// Same output rendered as the existing terminal report.
    pub formatted_text: String,
}

/// Optional AI question request over an existing debug result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAskRequest {
    /// Debugger output used as the AI context.
    pub info: TransactionDebugInfo,
    /// User question to answer from the provided context.
    pub question: String,
    /// Optional model override.
    pub model: Option<String>,
}

/// Runs a full transaction debug with the same RPC fallback behavior as the CLI.
pub async fn run_debug_request(request: DebugRequest) -> anyhow::Result<DebugResponse> {
    run_debug_request_blocking(request)
}

/// Blocking implementation for callers that already moved work off async executors.
pub fn run_debug_request_blocking(request: DebugRequest) -> anyhow::Result<DebugResponse> {
    reject_rpc_override(request.rpc_url.as_deref())?;
    let sig = request
        .signature
        .parse::<Signature>()
        .with_context(|| format!("invalid transaction signature {}", request.signature))?;
    let cfg = RpcConfig::from_env(request.no_fallback, request.cluster)?;
    let info = debug_with_fallback(&cfg, &sig)?;
    let formatted_text = format_debug_info(&info);
    Ok(DebugResponse {
        info,
        formatted_text,
    })
}

fn reject_rpc_override(rpc_url: Option<&str>) -> anyhow::Result<()> {
    if rpc_url.is_some_and(|url| !url.trim().is_empty()) {
        anyhow::bail!(
            "RPC overrides are disabled. Configure TRITON_DEVNET_RPC_URL and TRITON_MAINNET_RPC_URL on the server."
        );
    }
    Ok(())
}

/// Runs the optional AI path when the crate is built with the `ai` feature.
#[cfg(feature = "ai")]
pub async fn run_ai_request(request: AiAskRequest) -> anyhow::Result<crate::ai::AiResponse> {
    crate::ai::ask_ai(&request.info, &request.question, request.model.as_deref()).await
}

/// Reports a clear error when the AI feature is not compiled in.
#[cfg(not(feature = "ai"))]
pub async fn run_ai_request(request: AiAskRequest) -> anyhow::Result<serde_json::Value> {
    let _ = request;
    anyhow::bail!("AI question requested, but this build does not include the `ai` feature.");
}

fn debug_with_fallback(cfg: &RpcConfig, sig: &Signature) -> anyhow::Result<TransactionDebugInfo> {
    let primary = make_rpc(&cfg.primary_url);
    match debug_transaction(&primary, sig) {
        Ok(info) => Ok(with_primary_rpc(info, cfg)),
        Err(primary_err) => debug_with_fallback_rpc(cfg, sig, primary_err),
    }
}

fn with_primary_rpc(mut info: TransactionDebugInfo, cfg: &RpcConfig) -> TransactionDebugInfo {
    info.rpc.endpoint = redact_url(&cfg.primary_url);
    info.rpc.fallback_endpoint = cfg.fallback_url.as_deref().map(redact_url);
    info.provider = provider_info(cfg, false, Vec::new());
    info
}

fn debug_with_fallback_rpc(
    cfg: &RpcConfig,
    sig: &Signature,
    primary_err: anyhow::Error,
) -> anyhow::Result<TransactionDebugInfo> {
    let Some(fallback_url) = fallback_url(cfg) else {
        return Err(primary_err).with_context(|| format!("failed to debug transaction {sig}"));
    };

    let fallback = make_rpc(fallback_url);
    let mut info = debug_transaction(&fallback, sig).with_context(|| {
        format!(
            "primary RPC {} failed and fallback RPC {} also failed for {}",
            redact_url(&cfg.primary_url),
            redact_url(fallback_url),
            sig
        )
    })?;
    info.rpc.endpoint = redact_url(fallback_url);
    info.rpc.fallback_endpoint = Some(redact_url(fallback_url));
    info.rpc.fallback_used = true;
    info.metadata
        .fetch_warnings
        .push(format!("Primary RPC failed: {primary_err}"));
    info.provider = provider_info(
        cfg,
        true,
        vec![format!("Primary RPC failed: {primary_err}")],
    );
    Ok(info)
}

fn provider_info(cfg: &RpcConfig, fallback_used: bool, warnings: Vec<String>) -> ProviderDebugInfo {
    ProviderDebugInfo {
        name: cfg.provider_name.clone(),
        cluster: cfg.cluster.clone(),
        rpc_endpoint_redacted: if fallback_used {
            cfg.fallback_url
                .as_deref()
                .map(redact_url)
                .unwrap_or_else(|| redact_url(&cfg.primary_url))
        } else {
            redact_url(&cfg.primary_url)
        },
        fallback_endpoint_redacted: cfg.fallback_url.as_deref().map(redact_url),
        grpc_available: cfg.grpc_available,
        grpc_used: false,
        rate_limit: rate_limit_from_warnings(&warnings),
        warnings,
    }
}

fn rate_limit_from_warnings(warnings: &[String]) -> Option<RateLimitDebugInfo> {
    let limited = warnings
        .iter()
        .any(|warning| warning.contains("429") || warning.to_ascii_lowercase().contains("rate"));
    limited.then(|| RateLimitDebugInfo {
        limited,
        retry_after_seconds: Some(10),
        details: warnings.to_vec(),
    })
}

fn fallback_url(cfg: &RpcConfig) -> Option<&str> {
    cfg.fallback_url
        .as_deref()
        .filter(|_| cfg.fallback_enabled)
        .filter(|fallback| redact_url(fallback) != redact_url(&cfg.primary_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_request_defaults_to_fallback_enabled() {
        let req = DebugRequest::default();
        assert!(!req.no_fallback);
        assert_eq!(req.rpc_url, None);
    }

    #[test]
    fn rpc_override_is_rejected() {
        let err = reject_rpc_override(Some("https://example.com")).unwrap_err();
        assert!(err.to_string().contains("RPC overrides are disabled"));
    }
}
