//! Plain-language categories, explanations, and next-action templates.

use super::programs::{ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use super::types::StandardizedFailure;

/// Converts decoded errors into UI categories and concrete next actions.
pub(crate) fn category_for_message(message: &str) -> &'static str {
    let m = message.to_ascii_lowercase();
    if m.contains("slippage") || m.contains("too little") || m.contains("too much") {
        "price_or_slippage"
    } else if m.contains("liquidity")
        || m.contains("fund")
        || m.contains("supply")
        || m.contains("balance")
    {
        "liquidity_or_balance"
    } else if m.contains("owner")
        || m.contains("authority")
        || m.contains("approved")
        || m.contains("sign")
    {
        "authority_or_owner"
    } else if m.contains("account")
        || m.contains("vault")
        || m.contains("mint")
        || m.contains("token program")
    {
        "account_or_token_setup"
    } else if m.contains("overflow") || m.contains("calculate") || m.contains("division") {
        "calculation"
    } else {
        "program_rejection"
    }
}

pub(crate) fn actions_for_message(program_id: &str, message: &str) -> Vec<String> {
    let mut actions = action_checklist_for_message(program_id, message);
    if message.contains("PDA seeds") {
        actions.push("Re-derive the PDA with the exact seeds, bump, and program id used by the deployed program, then compare it with the account passed to the failing instruction.".to_string());
    }
    if message.contains("writable") || message.contains("read-only") {
        actions.push("Fix the client account metas: mark the account writable only when this instruction mutates it, then re-simulate before submitting.".to_string());
    }
    if message.contains("not initialized") || message.contains("does not exist") {
        actions.push("Create or initialize the missing account first; for token accounts, use idempotent ATA creation before the main instruction.".to_string());
    }
    if program_id == ASSOCIATED_TOKEN_PROGRAM_ID {
        actions.push(
            "Derive the ATA with the mint's actual token program and use idempotent ATA creation so existing accounts do not fail the flow."
                .to_string(),
        );
    }
    if program_id == TOKEN_2022_PROGRAM_ID && !is_insufficient_funds(message) {
        actions.push(
            "Inspect the Token-2022 mint extensions. Transfer hooks, fees, or confidential-transfer settings may require extra accounts in the instruction."
                .to_string(),
        );
    }
    dedupe(actions)
}

pub(crate) fn action_checklist_for_message(program_id: &str, message: &str) -> Vec<String> {
    if is_insufficient_funds(message) {
        return insufficient_funds_actions(program_id);
    }

    let category = category_for_message(message);
    match category {
        "price_or_slippage" => vec![
            "Fetch a fresh quote and pool state immediately before rebuilding the transaction."
                .to_string(),
            "Compare the quoted output with the transaction's minimum output or slippage limit."
                .to_string(),
            "Only increase slippage if the user accepts the worse execution price; otherwise reduce size or wait for pool state to improve."
                .to_string(),
        ],
        "liquidity_or_balance" => vec![
            "Check the source token account balance and the fee payer SOL balance before rebuilding the transaction."
                .to_string(),
            "Confirm token decimals and amount scaling so the transaction is not asking to spend more than the account holds."
                .to_string(),
            "Refresh pool liquidity and quote data, then rebuild the transaction from current on-chain state."
                .to_string(),
        ],
        "authority_or_owner" => vec![
            "Confirm the expected wallet, owner, delegate, or PDA authority is the signer for this instruction."
                .to_string(),
            "Compare the owner and signer accounts in the failing instruction with the program's IDL or SDK example."
                .to_string(),
            "Rebuild the transaction with the corrected authority account and re-simulate before submitting."
                .to_string(),
        ],
        "account_or_token_setup" => vec![
            "Check every account passed to the failing instruction against the program's IDL account list."
                .to_string(),
            "Verify ATAs, mints, vaults, owners, token program IDs, and writable flags are the expected ones."
                .to_string(),
            "Create or initialize any missing setup accounts before retrying the business instruction."
                .to_string(),
        ],
        "calculation" => vec![
            "Review amount scaling, token decimals, transfer fees, and pool math inputs used to build the instruction."
                .to_string(),
            "Recompute expected input/output values with current pool state and reject impossible values before signing."
                .to_string(),
        ],
        _ => vec![
            "Open the failing instruction in the program IDL or SDK example and compare the expected accounts with this transaction."
                .to_string(),
            "Use the program logs and custom code to identify the exact validation that rejected the instruction."
                .to_string(),
        ],
    }
}

