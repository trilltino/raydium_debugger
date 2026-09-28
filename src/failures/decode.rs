//! Decodes Solana runtime/custom errors into standardized failure objects.

use super::actions::{
    action_checklist_for_message, actions_for_message, category_for_message, generic_failure,
    plain_explanation_for_message, plain_title_for_message, primary_action_for_message,
    GenericFailureInput,
};
use super::programs::*;
use super::types::{FailureCode, StandardizedFailure};
use super::{anchor, native, raydium, token};
use serde::Deserialize;
use std::sync::OnceLock;

/// Decodes raw Solana errors and logs into stable UI-facing failures.
pub fn parse_custom_error_code(error_str: &str) -> Option<u32> {
    if let Some(start) = error_str.find("0x") {
        let end = error_str[start + 2..]
            .find(|c: char| !c.is_ascii_hexdigit())
            .map(|offset| start + 2 + offset)
            .unwrap_or(error_str.len());
        if end > start + 2 {
            return u32::from_str_radix(&error_str[start + 2..end], 16).ok();
        }
    }
    if let Some(start) = error_str.find("Custom(") {
        let tail = &error_str[start + 7..];
        let end = tail.find(')').unwrap_or(tail.len());
        return tail[..end].trim().parse::<u32>().ok();
    }
    None
}

pub fn parse_failing_instruction_index(err: &str) -> Option<usize> {
    let marker = "InstructionError(";
    if let Some(start) = err.find(marker).map(|start| start + marker.len()) {
        let tail = &err[start..];
        let end = tail.find(',')?;
        return tail[..end].trim().parse::<usize>().ok();
    }

    let marker = "Error processing Instruction ";
    let start = err.find(marker)? + marker.len();
    let tail = &err[start..];
    let end = tail.find(':').unwrap_or(tail.len());
    tail[..end].trim().parse::<usize>().ok()
}

pub fn last_failed_log_program(logs: &[String]) -> Option<String> {
    logs.iter().rev().find_map(|line| {
        let rest = line.strip_prefix("Program ")?;
        let (program_id, tail) = rest.split_once(' ')?;
        if tail.starts_with("failed:") || tail.contains(" failed:") {
            Some(program_id.to_string())
        } else {
            None
        }
    })
}

pub fn parse_program_error(code: u32) -> &'static str {
    lookup_code(RAYDIUM_CLMM_PROGRAM_ID, code)
        .or_else(|| lookup_code(RAYDIUM_CPMM_PROGRAM_ID, code))
        .or_else(|| lookup_code(RAYDIUM_LAUNCHLAB_PROGRAM_ID, code))
        .or_else(|| lookup_code(RAYDIUM_AMM_V4_PROGRAM_ID, code))
        .or_else(|| lookup_code(SPL_TOKEN_PROGRAM_ID, code))
        .or_else(|| lookup_anchor_framework_code(code))
        .map(|entry| entry.message)
        .unwrap_or("Unknown program error")
}

pub fn decode_standardized_failure(
    success: bool,
    raw_error: Option<&str>,
    instruction_index: Option<usize>,
    failing_program_id: Option<&str>,
    failing_program_label: Option<&str>,
    logs: &[String],
    compute_units: Option<u64>,
) -> Option<StandardizedFailure> {
    if success {
        return None;
    }

    let raw = raw_error.unwrap_or_default();
    let code = error_code(raw, logs);
    let ix = instruction_index.or_else(|| parse_failing_instruction_index(raw));
    let log_pid = failed_log_program_for_code(logs, code);
    let pid = log_pid.as_deref().or(failing_program_id);
    let program_id = pid.map(str::to_string);
    let label = program_label_for(pid, failing_program_label, log_pid.is_some());
    let evidence = evidence_lines(
        ix,
        failing_program_id,
        log_pid.as_deref(),
        raw_error,
        compute_units,
    );

    if let (Some(pid), Some(code)) = (pid, code) {
        if let Some(entry) = lookup_code(pid, code).or_else(|| lookup_anchor_framework_code(code)) {
            return Some(exact_failure(
                pid, code, entry, program_id, label, ix, evidence,
            ));
        }
    }

    Some(generic_failure(GenericFailureInput {
        raw_error,
        ix,
        program_id,
        program_label: label,
        code,
        logs,
        compute_units,
        evidence,
    }))
}

