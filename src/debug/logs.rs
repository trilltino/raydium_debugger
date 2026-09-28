//! Program-log parsing plus fallback root-cause classification.
//!
//! Solana logs are used here to build CPI frames, find failed programs, detect
//! compute/account/signature/blockhash patterns, and supply compatibility root
//! causes when no exact `StandardizedFailure` exists. Exact program registries
//! still win before these heuristics.

use serde_json::Value;
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta, UiInstruction, UiParsedInstruction,
};

use crate::failures::{
    program_label, StandardizedFailure, SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};

use super::types::{
    CpiFrame, FreshnessInfo, InstructionDebugInfo, RaydiumPhase, RaydiumProduct,
    RaydiumProductDebug, RootCause, TokenInstructionDetails, TokenInstructionParameter,
    TransactionMetadata,
};

/// Log/CPI parsing plus root-cause fallback heuristics.
pub(crate) fn parse_cpi_tree(logs: &[String]) -> Vec<CpiFrame> {
    logs.iter()
        .filter_map(|line| {
            let rest = line.strip_prefix("Program ")?;
            let mut parts = rest.split_whitespace();
            let program_id = parts.next()?;
            let status = parts.next()?;
            if status == "invoke" {
                let depth = parts
                    .next()
                    .and_then(|d| d.trim_matches(['[', ']']).parse::<usize>().ok())
                    .unwrap_or(1);
                return Some(CpiFrame {
                    depth,
                    program_id: program_id.to_string(),
                    program_label: program_label(program_id).to_string(),
                    status: "invoke".to_string(),
                    message: line.clone(),
                    token_instruction: None,
                });
            }
            if status == "success" || status == "failed:" {
                return Some(CpiFrame {
                    depth: 0,
                    program_id: program_id.to_string(),
                    program_label: program_label(program_id).to_string(),
                    status: if status == "success" {
                        "success"
                    } else {
                        "failed"
                    }
                    .to_string(),
                    message: line.clone(),
                    token_instruction: None,
                });
            }
            None
        })
        .collect()
}

/// Attaches decoded SPL Token/Token-2022 inner-instruction params to CPI invoke frames.
pub(crate) fn attach_token_cpi_params(
    mut frames: Vec<CpiFrame>,
    confirmed: &EncodedConfirmedTransactionWithStatusMeta,
    account_keys: &[String],
) -> Vec<CpiFrame> {
    let mut token_details = token_inner_instruction_details(confirmed, account_keys).into_iter();
    for frame in &mut frames {
        if frame.status == "invoke" && frame.depth > 1 && is_token_program(&frame.program_id) {
            frame.token_instruction = token_details.next();
        }
    }
    frames
}

fn token_inner_instruction_details(
    confirmed: &EncodedConfirmedTransactionWithStatusMeta,
    account_keys: &[String],
) -> Vec<TokenInstructionDetails> {
    let Some(meta) = confirmed.transaction.meta.as_ref() else {
        return Vec::new();
    };
    let Some(inner_groups) = Option::<Vec<_>>::from(meta.inner_instructions.clone()) else {
        return Vec::new();
    };

    inner_groups
        .iter()
        .flat_map(|group| group.instructions.iter())
        .filter_map(|ix| token_instruction_details(ix, account_keys))
        .collect()
}

fn token_instruction_details(
    instruction: &UiInstruction,
    account_keys: &[String],
) -> Option<TokenInstructionDetails> {
    match instruction {
        UiInstruction::Compiled(ix) => {
            let program_id = account_keys.get(ix.program_id_index as usize)?;
            if !is_token_program(program_id) {
                return None;
            }
            let accounts = ix
                .accounts
                .iter()
                .filter_map(|idx| account_keys.get(*idx as usize).cloned())
                .collect::<Vec<_>>();
            let data = bs58::decode(&ix.data).into_vec().ok()?;
            decode_token_instruction_data(&data, &accounts)
        }
        UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(ix)) => {
            if !is_token_program(&ix.program_id) {
                return None;
            }
            let data = bs58::decode(&ix.data).into_vec().ok()?;
            decode_token_instruction_data(&data, &ix.accounts)
        }
        UiInstruction::Parsed(UiParsedInstruction::Parsed(ix)) => {
            if !is_token_program(&ix.program_id) {
                return None;
            }
            parsed_token_instruction_details(&ix.parsed)
        }
    }
}

