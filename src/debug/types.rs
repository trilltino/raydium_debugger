//! Public structured debug output types.
//!
//! These structs are the stable JSON contract shared by the CLI, Axum server,
//! Tauri commands, React frontend, and optional AI context builder. New fields
//! should stay additive so existing integrators can keep consuming older output.

use serde::{Deserialize, Serialize};

use crate::failures::StandardizedFailure;
use crate::rpc::RpcDebugInfo;

/// Complete structured output returned by the transaction debugger.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransactionDebugInfo {
    /// Transaction signature that was inspected.
    pub signature: String,
    /// Slot where the transaction was confirmed.
    pub slot: u64,
    /// Exact slot rendered without JavaScript number precision loss.
    pub slot_exact: String,
    /// Unix timestamp reported by RPC, when available.
    pub timestamp: Option<i64>,
    /// Landing/finalization summary.
    pub status: TransactionStatusSummary,
    /// True when transaction metadata contains no error.
    pub success: bool,
    /// Raw Solana transaction error string.
    pub error: Option<String>,
    /// Message, version, and fetch metadata.
    pub metadata: TransactionMetadata,
    /// Top-level instructions in message order.
    pub outer_instructions: Vec<InstructionDebugInfo>,
    /// Top-level instruction selected as the failure point.
    pub failing_instruction: Option<InstructionDebugInfo>,
    /// Parsed CPI/log frames.
    pub cpi_tree: Vec<CpiFrame>,
    /// Account owner, signer, writable, and balance evidence.
    pub accounts: Vec<AccountEvidence>,
    /// Rent-exemption evidence for fetched accounts.
    pub rent_evidence: Vec<RentEvidence>,
    /// Program logs from transaction metadata.
    pub logs: Vec<String>,
    /// Lamport changes for account keys present in balance metadata.
    pub account_changes: Vec<AccountChange>,
    /// Compute units consumed, when RPC supplies it.
    pub compute_units_consumed: Option<u64>,
    /// Exact compute-unit count rendered without JavaScript number precision loss.
    pub compute_units_consumed_exact: Option<String>,
    /// Transaction fee in lamports.
    pub fee_paid: u64,
    /// Exact transaction fee rendered without JavaScript number precision loss.
    pub fee_paid_exact: String,
    /// Unique invoked program IDs.
    pub program_ids: Vec<String>,
    /// Redacted RPC endpoint metadata.
    pub rpc: RpcDebugInfo,
    /// Provider-level metadata for Triton/custom RPC debugging.
    pub provider: ProviderDebugInfo,
    /// Raydium product/phase classification, when detected.
    pub raydium_product: Option<RaydiumProductDebug>,
    /// Raydium-specific account roles, token movements, and swap evidence.
    pub raydium_context: Option<RaydiumContext>,
    /// Slot freshness note relative to the serving RPC.
    pub freshness: FreshnessInfo,
    /// Friendly status copy for product UIs.
    pub experience: ExperienceSummary,
    /// UI-ready decoded failure.
    pub failure: Option<StandardizedFailure>,
    /// Root-cause summary for compatibility with earlier output.
    pub root_cause: RootCause,
    /// Concrete next actions derived from the decoded failure.
    pub recommended_actions: Vec<String>,
}

/// Provider evidence for the data source used by this debug run.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderDebugInfo {
    pub name: String,
    pub cluster: Option<String>,
    pub rpc_endpoint_redacted: String,
    pub fallback_endpoint_redacted: Option<String>,
    pub grpc_available: bool,
    pub grpc_used: bool,
    pub rate_limit: Option<RateLimitDebugInfo>,
    pub warnings: Vec<String>,
}

/// Rate-limit evidence surfaced by Triton/rpcpool-compatible responses.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RateLimitDebugInfo {
    pub limited: bool,
    pub retry_after_seconds: Option<u64>,
    pub details: Vec<String>,
}

/// High-level transaction landing status.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransactionStatusSummary {
    pub landed: bool,
    pub finalized: bool,
    pub err: Option<String>,
}

