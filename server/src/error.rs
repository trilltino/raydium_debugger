//! Structured HTTP error mapping for the React frontend.

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

/// Stable HTTP error payload consumed by the React UI.
#[derive(Serialize)]
pub struct ErrorBody {
    /// User-visible error message.
    pub error: String,
    /// Stable machine-readable category for UI branching/tests.
    pub error_kind: &'static str,
    /// Technical backend-derived detail without remediation hints.
    pub hintless_details: Option<String>,
}

/// Converts an arbitrary application error into a structured HTTP response.
pub fn error_response(status: StatusCode, error: anyhow::Error) -> axum::response::Response {
    let message = error.to_string();
    let details = format!("{error:#}");
    error_message_response(status, error_kind(&details), message, Some(details))
}

/// Builds a structured HTTP error when the caller already knows the category.
pub fn error_kind_response(
    status: StatusCode,
    error_kind: &'static str,
    error: impl Into<String>,
) -> axum::response::Response {
    let message = error.into();
    error_message_response(status, error_kind, message.clone(), Some(message))
}

fn error_message_response(
    status: StatusCode,
    error_kind: &'static str,
    message: String,
    details: Option<String>,
) -> axum::response::Response {
    let body = ErrorBody {
        error: message,
        error_kind,
        hintless_details: details,
    };
    (status, Json(body)).into_response()
}

fn error_kind(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("invalid transaction signature") {
        "invalid_signature"
    } else if lower.contains("was not found on the selected cluster/rpc endpoint") {
        "transaction_not_found"
    } else if lower.contains("invalid rpc url") {
        "invalid_rpc_url"
    } else if lower.contains("rpc overrides are disabled") {
        "rpc_override_disabled"
    } else if lower.contains("rpc host is not allowed") {
        "rpc_not_allowed"
    } else if lower.contains("too many debug requests") {
        "busy"
    } else if lower.contains("ai question requested") || lower.contains("ai request failed") {
        "ai_unavailable"
    } else if lower.contains("decode") || lower.contains("unsupported transaction version") {
        "unsupported_decode"
    } else if lower.contains("transaction fetch failed") || lower.contains("rpc") {
        "rpc_fetch"
    } else {
        "debug_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_invalid_signature_errors() {
        assert_eq!(
            error_kind("invalid transaction signature not-a-signature"),
            "invalid_signature"
        );
    }

    #[test]
    fn serializes_hintless_details_field() {
        let body = ErrorBody {
            error: "bad request".to_string(),
            error_kind: "debug_failed",
            hintless_details: Some("backend details".to_string()),
        };
        let json = serde_json::to_value(body).unwrap();
        assert_eq!(json["hintless_details"], "backend details");
        assert!(json.get("details").is_none());
    }

    #[test]
    fn classifies_cluster_mismatch_fetch_errors() {
        assert_eq!(
            error_kind("transaction abc was not found on the selected cluster/RPC endpoint"),
            "transaction_not_found"
        );
    }
}