fn failed_log_program_for_code(logs: &[String], code: Option<u32>) -> Option<String> {
    let failed = failed_log_programs(logs);
    if let Some(code) = code {
        if let Some(program_id) = failed
            .iter()
            .find(|program_id| lookup_code(program_id, code).is_some())
        {
            return Some(program_id.clone());
        }
    }
    failed.into_iter().next_back()
}

fn failed_log_programs(logs: &[String]) -> Vec<String> {
    logs.iter()
        .filter_map(|line| {
            let rest = line.strip_prefix("Program ")?;
            let (program_id, tail) = rest.split_once(' ')?;
            (tail.starts_with("failed:") || tail.contains(" failed:"))
                .then(|| program_id.to_string())
        })
        .collect()
}

fn error_code(raw: &str, logs: &[String]) -> Option<u32> {
    parse_custom_error_code(raw)
        .or_else(|| logs.iter().find_map(|line| parse_custom_error_code(line)))
}

fn program_label_for(
    pid: Option<&str>,
    fallback_label: Option<&str>,
    used_log_program: bool,
) -> Option<String> {
    fallback_label
        .filter(|_| !used_log_program)
        .map(str::to_string)
        .or_else(|| pid.map(program_label).map(str::to_string))
}

fn evidence_lines(
    ix: Option<usize>,
    failing_pid: Option<&str>,
    log_pid: Option<&str>,
    raw_error: Option<&str>,
    compute_units: Option<u64>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(index) = ix {
        lines.push(format!("Instruction #{index} failed."));
    }
    if let Some(pid) = failing_pid {
        lines.push(format!("Failing program: {} ({pid}).", program_label(pid)));
    }
    if let Some(pid) = log_pid {
        lines.push(format!(
            "Last failed log line identifies {} ({pid}).",
            program_label(pid)
        ));
    }
    if let Some(raw) = raw_error {
        lines.push(format!("Raw Solana error: {raw}"));
    }
    if let Some(cu) = compute_units {
        lines.push(format!("Compute units consumed: {cu}"));
    }
    lines
}

fn exact_failure(
    pid: &str,
    code: u32,
    entry: FailureCode,
    program_id: Option<String>,
    program_label: Option<String>,
    ix: Option<usize>,
    evidence: Vec<String>,
) -> StandardizedFailure {
    let message = entry.message;
    let decode_explanation = decode_explanation(pid, code);
    StandardizedFailure {
        source: source_for_program(pid).to_string(),
        program_id,
        program_label,
        instruction_index: ix,
        code_decimal: Some(code),
        code_hex: Some(format!("0x{code:x}")),
        name: error_name(entry),
        title: format!(
            "{} rejected the transaction",
            super::programs::program_label(pid)
        ),
        user_message: ui_message(pid, entry),
        technical_message: format!(
            "{} custom error {} ({:#x}): {}",
            super::programs::program_label(pid),
            code,
            code,
            message
        ),
        category: category_for_message(message).to_string(),
        severity: "error".to_string(),
        confidence: "high".to_string(),
        evidence: evidence.clone(),
        suggested_actions: actions_for_message(pid, message),
        decode_status: "decoded_registry".to_string(),
        decode_attempts: vec![decode_explanation.clone()],
        missing_artifact: None,
        plain_title: Some(plain_title_for_message(pid, message)),
        plain_explanation: Some(plain_explanation_for_message(pid, message)),
        primary_action: Some(primary_action_for_message(pid, message)),
        action_checklist: action_checklist_for_message(pid, message),
        evidence_summary: evidence_summary(ix, pid, code, &evidence),
        decode_explanation: Some(decode_explanation),
    }
}

fn error_name(entry: FailureCode) -> Option<String> {
    (!entry.name.is_empty()).then(|| entry.name.to_string())
}