fn parsed_token_instruction_details(parsed: &Value) -> Option<TokenInstructionDetails> {
    let instruction_type = parsed.get("type")?.as_str()?.to_string();
    let parameters = parsed
        .get("info")
        .and_then(Value::as_object)
        .map(|info| {
            info.iter()
                .map(|(name, value)| TokenInstructionParameter {
                    name: name.clone(),
                    value: json_value_to_display(value),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(TokenInstructionDetails {
        instruction_type,
        parameters,
    })
}

fn decode_token_instruction_data(
    data: &[u8],
    accounts: &[String],
) -> Option<TokenInstructionDetails> {
    let opcode = *data.first()?;
    let amount = u64_param(data.get(1..9));
    let decimals = data.get(9).copied();
    let (instruction_type, parameters) = match opcode {
        3 => ("transfer", transfer_params(accounts, amount)),
        4 => ("approve", approve_params(accounts, amount)),
        7 => ("mint_to", mint_to_params(accounts, amount)),
        8 => ("burn", burn_params(accounts, amount)),
        9 => ("close_account", close_account_params(accounts)),
        12 => (
            "transfer_checked",
            checked_transfer_params(accounts, amount, decimals),
        ),
        13 => (
            "approve_checked",
            checked_approve_params(accounts, amount, decimals),
        ),
        14 => (
            "mint_to_checked",
            checked_mint_to_params(accounts, amount, decimals),
        ),
        15 => (
            "burn_checked",
            checked_burn_params(accounts, amount, decimals),
        ),
        17 => ("sync_native", account_only_params(accounts)),
        _ => (
            "unknown_token_instruction",
            vec![param("opcode", opcode.to_string())],
        ),
    };
    Some(TokenInstructionDetails {
        instruction_type: instruction_type.to_string(),
        parameters,
    })
}

fn transfer_params(accounts: &[String], amount: Option<u64>) -> Vec<TokenInstructionParameter> {
    let mut params = Vec::new();
    push_account(&mut params, "source", accounts, 0);
    push_account(&mut params, "destination", accounts, 1);
    push_account(&mut params, "authority", accounts, 2);
    push_amount(&mut params, amount);
    params
}

fn checked_transfer_params(
    accounts: &[String],
    amount: Option<u64>,
    decimals: Option<u8>,
) -> Vec<TokenInstructionParameter> {
    let mut params = Vec::new();
    push_account(&mut params, "source", accounts, 0);
    push_account(&mut params, "mint", accounts, 1);
    push_account(&mut params, "destination", accounts, 2);
    push_account(&mut params, "authority", accounts, 3);
    push_amount(&mut params, amount);
    push_decimals(&mut params, decimals);
    params
}

fn approve_params(accounts: &[String], amount: Option<u64>) -> Vec<TokenInstructionParameter> {
    let mut params = Vec::new();
    push_account(&mut params, "source", accounts, 0);
    push_account(&mut params, "delegate", accounts, 1);
    push_account(&mut params, "authority", accounts, 2);
    push_amount(&mut params, amount);
    params
}

fn checked_approve_params(
    accounts: &[String],
    amount: Option<u64>,
    decimals: Option<u8>,
) -> Vec<TokenInstructionParameter> {
    let mut params = Vec::new();
    push_account(&mut params, "source", accounts, 0);
    push_account(&mut params, "mint", accounts, 1);
    push_account(&mut params, "delegate", accounts, 2);
    push_account(&mut params, "authority", accounts, 3);
    push_amount(&mut params, amount);
    push_decimals(&mut params, decimals);
    params
}

fn mint_to_params(accounts: &[String], amount: Option<u64>) -> Vec<TokenInstructionParameter> {
    let mut params = Vec::new();
    push_account(&mut params, "mint", accounts, 0);
    push_account(&mut params, "destination", accounts, 1);
    push_account(&mut params, "mint_authority", accounts, 2);
    push_amount(&mut params, amount);
    params
}

fn checked_mint_to_params(
    accounts: &[String],
    amount: Option<u64>,
    decimals: Option<u8>,
) -> Vec<TokenInstructionParameter> {
    let mut params = mint_to_params(accounts, amount);
    push_decimals(&mut params, decimals);
    params
}

fn burn_params(accounts: &[String], amount: Option<u64>) -> Vec<TokenInstructionParameter> {
    let mut params = Vec::new();
    push_account(&mut params, "account", accounts, 0);
    push_account(&mut params, "mint", accounts, 1);
    push_account(&mut params, "authority", accounts, 2);
    push_amount(&mut params, amount);
    params
}

fn checked_burn_params(
    accounts: &[String],
    amount: Option<u64>,
    decimals: Option<u8>,
) -> Vec<TokenInstructionParameter> {
    let mut params = burn_params(accounts, amount);
    push_decimals(&mut params, decimals);
    params
}

fn close_account_params(accounts: &[String]) -> Vec<TokenInstructionParameter> {
    let mut params = Vec::new();
    push_account(&mut params, "account", accounts, 0);
    push_account(&mut params, "destination", accounts, 1);
    push_account(&mut params, "authority", accounts, 2);
    params
}

fn account_only_params(accounts: &[String]) -> Vec<TokenInstructionParameter> {
    let mut params = Vec::new();
    push_account(&mut params, "account", accounts, 0);
    params
}

fn push_account(
    params: &mut Vec<TokenInstructionParameter>,
    name: &str,
    accounts: &[String],
    index: usize,
) {
    if let Some(value) = accounts.get(index) {
        params.push(param(name, value.clone()));
    }
}

fn push_amount(params: &mut Vec<TokenInstructionParameter>, amount: Option<u64>) {
    if let Some(amount) = amount {
        params.push(param("amount", amount.to_string()));
    }
}

fn push_decimals(params: &mut Vec<TokenInstructionParameter>, decimals: Option<u8>) {
    if let Some(decimals) = decimals {
        params.push(param("decimals", decimals.to_string()));
    }
}

fn param(name: &str, value: String) -> TokenInstructionParameter {
    TokenInstructionParameter {
        name: name.to_string(),
        value,
    }
}

fn u64_param(data: Option<&[u8]>) -> Option<u64> {
    let data = data?;
    Some(u64::from_le_bytes(data.try_into().ok()?))
}

fn is_token_program(program_id: &str) -> bool {
    matches!(program_id, SPL_TOKEN_PROGRAM_ID | TOKEN_2022_PROGRAM_ID)
}

fn json_value_to_display(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

pub(crate) fn freshness_note(slot_age: Option<u64>) -> String {
    match slot_age {
        Some(age) if age > 10_000 => {
            format!("Execution slot is {age} slots behind the current RPC slot; compare this with quote/discovery slot before retrying.")
        }
        Some(age) => format!("Execution slot is {age} slots behind the current RPC slot."),
        None => "Current slot unavailable; freshness could not be quantified.".to_string(),
    }
}

pub(crate) fn classify_root_cause(
    success: bool,
    error: Option<&str>,
    failing_instruction: Option<&InstructionDebugInfo>,
    logs: &[String],
    compute_units: Option<u64>,
    metadata: &TransactionMetadata,
    freshness: &FreshnessInfo,
) -> RootCause {
    if success {
        return root(
            "landed_successfully",
            "The transaction landed successfully. No on-chain failure was found.",
            vec!["Transaction metadata has no error.".to_string()],
        );
    }

    let joined = logs.join("\n").to_ascii_lowercase();
    let err = error.unwrap_or("").to_ascii_lowercase();
    let mut evidence = base_evidence(error, failing_instruction);

    if err.contains("blockhash") || err.contains("not found") {
        return root(
            "rpc_submission",
            "The failure is consistent with transaction submission, blockhash, or confirmation state.",
            evidence,
        );
    }
    if joined.contains("computational budget exceeded")
        || joined.contains("compute budget exceeded")
        || err.contains("computationalbudgetexceeded")
    {
        evidence.push(format!("Compute units consumed: {:?}", compute_units));
        if metadata.uses_address_lookup_tables {
            evidence.push(
                "Address lookup tables were used; ALT reduces message size, not compute."
                    .to_string(),
            );
        }
        return root(
            "protocol_rejection",
            "The transaction exhausted compute or hit a protocol-level execution limit.",
            evidence,
        );
    }
    if joined.contains("token-2022")
        || failing_instruction.is_some_and(|ix| ix.program_label == "Token-2022")
    {
        return root(
            "token_program_incompatibility",
            "The failure reached Token-2022; verify extension-specific account requirements for this instruction path.",
            evidence,
        );
    }
    if freshness.slot_age.is_some_and(|age| age > 10_000) {
        evidence.push(freshness.note.clone());
        return root(
            "stale_state",
            "The execution slot is far behind the current observed slot; stale quote or state should be investigated.",
            evidence,
        );
    }
    if joined.contains("invalid account")
        || joined.contains("owner")
        || joined.contains("account data")
        || joined.contains("custom program error")
    {
        return root(
            "sdk_client_construction",
            "The failure looks like an account graph, ownership, layout, or serialized instruction mismatch.",
            evidence,
        );
    }

    root(
        "unknown",
        "The transaction failed, but the available metadata is not enough for a confident classification.",
        evidence,
    )
}

fn base_evidence(error: Option<&str>, failing_ix: Option<&InstructionDebugInfo>) -> Vec<String> {
    let mut evidence = Vec::new();
    if let Some(ix) = failing_ix {
        evidence.push(format!(
            "Instruction #{} failed in {} ({})",
            ix.index, ix.program_label, ix.program_id
        ));
    }
    if let Some(raw) = error {
        evidence.push(format!("Raw Solana error: {raw}"));
    }
    evidence
}

fn root(category: &str, summary: &str, evidence: Vec<String>) -> RootCause {
    RootCause {
        category: category.to_string(),
        summary: summary.to_string(),
        evidence,
    }
}

pub(crate) fn root_cause_from_failure(failure: &StandardizedFailure) -> RootCause {
    RootCause {
        category: failure.category.clone(),
        summary: failure.user_message.clone(),
        evidence: failure.evidence.clone(),
    }
}

pub(crate) fn recommended_actions(
    root_cause: &RootCause,
    failing_instruction: &Option<InstructionDebugInfo>,
    success: bool,
    raydium_product: Option<&RaydiumProductDebug>,
) -> Vec<String> {
    if success {
        return vec![
            "This interface found the transaction on-chain. Do not send the same action again."
                .to_string(),
        ];
    }
    let mut actions = vec![
        "If this interface says the transaction did not land, rebuild it with fresh data before asking the user to sign again."
            .to_string(),
    ];
    if let Some(ix) = failing_instruction {
        actions.push(format!(
            "Inspect instruction #{} account order, signer/writable flags, and discriminator {}.",
            ix.index,
            ix.discriminator.as_deref().unwrap_or("n/a")
        ));
    }
    actions.push(category_action(&root_cause.category).to_string());
    if let Some(action) = launchlab_action(raydium_product) {
        actions.push(action.to_string());
    }
    actions
}

fn category_action(category: &str) -> &'static str {
    match category {
        "token_program_incompatibility" => {
            "Check legacy SPL Token vs Token-2022 mint extensions and required extra accounts."
        }
        "stale_state" => {
            "Rebuild from a fresh quote, recent blockhash, and current pool/account state before asking the user to sign again."
        }
        "protocol_rejection" => {
            "Separate message-size issues from compute issues; ALT usage will not reduce compute."
        }
        "sdk_client_construction" => {
            "Validate expected PDA, owner, data length/layout, discriminator, and semantic fields separately."
        }
        _ => "Use the CPI tree and logs to identify the lowest-level program that rejected execution.",
    }
}

fn launchlab_action(product: Option<&RaydiumProductDebug>) -> Option<&'static str> {
    let product = product.filter(|p| p.product == RaydiumProduct::LaunchLab)?;
    match product.phase {
        Some(RaydiumPhase::Initialize) => Some(
            "For LaunchLab initialize, verify global/platform config PDAs, mint programs, metadata accounts, vesting params, and pool seed derivations.",
        ),
        Some(RaydiumPhase::Buy) | Some(RaydiumPhase::Sell) => Some(
            "For LaunchLab curve trades, compare expected quote/slippage, user token accounts, referral/platform fee accounts, and fresh pool state.",
        ),
        Some(RaydiumPhase::Graduate) => Some(
            "For LaunchLab graduation, verify the curve completion state and CPMM migration accounts before retrying.",
        ),
        _ => Some(
            "For LaunchLab, inspect whether this is initialize, buy, sell, vesting, or graduation, then validate the account list against that path.",
        ),
    }
}