/// Message and fetch metadata used to explain debugger confidence.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransactionMetadata {
    pub payer: Option<String>,
    pub recent_blockhash: Option<String>,
    pub required_signatures: usize,
    pub readonly_signed_accounts: u8,
    pub readonly_unsigned_accounts: u8,
    pub transaction_version: String,
    pub max_supported_transaction_version: u8,
    pub rpc_v1_fetch_supported: bool,
    pub transaction_size_bytes: Option<usize>,
    /// Exact transaction size rendered without JavaScript number precision loss.
    pub transaction_size_bytes_exact: Option<String>,
    pub uses_address_lookup_tables: bool,
    pub static_account_count: usize,
    /// Full account count after appending loaded writable and readonly ALT addresses.
    pub resolved_account_count: usize,
    pub loaded_writable_account_count: usize,
    pub loaded_readonly_account_count: usize,
    pub v1_compute_unit_limit: Option<u64>,
    /// Exact v1 compute-unit limit rendered without JavaScript number precision loss.
    pub v1_compute_unit_limit_exact: Option<String>,
    pub v1_loaded_accounts_data_size_limit: Option<u64>,
    /// Exact v1 loaded-account data size limit rendered without JavaScript number precision loss.
    pub v1_loaded_accounts_data_size_limit_exact: Option<String>,
    pub fetch_warnings: Vec<String>,
}

/// A top-level instruction plus decoded account metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionDebugInfo {
    pub index: usize,
    pub program_id: String,
    pub program_label: String,
    pub account_indexes: Vec<u8>,
    pub accounts: Vec<InstructionAccountMeta>,
    pub data_base58: String,
    pub discriminator: Option<String>,
    pub error: Option<String>,
}

/// Account metadata as seen by a top-level instruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionAccountMeta {
    pub index: usize,
    pub pubkey: String,
    pub signer: bool,
    pub writable: bool,
    pub owner: Option<String>,
    pub owner_label: Option<String>,
    pub raydium_role: Option<String>,
    pub raydium_role_confidence: Option<String>,
}

/// Evidence about an account referenced by the transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountEvidence {
    pub index: usize,
    pub pubkey: String,
    pub owner: Option<String>,
    pub owner_label: Option<String>,
    pub executable: Option<bool>,
    pub lamports_pre: Option<u64>,
    /// Exact pre-balance rendered without JavaScript number precision loss.
    pub lamports_pre_exact: Option<String>,
    pub lamports_post: Option<u64>,
    /// Exact post-balance rendered without JavaScript number precision loss.
    pub lamports_post_exact: Option<String>,
    /// Compatibility numeric delta; saturated if it exceeds i64 range.
    pub lamports_change: Option<i64>,
    /// Exact signed lamport delta rendered without precision loss.
    pub lamports_change_exact: Option<String>,
    pub data_len: Option<usize>,
    pub signer: bool,
    pub writable: bool,
}

/// Rent-exemption information for an account.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RentEvidence {
    pub pubkey: String,
    pub lamports: u64,
    /// Exact lamport balance rendered without JavaScript number precision loss.
    pub lamports_exact: String,
    pub data_len: usize,
    pub rent_exempt_minimum: Option<u64>,
    /// Exact rent-exempt minimum rendered without JavaScript number precision loss.
    pub rent_exempt_minimum_exact: Option<String>,
    pub reclaimable_surplus: Option<u64>,
    /// Exact reclaimable surplus rendered without JavaScript number precision loss.
    pub reclaimable_surplus_exact: Option<String>,
    pub below_rent_exempt: Option<bool>,
}

/// Parsed program log frame for invokes, successes, and failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpiFrame {
    pub depth: usize,
    pub program_id: String,
    pub program_label: String,
    pub status: String,
    pub message: String,
    /// Decoded SPL Token/Token-2022 CPI parameters, when transaction metadata provides them.
    pub token_instruction: Option<TokenInstructionDetails>,
}

/// Decoded token instruction details attached to a CPI frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInstructionDetails {
    pub instruction_type: String,
    pub parameters: Vec<TokenInstructionParameter>,
}

/// One display-safe token instruction parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInstructionParameter {
    pub name: String,
    pub value: String,
}

/// Pre/post lamport balance change for one account key.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountChange {
    pub pubkey: String,
    pub pre_balance: u64,
    /// Exact pre-balance rendered without JavaScript number precision loss.
    pub pre_balance_exact: String,
    pub post_balance: u64,
    /// Exact post-balance rendered without JavaScript number precision loss.
    pub post_balance_exact: String,
    /// Compatibility numeric delta; saturated if it exceeds i64 range.
    pub change: i64,
    /// Exact signed lamport delta rendered without precision loss.
    pub change_exact: String,
}