pub(crate) fn lookup_code(program_id: &str, code: u32) -> Option<FailureCode> {
    if let Some(entry) = lookup_generated_raydium_code(program_id, code) {
        return Some(entry);
    }
    let table = match program_id {
        RAYDIUM_CLMM_PROGRAM_ID => raydium::RAYDIUM_CLMM_ERRORS,
        RAYDIUM_CPMM_PROGRAM_ID | RAYDIUM_CPMM_LEGACY_PROGRAM_ID => raydium::RAYDIUM_CPMM_ERRORS,
        RAYDIUM_LAUNCHLAB_PROGRAM_ID => raydium::RAYDIUM_LAUNCHPAD_ERRORS,
        RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID => {
            raydium::RAYDIUM_AMM_V4_ERRORS
        }
        SYSTEM_PROGRAM_ID => native::SYSTEM_PROGRAM_ERRORS,
        ASSOCIATED_TOKEN_PROGRAM_ID => native::ASSOCIATED_TOKEN_ACCOUNT_ERRORS,
        SPL_TOKEN_PROGRAM_ID | TOKEN_2022_PROGRAM_ID => token::SPL_TOKEN_ERRORS,
        _ => &[],
    };
    table.iter().copied().find(|entry| entry.code == code)
}

fn lookup_generated_raydium_code(program_id: &str, code: u32) -> Option<FailureCode> {
    let product = generated_product_for_program(program_id)?;
    generated_registry()
        .iter()
        .find(|source| source.product == product)
        .and_then(|source| source.errors.iter().find(|entry| entry.code == code))
        .map(|entry| FailureCode {
            code: entry.code,
            name: entry.name,
            message: entry.message,
        })
}

fn generated_product_for_program(program_id: &str) -> Option<&'static str> {
    match program_id {
        RAYDIUM_CLMM_PROGRAM_ID => Some("raydium_clmm"),
        RAYDIUM_CPMM_PROGRAM_ID | RAYDIUM_CPMM_LEGACY_PROGRAM_ID => Some("raydium_cpmm"),
        RAYDIUM_LAUNCHLAB_PROGRAM_ID => Some("raydium_launchpad"),
        RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID => Some("raydium_amm_v4"),
        _ => None,
    }
}

fn generated_registry() -> &'static [GeneratedSource] {
    static REGISTRY: OnceLock<Vec<GeneratedSource>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let raw = include_str!("raydium_registry.generated.json");
            let snapshot: GeneratedSnapshot =
                serde_json::from_str(raw).expect("generated Raydium registry must be valid JSON");
            snapshot
                .sources
                .into_iter()
                .map(|source| GeneratedSource {
                    product: leak(source.product),
                    errors: source
                        .errors
                        .into_iter()
                        .map(|entry| GeneratedFailureCode {
                            code: entry.code,
                            name: leak(entry.name),
                            message: leak(entry.message),
                        })
                        .collect(),
                })
                .collect()
        })
        .as_slice()
}

fn leak(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

#[derive(Deserialize)]
struct GeneratedSnapshot {
    sources: Vec<GeneratedSourceOwned>,
}

#[derive(Deserialize)]
struct GeneratedSourceOwned {
    product: String,
    errors: Vec<GeneratedFailureCodeOwned>,
}

#[derive(Deserialize)]
struct GeneratedFailureCodeOwned {
    code: u32,
    name: String,
    message: String,
}

struct GeneratedSource {
    product: &'static str,
    errors: Vec<GeneratedFailureCode>,
}

struct GeneratedFailureCode {
    code: u32,
    name: &'static str,
    message: &'static str,
}

pub(crate) fn lookup_anchor_framework_code(code: u32) -> Option<FailureCode> {
    anchor::ANCHOR_FRAMEWORK_ERRORS
        .iter()
        .copied()
        .find(|entry| entry.code == code)
}

pub(crate) fn source_for_program(program_id: &str) -> &'static str {
    match program_id {
        RAYDIUM_CLMM_PROGRAM_ID => "raydium-idl/raydium_clmm/raydium_clmm.json",
        RAYDIUM_CPMM_PROGRAM_ID | RAYDIUM_CPMM_LEGACY_PROGRAM_ID => {
            "raydium-idl/raydium_cpmm/raydium_cp_swap.json"
        }
        RAYDIUM_LAUNCHLAB_PROGRAM_ID => "raydium-idl/raydium_launchpad/raydium_launchpad.json",
        RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID => {
            "raydium-amm/program/src/error.rs"
        }
        SYSTEM_PROGRAM_ID => "solana system program error enum",
        ASSOCIATED_TOKEN_PROGRAM_ID => "associated token account program behavior",
        SPL_TOKEN_PROGRAM_ID => "spl-token error enum",
        TOKEN_2022_PROGRAM_ID => "spl-token-2022 error enum",
        _ => "solana runtime",
    }
}

