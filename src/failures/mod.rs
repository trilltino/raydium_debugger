//! Failure-code registries and standardized decoder output.

mod actions;
mod anchor;
mod anchor_idl;
mod decode;
mod native;
mod programs;
mod raydium;
mod token;
mod types;

pub(crate) use anchor_idl::enrich_with_onchain_anchor_idl;
pub use decode::{
    decode_standardized_failure, last_failed_log_program, parse_custom_error_code,
    parse_failing_instruction_index, parse_program_error,
};
pub use programs::{
    program_label, ASSOCIATED_TOKEN_PROGRAM_ID, COMPUTE_BUDGET_PROGRAM_ID, MEMO_PROGRAM_ID,
    RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID, RAYDIUM_AMM_V4_PROGRAM_ID, RAYDIUM_CLMM_PROGRAM_ID,
    RAYDIUM_CPMM_LEGACY_PROGRAM_ID, RAYDIUM_CPMM_PROGRAM_ID, RAYDIUM_LAUNCHLAB_PROGRAM_ID,
    SPL_TOKEN_PROGRAM_ID, SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};
pub use types::StandardizedFailure;
