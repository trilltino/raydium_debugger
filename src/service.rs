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

/// V2 diagnosis response that can represent both landed and non-observed signatures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResponse {
    pub observation: ObservationStatus,
    pub diagnosis: Diagnosis,
    pub transaction: Option<TransactionDebugInfo>,
    pub formatted_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationStatus {
    pub status: String,
    pub cluster: Option<String>,
    pub providers_queried: Vec<String>,
    pub evidence: Vec<String>,
    pub hypotheses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    pub title: String,
    pub explanation: String,
    pub primary_action: String,
    pub evidence: Vec<String>,
    pub confidence: String,
    pub category: String,
    pub copy_markdown: String,
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

pub fn run_diagnostic_request_blocking(
    request: DebugRequest,
) -> anyhow::Result<DiagnosticResponse> {
    let cluster = request.cluster;
    match run_debug_request_blocking(request.clone()) {
        Ok(response) => {
            let observation = ObservationStatus {
                status: "landed".to_string(),
                cluster: response
                    .info
                    .provider
                    .cluster
                    .clone()
                    .or_else(|| cluster.map(|cluster| cluster.as_str().to_string())),
                providers_queried: observed_providers(&response.info),
                evidence: vec![
                    format!(
                        "Transaction was fetched at slot {}.",
                        response.info.slot_exact
                    ),
                    format!(
                        "Confirmation evidence: {}.",
                        response
                            .info
                            .status
                            .confirmation_status
                            .as_deref()
                            .unwrap_or("confirmed fetch")
                    ),
                ],
                hypotheses: Vec::new(),
            };
            let diagnosis = diagnosis_from_transaction(&response.info);
            Ok(DiagnosticResponse {
                observation,
                diagnosis,
                formatted_text: response.formatted_text,
                transaction: Some(response.info),
            })
        }
        Err(error) if looks_not_observed(&error) => {
            let cluster_label = cluster.map(|cluster| cluster.as_str().to_string());
            let observation = ObservationStatus {
                status: "not_observed_on_selected_provider".to_string(),
                cluster: cluster_label.clone(),
                providers_queried: cluster_label
                    .as_ref()
                    .map(|cluster| vec![format!("configured Triton {cluster} endpoint")])
                    .unwrap_or_else(|| vec!["configured Triton endpoint".to_string()]),
                evidence: vec![error.to_string()],
                hypotheses: vec![
                    "The transaction was never submitted.".to_string(),
                    "The signature belongs to a different cluster or provider history.".to_string(),
                    "The transaction expired or was dropped before confirmation.".to_string(),
                    "The wallet, SDK, RPC, or relay failed before the transaction landed."
                        .to_string(),
                ],
            };
            let diagnosis = Diagnosis {
                title: "Transaction was not observed on the selected provider".to_string(),
                explanation: "The debugger could not fetch a landed transaction for this signature on the selected cluster/provider. With a signature alone, it cannot prove blockhash expiry, packet drop, or priority-fee failure.".to_string(),
                primary_action: "Verify the cluster, then retry with submission telemetry or the raw signed transaction if you need pre-landing diagnosis.".to_string(),
                evidence: observation.evidence.clone(),
                confidence: "medium".to_string(),
                category: "not_observed".to_string(),
                copy_markdown: not_observed_markdown(&observation),
            };
            Ok(DiagnosticResponse {
                observation,
                diagnosis,
                transaction: None,
                formatted_text: String::new(),
            })
        }
        Err(error) => Err(error),
    }
}

fn reject_rpc_override(rpc_url: Option<&str>) -> anyhow::Result<()> {
    if rpc_url.is_some_and(|url| !url.trim().is_empty()) {
        anyhow::bail!(
            "RPC overrides are disabled. Configure TRITON_DEVNET_RPC_URL and TRITON_MAINNET_RPC_URL on the server."
        );
    }
    Ok(())
}

fn looks_not_observed(error: &anyhow::Error) -> bool {
    let message = format!("{error:#}").to_ascii_lowercase();
    message.contains("was not found on the selected cluster/rpc endpoint")
        || message.contains("invalid type: null")
}

fn observed_providers(info: &TransactionDebugInfo) -> Vec<String> {
    let mut providers = Vec::new();
    if !info.provider.rpc_endpoint_redacted.is_empty() {
        providers.push(info.provider.rpc_endpoint_redacted.clone());
    }
    if info.rpc.fallback_used {
        if let Some(fallback) = &info.provider.fallback_endpoint_redacted {
            providers.push(fallback.clone());
        }
    }
    providers
}

fn diagnosis_from_transaction(info: &TransactionDebugInfo) -> Diagnosis {
    let title = info
        .failure
        .as_ref()
        .and_then(|failure| failure.plain_title.clone())
        .unwrap_or_else(|| info.experience.headline.clone());
    let explanation = info
        .failure
        .as_ref()
        .and_then(|failure| failure.plain_explanation.clone())
        .unwrap_or_else(|| info.experience.message.clone());
    let primary_action = info
        .failure
        .as_ref()
        .and_then(|failure| failure.primary_action.clone())
        .unwrap_or_else(|| info.experience.next_step.clone());
    let evidence = info
        .failure
        .as_ref()
        .map(|failure| {
            if failure.evidence_summary.is_empty() {
                failure.evidence.clone()
            } else {
                failure.evidence_summary.clone()
            }
        })
        .unwrap_or_else(|| info.root_cause.evidence.clone());
    let confidence = info
        .failure
        .as_ref()
        .map(|failure| failure.confidence.clone())
        .unwrap_or_else(|| "medium".to_string());
    let category = info
        .failure
        .as_ref()
        .map(|failure| failure.category.clone())
        .unwrap_or_else(|| info.root_cause.category.clone());
    let copy_markdown =
        transaction_markdown(info, &title, &explanation, &primary_action, &evidence);
    Diagnosis {
        title,
        explanation,
        primary_action,
        evidence,
        confidence,
        category,
        copy_markdown,
    }
}

fn transaction_markdown(
    info: &TransactionDebugInfo,
    title: &str,
    explanation: &str,
    primary_action: &str,
    evidence: &[String],
) -> String {
    let mut lines = vec![
        format!("### {title}"),
        String::new(),
        format!("Signature: `{}`", info.signature),
        format!(
            "Cluster: `{}`",
            info.provider.cluster.as_deref().unwrap_or("unknown")
        ),
        format!("Status: `{}`", info.experience.status_label),
        String::new(),
        explanation.to_string(),
        String::new(),
        format!("Primary action: {primary_action}"),
    ];
    if !evidence.is_empty() {
        lines.push(String::new());
        lines.push("Evidence:".to_string());
        lines.extend(evidence.iter().map(|line| format!("- {line}")));
    }
    lines.join("\n")
}

fn not_observed_markdown(observation: &ObservationStatus) -> String {
    let mut lines = vec![
        "### Transaction was not observed on the selected provider".to_string(),
        String::new(),
        format!(
            "Cluster: `{}`",
            observation.cluster.as_deref().unwrap_or("unknown")
        ),
        "The debugger could not fetch a landed transaction for this signature.".to_string(),
        String::new(),
        "Evidence:".to_string(),
    ];
    lines.extend(observation.evidence.iter().map(|line| format!("- {line}")));
    lines.push(String::new());
    lines.push("Possible causes, not proven from a signature alone:".to_string());
    lines.extend(
        observation
            .hypotheses
            .iter()
            .map(|line| format!("- {line}")),
    );
    lines.join("\n")
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