pub(crate) fn ui_message(program_id: &str, entry: FailureCode) -> String {
    let code_name = if entry.name.is_empty() {
        format!("custom error {}", entry.code)
    } else {
        entry.name.to_string()
    };
    format!(
        "{} failed with {code_name}: {}",
        program_label(program_id),
        entry.message
    )
}

fn decode_explanation(program_id: &str, code: u32) -> String {
    format!(
        "Matched code {code} ({code:#x}) against the {} error list.",
        registry_label(program_id)
    )
}

fn registry_label(program_id: &str) -> &'static str {
    match program_id {
        RAYDIUM_CLMM_PROGRAM_ID => "Raydium CLMM",
        RAYDIUM_CPMM_PROGRAM_ID | RAYDIUM_CPMM_LEGACY_PROGRAM_ID => "Raydium CPMM",
        RAYDIUM_LAUNCHLAB_PROGRAM_ID => "Raydium LaunchLab",
        RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID => "Raydium AMM v4",
        SYSTEM_PROGRAM_ID => "Solana System Program",
        ASSOCIATED_TOKEN_PROGRAM_ID => "Associated Token Account",
        SPL_TOKEN_PROGRAM_ID => "SPL Token",
        TOKEN_2022_PROGRAM_ID => "Token-2022",
        _ => "known program",
    }
}

