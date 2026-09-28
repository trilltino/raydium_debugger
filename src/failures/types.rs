//! Standardized failure types shared by CLI, server, frontend, and AI.

use serde::{Deserialize, Serialize};

/// UI-ready diagnosis for a failed transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StandardizedFailure {
    /// Registry or heuristic that produced this diagnosis.
    pub source: String,
    /// Failing program id when identified.
    pub program_id: Option<String>,
    /// Human-readable program label.
    pub program_label: Option<String>,
    /// Top-level failing instruction index.
    pub instruction_index: Option<usize>,
    /// Custom error code in decimal form.
    pub code_decimal: Option<u32>,
    /// Custom error code in hexadecimal form.
    pub code_hex: Option<String>,
    /// Program error enum variant name when known.
    pub name: Option<String>,
    /// Short UI headline.
    pub title: String,
    /// User-facing explanation.
    pub user_message: String,
    /// Lower-level technical explanation.
    pub technical_message: String,
    /// Stable UI category.
    pub category: String,
    /// Severity label.
    pub severity: String,
    /// Confidence label.
    pub confidence: String,
    /// Evidence lines used for the diagnosis.
    pub evidence: Vec<String>,
    /// Concrete follow-up actions.
    pub suggested_actions: Vec<String>,
    /// Stable status describing how far decoding got.
    pub decode_status: String,
    /// Sources the decoder checked before producing this result.
    pub decode_attempts: Vec<String>,
    /// Artifact needed to decode this error more precisely, when applicable.
    pub missing_artifact: Option<String>,
    /// Plain-language title for non-technical UI surfaces.
    pub plain_title: Option<String>,
    /// Plain-language explanation of what happened.
    pub plain_explanation: Option<String>,
    /// The single highest-value next action.
    pub primary_action: Option<String>,
    /// Ordered checklist for a developer or integrator.
    pub action_checklist: Vec<String>,
    /// Grouped evidence explained without raw Solana jargon.
    pub evidence_summary: Vec<String>,
    /// Plain-language explanation of how the error was decoded.
    pub decode_explanation: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FailureCode {
    pub(crate) code: u32,
    pub(crate) name: &'static str,
    pub(crate) message: &'static str,
}