pub(crate) fn primary_action_for_message(program_id: &str, message: &str) -> String {
    action_checklist_for_message(program_id, message)
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            "Open the failing instruction and compare its accounts with the program IDL or SDK."
                .to_string()
        })
}

pub(crate) fn plain_explanation_for_message(program_id: &str, message: &str) -> String {
    if is_insufficient_funds(message) {
        let account = if program_id == TOKEN_2022_PROGRAM_ID {
            "Token-2022 source account"
        } else if program_id == SPL_TOKEN_PROGRAM_ID {
            "SPL token source account"
        } else {
            "source account"
        };
        return format!(
            "The {account} did not have enough spendable balance for the amount requested by this instruction."
        );
    }

    match category_for_message(message) {
        "price_or_slippage" => "The transaction was built with a price or slippage limit that the current pool state no longer satisfies.".to_string(),
        "liquidity_or_balance" => "The transaction asked the program to move more value than the wallet/account/pool state can currently support.".to_string(),
        "authority_or_owner" => "The instruction used an owner, signer, delegate, or authority account that the program did not accept.".to_string(),
        "account_or_token_setup" => "One of the accounts supplied to the instruction is missing, belongs to the wrong program, or has the wrong writable/setup state.".to_string(),
        "calculation" => "The program rejected a calculated amount or pool math input as invalid.".to_string(),
        _ => format!("The program rejected the instruction with: {message}"),
    }
}

pub(crate) fn plain_title_for_message(program_id: &str, message: &str) -> String {
    if is_insufficient_funds(message) {
        if program_id == TOKEN_2022_PROGRAM_ID {
            return "Token-2022 account needs more funds".to_string();
        }
        if program_id == SPL_TOKEN_PROGRAM_ID {
            return "Token account needs more funds".to_string();
        }
        return "Account needs more funds".to_string();
    }

    match category_for_message(message) {
        "price_or_slippage" => "Quote or slippage limit is no longer valid".to_string(),
        "liquidity_or_balance" => "Balance or liquidity is not enough".to_string(),
        "authority_or_owner" => "Wrong owner, signer, or authority".to_string(),
        "account_or_token_setup" => "Account setup does not match the instruction".to_string(),
        "calculation" => "Amount or pool math input is invalid".to_string(),
        _ => "Program rejected the instruction".to_string(),
    }
}

pub(crate) fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !out.iter().any(|existing| existing == &value) {
            out.push(value);
        }
    }
    out
}

fn insufficient_funds_actions(program_id: &str) -> Vec<String> {
    let mut actions = vec![
        "Find the token account used as the source/input account in the failing instruction."
            .to_string(),
        "Fund that account or reduce the input amount so the transfer can be covered, including fees."
            .to_string(),
        "Rebuild the transaction with a fresh quote and re-simulate before asking the user to sign again."
            .to_string(),
    ];
    if program_id == TOKEN_2022_PROGRAM_ID {
        actions.push(
            "For Token-2022 mints, include transfer-fee and transfer-hook behavior in the balance check; the spendable amount may be lower than the displayed balance."
                .to_string(),
        );
    }
    actions
}

fn is_insufficient_funds(message: &str) -> bool {
    message.to_ascii_lowercase().contains("insufficient funds")
}

