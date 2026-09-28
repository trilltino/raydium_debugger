//! Helpers for instruction discriminators, account flags, and compute-budget limits.
//!
//! The build pipeline calls this module while turning raw Solana message
//! instructions into UI-ready `InstructionDebugInfo`. It keeps signer/writable
//! inference, ALT-aware account mutability, instruction discriminators, and
//! compute-budget resource parsing out of the orchestration code.

use crate::failures::COMPUTE_BUDGET_PROGRAM_ID;

use super::types::{ComputeBudgetInfo, DeprecatedRequestUnits};

/// Helpers for compiled instruction metadata and message resource limits.
pub(crate) fn instruction_discriminator(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        None
    } else {
        Some(hex::encode(&data[..data.len().min(8)]))
    }
}

pub(crate) fn is_signer(index: usize, required_signatures: usize) -> bool {
    index < required_signatures
}

pub(crate) fn is_writable(
    index: usize,
    account_count: usize,
    static_account_count: usize,
    loaded_writable_account_count: usize,
    required_signatures: usize,
    readonly_signed: u8,
    readonly_unsigned: u8,
) -> bool {
    if index >= account_count {
        return false;
    }
    if index >= static_account_count {
        return index < static_account_count + loaded_writable_account_count;
    }
    if index < required_signatures {
        index < required_signatures.saturating_sub(readonly_signed as usize)
    } else {
        index < static_account_count.saturating_sub(readonly_unsigned as usize)
    }
}

pub(crate) fn v1_resource_limits<'a>(
    instructions: impl IntoIterator<Item = (u8, &'a [u8])>,
    account_keys: &[String],
) -> (Option<u64>, Option<u64>) {
    let budget = compute_budget_info(instructions, account_keys);
    (
        budget.compute_unit_limit,
        budget.loaded_accounts_data_size_limit,
    )
}

pub(crate) fn compute_budget_info<'a>(
    instructions: impl IntoIterator<Item = (u8, &'a [u8])>,
    account_keys: &[String],
) -> ComputeBudgetInfo {
    let mut compute_unit_limit = None;
    let mut compute_unit_price_micro_lamports = None;
    let mut loaded_accounts_data_size_limit = None;
    let mut heap_frame_bytes = None;
    let mut deprecated_request_units = None;
    for (program_id_index, data) in instructions {
        let Some(program_id) = account_keys.get(program_id_index as usize) else {
            continue;
        };
        if program_id != COMPUTE_BUDGET_PROGRAM_ID {
            continue;
        }
        match data {
            [0, rest @ ..] if rest.len() >= 8 => {
                let units = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as u64;
                let additional_fee_lamports =
                    u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]) as u64;
                deprecated_request_units = Some(DeprecatedRequestUnits {
                    units,
                    units_exact: units.to_string(),
                    additional_fee_lamports,
                    additional_fee_lamports_exact: additional_fee_lamports.to_string(),
                });
            }
            [1, rest @ ..] if rest.len() >= 4 => {
                heap_frame_bytes =
                    Some(u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as u64);
            }
            [2, rest @ ..] if rest.len() >= 4 => {
                compute_unit_limit =
                    Some(u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as u64);
            }
            [3, rest @ ..] if rest.len() >= 8 => {
                compute_unit_price_micro_lamports = Some(u64::from_le_bytes([
                    rest[0], rest[1], rest[2], rest[3], rest[4], rest[5], rest[6], rest[7],
                ]));
            }
            [4, rest @ ..] if rest.len() >= 4 => {
                loaded_accounts_data_size_limit =
                    Some(u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as u64);
            }
            _ => {}
        }
    }
    ComputeBudgetInfo {
        compute_unit_limit,
        compute_unit_limit_exact: compute_unit_limit.map(|value| value.to_string()),
        compute_unit_price_micro_lamports,
        compute_unit_price_micro_lamports_exact: compute_unit_price_micro_lamports
            .map(|value| value.to_string()),
        loaded_accounts_data_size_limit,
        loaded_accounts_data_size_limit_exact: loaded_accounts_data_size_limit
            .map(|value| value.to_string()),
        heap_frame_bytes,
        heap_frame_bytes_exact: heap_frame_bytes.map(|value| value.to_string()),
        deprecated_request_units,
    }
}
