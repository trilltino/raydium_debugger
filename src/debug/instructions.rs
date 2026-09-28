//! Helpers for instruction discriminators, account flags, and compute-budget limits.
//!
//! The build pipeline calls this module while turning raw Solana message
//! instructions into UI-ready `InstructionDebugInfo`. It keeps signer/writable
//! inference, ALT-aware account mutability, instruction discriminators, and
//! compute-budget resource parsing out of the orchestration code.

use crate::failures::COMPUTE_BUDGET_PROGRAM_ID;

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
    let mut compute_unit_limit = None;
    let mut loaded_accounts_data_size_limit = None;
    for (program_id_index, data) in instructions {
        let Some(program_id) = account_keys.get(program_id_index as usize) else {
            continue;
        };
        if program_id != COMPUTE_BUDGET_PROGRAM_ID {
            continue;
        }
        match data {
            [2, rest @ ..] if rest.len() >= 4 => {
                compute_unit_limit =
                    Some(u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as u64);
            }
            [4, rest @ ..] if rest.len() >= 4 => {
                loaded_accounts_data_size_limit =
                    Some(u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as u64);
            }
            _ => {}
        }
    }
    (compute_unit_limit, loaded_accounts_data_size_limit)
}
