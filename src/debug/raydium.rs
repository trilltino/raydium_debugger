//! Raydium product and LaunchLab phase classification.
//!
//! This module identifies which Raydium family a transaction touched by looking
//! at invoked program IDs and logs. Its product/phase result is reused by
//! recommendations, UI badges, AI context, and the richer Raydium semantic
//! layer.

use crate::failures::{
    program_label, RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID, RAYDIUM_AMM_V4_PROGRAM_ID,
    RAYDIUM_CLMM_PROGRAM_ID, RAYDIUM_CPMM_LEGACY_PROGRAM_ID, RAYDIUM_CPMM_PROGRAM_ID,
    RAYDIUM_LAUNCHLAB_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};

use super::types::{InstructionDebugInfo, RaydiumPhase, RaydiumProduct, RaydiumProductDebug};

/// Raydium-specific product and LaunchLab phase classification.
pub(crate) fn classify_raydium_product(
    instructions: &[InstructionDebugInfo],
    program_ids: &[String],
    logs: &[String],
) -> Option<RaydiumProductDebug> {
    let matched_program_ids: Vec<String> = program_ids
        .iter()
        .filter(|id| is_raydium_or_token_program(id))
        .cloned()
        .collect();
    if matched_program_ids.is_empty() {
        return None;
    }

    let mut evidence = matched_program_ids
        .iter()
        .map(|id| format!("Matched {}", program_label(id)))
        .collect::<Vec<_>>();
    let product = product_from_programs(program_ids);

    let phase = if product == RaydiumProduct::LaunchLab {
        let phase = classify_launchlab_phase(instructions, logs);
        evidence.push(format!("LaunchLab phase: {:?}", phase));
        Some(phase)
    } else {
        None
    };

    Some(RaydiumProductDebug {
        product,
        phase,
        matched_program_ids,
        evidence,
    })
}

fn product_from_programs(program_ids: &[String]) -> RaydiumProduct {
    if has_program(program_ids, &[RAYDIUM_LAUNCHLAB_PROGRAM_ID]) {
        RaydiumProduct::LaunchLab
    } else if has_program(
        program_ids,
        &[RAYDIUM_CPMM_PROGRAM_ID, RAYDIUM_CPMM_LEGACY_PROGRAM_ID],
    ) {
        RaydiumProduct::Cpmm
    } else if has_program(program_ids, &[RAYDIUM_CLMM_PROGRAM_ID]) {
        RaydiumProduct::Clmm
    } else if has_program(
        program_ids,
        &[RAYDIUM_AMM_V4_PROGRAM_ID, RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID],
    ) {
        RaydiumProduct::AmmV4
    } else if has_program(program_ids, &[SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID]) {
        RaydiumProduct::TokenProgram
    } else {
        RaydiumProduct::Unknown
    }
}

fn has_program(program_ids: &[String], candidates: &[&str]) -> bool {
    program_ids
        .iter()
        .any(|id| candidates.iter().any(|candidate| id == candidate))
}

pub(crate) fn is_raydium_or_token_program(program_id: &str) -> bool {
    matches!(
        program_id,
        RAYDIUM_AMM_V4_PROGRAM_ID
            | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID
            | RAYDIUM_CLMM_PROGRAM_ID
            | RAYDIUM_CPMM_PROGRAM_ID
            | RAYDIUM_CPMM_LEGACY_PROGRAM_ID
            | RAYDIUM_LAUNCHLAB_PROGRAM_ID
            | SPL_TOKEN_PROGRAM_ID
            | TOKEN_2022_PROGRAM_ID
    )
}

pub(crate) fn classify_launchlab_phase(
    instructions: &[InstructionDebugInfo],
    logs: &[String],
) -> RaydiumPhase {
    let evidence = logs
        .iter()
        .chain(instructions.iter().map(|ix| &ix.data_base58))
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");

    if evidence.contains("initialize") || evidence.contains("create_platform") {
        RaydiumPhase::Initialize
    } else if evidence.contains("buy") {
        RaydiumPhase::Buy
    } else if evidence.contains("sell") {
        RaydiumPhase::Sell
    } else if evidence.contains("migrate")
        || evidence.contains("graduate")
        || evidence.contains("cpmm")
    {
        RaydiumPhase::Graduate
    } else {
        RaydiumPhase::UnknownLaunchLab
    }
}