pub(crate) fn generic_failure(input: GenericFailureInput<'_>) -> StandardizedFailure {
    let mut evidence = input.evidence;
    let joined = input.logs.join("\n").to_ascii_lowercase();
    let err = input.raw_error.unwrap_or("").to_ascii_lowercase();
    let kind = fallback_kind(&err, &joined, input.code);
    add_fallback_evidence(&mut evidence, input.code, input.compute_units);
    let actions = suggested_fallback_actions(input.code);
    let evidence_summary = fallback_evidence_summary(
        input.ix,
        input.program_label.as_deref(),
        input.code,
        input.compute_units,
    );

    StandardizedFailure {
        source: "solana runtime/log heuristic".to_string(),
        program_id: input.program_id,
        program_label: input.program_label,
        instruction_index: input.ix,
        code_decimal: input.code,
        code_hex: input.code.map(|c| format!("0x{c:x}")),
        name: None,
        title: kind.title.to_string(),
        user_message: kind.message.to_string(),
        technical_message: input
            .raw_error
            .unwrap_or("No raw error string available")
            .to_string(),
        category: kind.category.to_string(),
        severity: "error".to_string(),
        confidence: kind.confidence.to_string(),
        evidence,
        suggested_actions: actions.clone(),
        decode_status: decode_status_for_fallback(input.code).to_string(),
        decode_attempts: decode_attempts_for_fallback(input.code),
        missing_artifact: missing_artifact_for_fallback(input.code).map(str::to_string),
        plain_title: Some(kind.title.to_string()),
        plain_explanation: Some(kind.message.to_string()),
        primary_action: actions.first().cloned(),
        action_checklist: actions,
        evidence_summary,
        decode_explanation: Some(fallback_decode_explanation(input.code)),
    }
}

pub(crate) struct GenericFailureInput<'a> {
    pub(crate) raw_error: Option<&'a str>,
    pub(crate) ix: Option<usize>,
    pub(crate) program_id: Option<String>,
    pub(crate) program_label: Option<String>,
    pub(crate) code: Option<u32>,
    pub(crate) logs: &'a [String],
    pub(crate) compute_units: Option<u64>,
    pub(crate) evidence: Vec<String>,
}

struct FallbackKind {
    title: &'static str,
    message: &'static str,
    category: &'static str,
    confidence: &'static str,
}

fn fallback_kind(err: &str, logs: &str, code: Option<u32>) -> FallbackKind {
    match fallback_case(err, logs, code) {
        FallbackCase::Blockhash => fallback(
            "Transaction expired or was not found",
            "If this interface says the transaction did not land, rebuild it with a fresh recent blockhash, fresh quote, and current account/pool state before asking the user to sign again.",
            "rpc_submission",
            "medium",
        ),
        FallbackCase::Compute => fallback(
            "Transaction exceeded compute limits",
            "The transaction ran out of compute units before it completed.",
            "protocol_rejection",
            "high",
        ),
        FallbackCase::Funds => fallback(
            "Insufficient funds",
            "The fee payer or token account did not have enough balance for this transaction.",
            "liquidity_or_balance",
            "high",
        ),
        FallbackCase::Account => fallback(
            "Invalid or missing account",
            "One of the accounts passed to the failing instruction is missing, has the wrong owner, or has unexpected data.",
            "account_or_token_setup",
            "medium",
        ),
        FallbackCase::Signature => fallback(
            "Missing required signature",
            "A required signer did not sign the transaction or the wrong authority was supplied.",
            "authority_or_owner",
            "high",
        ),
        FallbackCase::InstructionData => fallback(
            "Invalid instruction data",
            "The failing instruction data does not match what the target program expects.",
            "sdk_client_construction",
            "medium",
        ),
        FallbackCase::UnknownCustom => fallback(
            "Unknown custom program error",
            "The failing program emitted a custom error code. This code is program-specific, so the debugger cannot safely name it without that program's IDL, source error enum, SDK error map, or docs.",
            "program_rejection",
            "low",
        ),
        FallbackCase::Unknown => fallback(
            "Transaction failed",
            "The transaction failed, but the available metadata did not identify a specific standardized cause.",
            "unknown",
            "low",
        ),
    }
}

fn suggested_fallback_actions(code: Option<u32>) -> Vec<String> {
    if code.is_some() {
        return vec![
            "Send the program id, custom error code, instruction number, and signature to the program owner.".to_string(),
            "Ask the owner for the Anchor IDL, source error enum, SDK error map, or docs entry for this code.".to_string(),
            "Compare the failing instruction accounts with that program artifact before rebuilding the transaction.".to_string(),
        ];
    }

    vec!["Inspect the failing instruction, CPI tree, and logs before retrying.".to_string()]
}

fn decode_status_for_fallback(code: Option<u32>) -> &'static str {
    if code.is_some() {
        "missing_registry"
    } else {
        "runtime_heuristic"
    }
}

