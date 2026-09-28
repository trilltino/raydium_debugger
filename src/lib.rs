//! Shared Solana/Raydium transaction debugging library.
//!
//! The crate powers the CLI, Axum server, React/Tauri app, and optional AI
//! context builder with one deterministic path for transaction fetching, failure
//! decoding, Raydium semantic context, and human-readable formatting. Runtime
//! debugging uses configured Triton RPC; registry refresh and AI are explicit
//! opt-in paths.

#[cfg(feature = "ai")]
pub mod ai;
pub mod debug;
pub mod failures;
pub mod rpc;
pub mod service;
pub mod transaction_fetch;

#[cfg(feature = "ai")]
pub use ai::{ask_ai, build_ai_prompt, AiDebugContext, AiResponse};
pub use debug::{
    debug_transaction, format_debug_info, AccountChange, AccountEvidence, CpiFrame, FreshnessInfo,
    InstructionAccountMeta, InstructionDebugInfo, ProviderDebugInfo, RateLimitDebugInfo,
    RaydiumAccountRole, RaydiumContext, RaydiumInstructionRole, RaydiumPhase, RaydiumProduct,
    RaydiumProductDebug, RaydiumSwapSummary, RentEvidence, RootCause, TokenInstructionDetails,
    TokenInstructionParameter, TokenMovement, TransactionDebugInfo, TransactionMetadata,
    TransactionStatusSummary,
};
pub use failures::{
    parse_custom_error_code, parse_failing_instruction_index, parse_program_error, program_label,
    StandardizedFailure,
};
pub use rpc::{make_rpc, redact_url, RpcCluster, RpcConfig, RpcDebugInfo, RPC_TIMEOUT};
pub use service::{
    run_ai_request, run_debug_request, run_debug_request_blocking, AiAskRequest, DebugDataMode,
    DebugRequest, DebugResponse,
};
pub use transaction_fetch::{
    fetch_transaction_v1_aware, transaction_fetch_config, v1_read_required,
    FetchedSolanaTransaction, MAX_SUPPORTED_TX_VERSION,
};