fn evidence_summary(
    ix: Option<usize>,
    pid: &str,
    code: u32,
    raw_evidence: &[String],
) -> Vec<String> {
    let mut summary = Vec::new();
    if let Some(ix) = ix {
        summary.push(format!(
            "Where it failed: instruction #{ix} in {}.",
            program_label(pid)
        ));
    }
    summary.push(format!(
        "What code came back: custom code {code} ({code:#x})."
    ));
    if let Some(line) = raw_evidence
        .iter()
        .find(|line| line.contains("Last failed log line"))
    {
        summary.push(format!(
            "What the logs confirm: {}",
            line.trim_end_matches('.')
        ));
    }
    if let Some(line) = raw_evidence
        .iter()
        .find(|line| line.contains("Compute units consumed"))
    {
        summary.push(format!(
            "Resource use: {} before the failure.",
            line.trim_end_matches('.').to_ascii_lowercase()
        ));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_decimal_custom_codes() {
        assert_eq!(
            parse_custom_error_code("custom program error: 0x1771"),
            Some(6001)
        );
        assert_eq!(
            parse_custom_error_code("InstructionError(2, Custom(6001))"),
            Some(6001)
        );
    }

    #[test]
    fn parses_runtime_instruction_index_shape() {
        assert_eq!(
            parse_failing_instruction_index(
                "Error processing Instruction 1: custom program error: 0x1775"
            ),
            Some(1)
        );
    }

    #[test]
    fn same_code_depends_on_program_id() {
        let clmm = decode_standardized_failure(
            false,
            Some("InstructionError(0, Custom(6001))"),
            Some(0),
            Some(RAYDIUM_CLMM_PROGRAM_ID),
            Some("Raydium CLMM"),
            &[],
            None,
        )
        .unwrap();
        let cpmm = decode_standardized_failure(
            false,
            Some("InstructionError(0, Custom(6001))"),
            Some(0),
            Some(RAYDIUM_CPMM_PROGRAM_ID),
            Some("Raydium CPMM"),
            &[],
            None,
        )
        .unwrap();
        assert_ne!(clmm.name, cpmm.name);
    }

    #[test]
    fn maps_raydium_amm_v4_ordinal_codes() {
        let failure = decode_standardized_failure(
            false,
            Some("custom program error: 0x1e"),
            Some(0),
            Some(RAYDIUM_AMM_V4_PROGRAM_ID),
            Some("Raydium AMM v4"),
            &[],
            None,
        )
        .unwrap();
        assert_eq!(failure.name.as_deref(), Some("ExceededSlippage"));
    }

    #[test]
    fn falls_back_for_unknown_custom_code() {
        let failure = decode_standardized_failure(
            false,
            Some("InstructionError(3, Custom(9999))"),
            Some(3),
            Some("Unknown111111111111111111111111111111111"),
            Some("Unknown Program"),
            &[],
            None,
        )
        .unwrap();
        assert_eq!(failure.title, "Unknown custom program error");
        assert_eq!(failure.code_decimal, Some(9999));
        assert_eq!(failure.decode_status, "missing_registry");
        assert_eq!(
            failure.missing_artifact.as_deref(),
            Some("program_idl_source_or_docs")
        );
        assert!(failure.user_message.contains("program-specific"));
    }

    #[test]
    fn cpi_failed_log_program_overrides_outer_instruction_program() {
        let logs = vec![
            format!("Program {RAYDIUM_CPMM_PROGRAM_ID} invoke [1]"),
            format!("Program {SPL_TOKEN_PROGRAM_ID} invoke [2]"),
            "Program log: Error: insufficient funds".to_string(),
            format!("Program {SPL_TOKEN_PROGRAM_ID} failed: custom program error: 0x1"),
        ];
        let failure = decode_standardized_failure(
            false,
            Some("InstructionError(0, Custom(1))"),
            Some(0),
            Some(RAYDIUM_CPMM_PROGRAM_ID),
            Some("Raydium CPMM"),
            &logs,
            None,
        )
        .unwrap();

        assert_eq!(failure.program_id.as_deref(), Some(SPL_TOKEN_PROGRAM_ID));
        assert_eq!(failure.name.as_deref(), Some("InsufficientFunds"));
        assert_eq!(failure.category, "liquidity_or_balance");
        assert!(failure
            .decode_explanation
            .as_deref()
            .unwrap_or_default()
            .contains("Matched code 1"));
    }

    #[test]
    fn maps_anchor_framework_codes_for_unknown_anchor_programs() {
        let failure = decode_standardized_failure(
            false,
            Some("InstructionError(0, Custom(3012))"),
            Some(0),
            Some("AnchorProg11111111111111111111111111111111"),
            Some("Unknown Program"),
            &[],
            None,
        )
        .unwrap();

        assert_eq!(failure.name.as_deref(), Some("AccountNotInitialized"));
        assert!(failure
            .suggested_actions
            .iter()
            .any(|action| action.contains("Create or initialize")));
    }

    #[test]
    fn maps_system_and_ata_native_codes() {
        let system = decode_standardized_failure(
            false,
            Some("InstructionError(0, Custom(1))"),
            Some(0),
            Some(SYSTEM_PROGRAM_ID),
            Some("System Program"),
            &[],
            None,
        )
        .unwrap();
        assert_eq!(system.name.as_deref(), Some("ResultWithNegativeLamports"));

        let ata = decode_standardized_failure(
            false,
            Some("InstructionError(0, Custom(0))"),
            Some(0),
            Some(ASSOCIATED_TOKEN_PROGRAM_ID),
            Some("Associated Token"),
            &[],
            None,
        )
        .unwrap();
        assert_eq!(ata.name.as_deref(), Some("AccountAlreadyInUse"));
        assert!(ata
            .suggested_actions
            .iter()
            .any(|action| action.contains("idempotent ATA")));
    }

    #[test]
    fn classifies_compute_and_signature_runtime_errors() {
        let compute = decode_standardized_failure(
            false,
            Some("InstructionError(1, ComputationalBudgetExceeded)"),
            Some(1),
            None,
            None,
            &["Program failed: computational budget exceeded".to_string()],
            Some(200_000),
        )
        .unwrap();
        assert_eq!(compute.category, "protocol_rejection");

        let signature = decode_standardized_failure(
            false,
            Some("MissingRequiredSignature"),
            None,
            None,
            None,
            &[],
            None,
        )
        .unwrap();
        assert_eq!(signature.category, "authority_or_owner");
    }
}
