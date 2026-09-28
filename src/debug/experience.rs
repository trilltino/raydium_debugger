//! Plain-language UX summaries derived from deterministic debug evidence.
//!
//! This module turns decoded failures, freshness, Raydium product hints, and
//! recommended actions into short UI copy such as status labels, headlines,
//! badges, and primary next steps. It deliberately summarizes existing evidence
//! rather than inventing new causes.

use crate::failures::StandardizedFailure;

use super::types::{
    ExperienceSummary, FreshnessInfo, InstructionDebugInfo, RaydiumProductDebug, RootCause,
    TransactionMetadata,
};

/// Builds plain-language UX copy without changing the deterministic diagnosis.
pub(crate) fn build_experience_summary(input: ExperienceInput<'_>) -> ExperienceSummary {
    let detail_badges = detail_badges(&input);
    let next_step = input
        .actions
        .first()
        .cloned()
        .unwrap_or_else(|| "Review the evidence and logs before taking action.".to_string());

    if input.success {
        return success_summary(input, detail_badges, next_step);
    }

    if let Some(failure) = input.failure {
        return decoded_failure_summary(failure, detail_badges, next_step);
    }

    fallback_failure_summary(input, detail_badges, next_step)
}

pub(crate) struct ExperienceInput<'a> {
    pub(crate) success: bool,
    pub(crate) failure: Option<&'a StandardizedFailure>,
    pub(crate) root: &'a RootCause,
    pub(crate) actions: &'a [String],
    pub(crate) failing_ix: Option<&'a InstructionDebugInfo>,
    pub(crate) metadata: &'a TransactionMetadata,
    pub(crate) freshness: &'a FreshnessInfo,
    pub(crate) product: Option<&'a RaydiumProductDebug>,
}

fn success_summary(
    input: ExperienceInput<'_>,
    detail_badges: Vec<String>,
    next_step: String,
) -> ExperienceSummary {
    let message = if has_fetch_warnings(input.metadata) {
        "The transaction landed successfully. Some supporting account or decode evidence was incomplete, so use the warnings when comparing details.".to_string()
    } else {
        "The transaction landed and the on-chain metadata reports no execution error.".to_string()
    };

    ExperienceSummary {
        tone: "success".to_string(),
        status_label: "Landed".to_string(),
        headline: "Transaction completed successfully".to_string(),
        message,
        next_step,
        detail_badges,
    }
}

fn decoded_failure_summary(
    failure: &StandardizedFailure,
    detail_badges: Vec<String>,
    next_step: String,
) -> ExperienceSummary {
    let program = failure.program_label.as_deref().unwrap_or("the program");
    let code = failure
        .code_hex
        .as_deref()
        .map(|hex| format!(" with code {hex}"))
        .unwrap_or_default();

    ExperienceSummary {
        tone: severity_tone(&failure.severity).to_string(),
        status_label: "Failed".to_string(),
        headline: format!("{program} rejected the transaction{code}"),
        message: failure.user_message.clone(),
        next_step,
        detail_badges,
    }
}

fn fallback_failure_summary(
    input: ExperienceInput<'_>,
    detail_badges: Vec<String>,
    next_step: String,
) -> ExperienceSummary {
    let location = input
        .failing_ix
        .map(|ix| format!(" at instruction #{} ({})", ix.index, ix.program_label))
        .unwrap_or_default();

    ExperienceSummary {
        tone: "danger".to_string(),
        status_label: "Failed".to_string(),
        headline: format!("Transaction failed{location}"),
        message: input.root.summary.clone(),
        next_step,
        detail_badges,
    }
}

fn detail_badges(input: &ExperienceInput<'_>) -> Vec<String> {
    let mut badges = Vec::new();
    if let Some(product) = input.product {
        badges.push(format!("Product: {:?}", product.product));
        if let Some(phase) = &product.phase {
            badges.push(format!("Phase: {phase:?}"));
        }
    }
    if let Some(ix) = input.failing_ix {
        badges.push(format!("Instruction #{}", ix.index));
        badges.push(ix.program_label.clone());
    }
    if input.metadata.uses_address_lookup_tables {
        badges.push(format!(
            "ALT accounts: {}",
            input.metadata.loaded_writable_account_count
                + input.metadata.loaded_readonly_account_count
        ));
    }
    if has_fetch_warnings(input.metadata) {
        badges.push(format!(
            "{} fetch warning(s)",
            input.metadata.fetch_warnings.len()
        ));
    }
    if let Some(age) = input.freshness.slot_age {
        if age > 10_000 {
            badges.push(format!("Stale by {age} slots"));
        }
    }
    badges
}

fn has_fetch_warnings(metadata: &TransactionMetadata) -> bool {
    !metadata.fetch_warnings.is_empty()
}

fn severity_tone(severity: &str) -> &'static str {
    match severity {
        "warning" | "warn" => "warning",
        "info" => "neutral",
        _ => "danger",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_copy_is_friendly() {
        let root = RootCause::default();
        let meta = TransactionMetadata::default();
        let freshness = FreshnessInfo::default();
        let summary = build_experience_summary(ExperienceInput {
            success: true,
            failure: None,
            root: &root,
            actions: &["No retry needed.".to_string()],
            failing_ix: None,
            metadata: &meta,
            freshness: &freshness,
            product: None,
        });

        assert_eq!(summary.tone, "success");
        assert!(summary.headline.contains("completed successfully"));
    }
}