fn decode_attempts_for_fallback(code: Option<u32>) -> Vec<String> {
    if code.is_some() {
        vec![
            "Checked the built-in Solana, SPL Token, Token-2022, Associated Token, and Raydium error lists.".to_string(),
            "Checked Anchor's framework error list for standard Anchor account/constraint errors.".to_string(),
        ]
    } else {
        vec![
            "Read the Solana runtime error and program logs because no custom code was available."
                .to_string(),
        ]
    }
}

fn missing_artifact_for_fallback(code: Option<u32>) -> Option<&'static str> {
    code.map(|_| "program_idl_source_or_docs")
}

enum FallbackCase {
    Blockhash,
    Compute,
    Funds,
    Account,
    Signature,
    InstructionData,
    UnknownCustom,
    Unknown,
}

fn fallback_case(err: &str, logs: &str, code: Option<u32>) -> FallbackCase {
    if err.contains("blockhash") || err.contains("not found") {
        FallbackCase::Blockhash
    } else if logs.contains("computational budget exceeded")
        || logs.contains("compute budget exceeded")
        || err.contains("computationalbudgetexceeded")
    {
        FallbackCase::Compute
    } else if err.contains("insufficientfunds") || logs.contains("insufficient funds") {
        FallbackCase::Funds
    } else if err.contains("accountnotfound")
        || logs.contains("invalid account")
        || logs.contains("account data")
    {
        FallbackCase::Account
    } else if err.contains("missingrequiredsignature") || logs.contains("signature") {
        FallbackCase::Signature
    } else if err.contains("invalidinstructiondata") || logs.contains("invalid instruction") {
        FallbackCase::InstructionData
    } else if code.is_some() {
        FallbackCase::UnknownCustom
    } else {
        FallbackCase::Unknown
    }
}

fn fallback(
    title: &'static str,
    message: &'static str,
    category: &'static str,
    confidence: &'static str,
) -> FallbackKind {
    FallbackKind {
        title,
        message,
        category,
        confidence,
    }
}

fn add_fallback_evidence(
    evidence: &mut Vec<String>,
    code: Option<u32>,
    compute_units: Option<u64>,
) {
    if let Some(code) = code {
        evidence.push(format!("Custom error code: {} ({:#x}).", code, code));
    }
    if let Some(cu) = compute_units {
        evidence.push(format!("Compute units consumed: {cu}"));
    }
}

fn fallback_evidence_summary(
    ix: Option<usize>,
    program_label: Option<&str>,
    code: Option<u32>,
    compute_units: Option<u64>,
) -> Vec<String> {
    let mut summary = Vec::new();
    if let Some(ix) = ix {
        summary.push(format!(
            "Where it failed: instruction #{ix}{}.",
            program_label
                .map(|label| format!(" in {label}"))
                .unwrap_or_default()
        ));
    }
    if let Some(code) = code {
        summary.push(format!(
            "What code came back: custom code {code} ({code:#x})."
        ));
    }
    if let Some(cu) = compute_units {
        summary.push(format!(
            "Resource use: the transaction consumed {cu} compute units before failing."
        ));
    }
    summary
}

fn fallback_decode_explanation(code: Option<u32>) -> String {
    if let Some(code) = code {
        format!("The transaction returned custom code {code} ({code:#x}), but no loaded registry or on-chain IDL gave it a safe name.")
    } else {
        "The debugger used the Solana runtime error and logs because there was no custom program code to map.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_2022_insufficient_funds_actions_are_specific_and_deduped() {
        let actions = actions_for_message(TOKEN_2022_PROGRAM_ID, "Insufficient funds");

        assert!(actions
            .iter()
            .any(|action| action.contains("source/input account")));
        assert!(actions
            .iter()
            .any(|action| action.contains("Token-2022 mints")));
        assert!(!actions
            .iter()
            .any(|action| action.contains("Do not retry blindly")));
        assert_eq!(actions.len(), dedupe(actions.clone()).len());
    }

    #[test]
    fn slippage_actions_are_quote_focused() {
        let actions = actions_for_message("program", "Slippage tolerance exceeded");

        assert!(actions.iter().any(|action| action.contains("fresh quote")));
        assert!(actions
            .iter()
            .any(|action| action.contains("minimum output")));
    }
}
