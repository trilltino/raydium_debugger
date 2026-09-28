//! Transaction debugging pipeline and UI-ready output model.
//!
//! This module is the crate's core diagnostic layer. `build` orchestrates RPC
//! transaction fetching and report assembly, while the sibling modules keep
//! account analysis, instruction metadata, log parsing, Raydium semantics, UX
//! summaries, and text formatting separated by responsibility.

mod accounts;
mod build;
mod experience;
mod format;
mod instructions;
mod logs;
mod raydium;
mod raydium_context;
mod types;

pub use build::debug_transaction;
pub use format::format_debug_info;
pub use types::{
    AccountChange, AccountEvidence, ComputeAttribution, ComputeBudgetInfo, CpiFrame,
    DecodedInstruction, ExecutionNode, ExperienceSummary, FreshnessInfo, InstructionAccountMeta,
    InstructionDebugInfo, InstructionSemanticDecode, ProgramInvocationSummary, ProviderDebugInfo,
    RateLimitDebugInfo, RaydiumAccountRole, RaydiumContext, RaydiumInstructionRole, RaydiumPhase,
    RaydiumProduct, RaydiumProductDebug, RaydiumSwapSummary, RentEvidence, ResourceUsage,
    RootCause, TokenInstructionDetails, TokenInstructionParameter, TokenMovement,
    TransactionDebugInfo, TransactionMetadata, TransactionProgramContext, TransactionStatusSummary,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::failures::{
        RAYDIUM_AMM_V4_PROGRAM_ID, RAYDIUM_CLMM_PROGRAM_ID, RAYDIUM_CPMM_PROGRAM_ID,
        RAYDIUM_LAUNCHLAB_PROGRAM_ID,
    };

    #[test]
    fn test_parse_program_error() {
        assert_eq!(
            crate::failures::parse_program_error(0x1771),
            "invalid update amm config flag"
        );
        assert_eq!(
            crate::failures::parse_program_error(0x1e),
            "Exceeded desired slippage limit."
        );
        assert_eq!(
            crate::failures::parse_program_error(0xFFFF),
            "Unknown program error"
        );
    }

    #[test]
    fn test_extract_error_code() {
        assert_eq!(
            crate::failures::parse_custom_error_code("custom program error: 0x1771"),
            Some(6001)
        );
        assert_eq!(
            crate::failures::parse_custom_error_code("Custom(5)"),
            Some(5)
        );
        assert_eq!(
            crate::failures::parse_custom_error_code("no error code"),
            None
        );
    }

    #[test]
    fn parses_cpi_frames() {
        let frames = logs::parse_cpi_tree(&[
            "Program 11111111111111111111111111111111 invoke [1]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
        ]);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].depth, 1);
        assert_eq!(frames[1].status, "success");
    }

    #[test]
    fn labels_raydium_programs() {
        assert_eq!(
            crate::failures::program_label(RAYDIUM_AMM_V4_PROGRAM_ID),
            "Raydium AMM v4"
        );
        assert_eq!(
            crate::failures::program_label(RAYDIUM_CLMM_PROGRAM_ID),
            "Raydium CLMM"
        );
        assert_eq!(
            crate::failures::program_label(RAYDIUM_CPMM_PROGRAM_ID),
            "Raydium CPMM"
        );
        assert_eq!(
            crate::failures::program_label(RAYDIUM_LAUNCHLAB_PROGRAM_ID),
            "Raydium LaunchLab"
        );
    }

    #[test]
    fn classifies_launchlab_phases_from_logs() {
        let empty = Vec::<InstructionDebugInfo>::new();
        assert_eq!(
            raydium::classify_launchlab_phase(
                &empty,
                &["Program log: Instruction: Initialize".to_string()]
            ),
            RaydiumPhase::Initialize
        );
        assert_eq!(
            raydium::classify_launchlab_phase(
                &empty,
                &["Program log: Instruction: BuyExactIn".to_string()]
            ),
            RaydiumPhase::Buy
        );
        assert_eq!(
            raydium::classify_launchlab_phase(
                &empty,
                &["Program log: Instruction: SellExactIn".to_string()]
            ),
            RaydiumPhase::Sell
        );
        assert_eq!(
            raydium::classify_launchlab_phase(
                &empty,
                &["Program log: Instruction: MigrateToCpmm".to_string()]
            ),
            RaydiumPhase::Graduate
        );
    }

    #[test]
    fn classifies_launchlab_product() {
        let programs = vec![RAYDIUM_LAUNCHLAB_PROGRAM_ID.to_string()];
        let product = raydium::classify_raydium_product(
            &[],
            &programs,
            &["Program log: Instruction: Buy".to_string()],
        )
        .expect("LaunchLab program should be classified");

        assert_eq!(product.product, RaydiumProduct::LaunchLab);
        assert_eq!(product.phase, Some(RaydiumPhase::Buy));
        assert_eq!(
            product.matched_program_ids,
            vec![RAYDIUM_LAUNCHLAB_PROGRAM_ID.to_string()]
        );
    }

    #[test]
    fn launchlab_recommendations_are_appended() {
        let root_cause = RootCause {
            category: "sdk_client_construction".to_string(),
            summary: "account graph mismatch".to_string(),
            evidence: vec![],
        };
        let product = RaydiumProductDebug {
            product: RaydiumProduct::LaunchLab,
            phase: Some(RaydiumPhase::Graduate),
            matched_program_ids: vec![RAYDIUM_LAUNCHLAB_PROGRAM_ID.to_string()],
            evidence: vec![],
        };

        let actions = logs::recommended_actions(&root_cause, &None, false, Some(&product));

        assert!(actions
            .iter()
            .any(|action| action.contains("CPMM migration accounts")));
    }
}