/// Freshness metadata comparing transaction slot to current RPC slot.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FreshnessInfo {
    pub execution_slot: u64,
    /// Exact execution slot rendered without JavaScript number precision loss.
    pub execution_slot_exact: String,
    pub current_slot: Option<u64>,
    /// Exact current slot rendered without JavaScript number precision loss.
    pub current_slot_exact: Option<String>,
    pub slot_age: Option<u64>,
    /// Exact slot age rendered without JavaScript number precision loss.
    pub slot_age_exact: Option<String>,
    pub note: String,
}

/// Product-friendly summary derived from deterministic debugger evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceSummary {
    pub tone: String,
    pub status_label: String,
    pub headline: String,
    pub message: String,
    pub next_step: String,
    pub detail_badges: Vec<String>,
}

impl Default for ExperienceSummary {
    fn default() -> Self {
        Self {
            tone: "neutral".to_string(),
            status_label: "Unknown".to_string(),
            headline: "Transaction status unavailable".to_string(),
            message: "The debugger did not receive enough transaction metadata to explain this result yet.".to_string(),
            next_step: "Check the signature and RPC endpoint, then run the debugger again.".to_string(),
            detail_badges: Vec::new(),
        }
    }
}

/// Raydium product family touched by the transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RaydiumProduct {
    AmmV4,
    Cpmm,
    Clmm,
    LaunchLab,
    TokenProgram,
    Unknown,
}

/// Best-effort LaunchLab phase classification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RaydiumPhase {
    Initialize,
    Buy,
    Sell,
    Graduate,
    UnknownLaunchLab,
}

/// Evidence explaining the Raydium product classification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RaydiumProductDebug {
    pub product: RaydiumProduct,
    pub phase: Option<RaydiumPhase>,
    pub matched_program_ids: Vec<String>,
    pub evidence: Vec<String>,
}

/// Best-effort Raydium business context derived from deterministic tx evidence.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RaydiumContext {
    pub product: Option<RaydiumProduct>,
    pub phase: Option<RaydiumPhase>,
    pub instruction_roles: Vec<RaydiumInstructionRole>,
    pub account_roles: Vec<RaydiumAccountRole>,
    pub token_movements: Vec<TokenMovement>,
    pub swap_summary: Option<RaydiumSwapSummary>,
    pub warnings: Vec<String>,
}

/// Raydium instruction name evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaydiumInstructionRole {
    pub instruction_index: usize,
    pub program_id: String,
    pub instruction_name: String,
    pub role_source: String,
    pub confidence: String,
}

/// Raydium account role evidence for an instruction account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaydiumAccountRole {
    pub instruction_index: usize,
    pub account_index: usize,
    pub pubkey: String,
    pub role: String,
    pub mint: Option<String>,
    pub owner: Option<String>,
    pub writable: bool,
    pub signer: bool,
    pub source: String,
    pub confidence: String,
}

/// Exact token balance movement for one token account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMovement {
    pub account_index: usize,
    pub account: Option<String>,
    pub mint: String,
    pub owner: Option<String>,
    pub program_id: Option<String>,
    pub pre_amount_raw: String,
    pub post_amount_raw: String,
    pub delta_raw: String,
    pub decimals: u8,
    pub ui_pre_amount: String,
    pub ui_post_amount: String,
}

/// Swap-oriented Raydium evidence used by UI and AI.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RaydiumSwapSummary {
    pub route_kind: String,
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub input_amount_raw: Option<String>,
    pub output_amount_raw: Option<String>,
    pub min_output_raw: Option<String>,
    pub max_input_raw: Option<String>,
    pub slippage_result: Option<String>,
    pub transfer_fee_notes: Vec<String>,
    pub route_leg_status: Vec<String>,
}

/// Compact root-cause summary for terminal and legacy JSON consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCause {
    pub category: String,
    pub summary: String,
    pub evidence: Vec<String>,
}

impl Default for RootCause {
    fn default() -> Self {
        Self {
            category: "unknown".to_string(),
            summary: "No failure evidence was available.".to_string(),
            evidence: Vec::new(),
        }
    }
}
