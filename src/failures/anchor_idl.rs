//! Optional on-chain Anchor IDL lookup for otherwise unknown custom errors.

use std::io::Read;

use flate2::read::ZlibDecoder;
use serde::Deserialize;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

use super::actions::{
    action_checklist_for_message, actions_for_message, category_for_message,
    plain_explanation_for_message, plain_title_for_message, primary_action_for_message,
};
use super::programs::program_label;
use super::types::StandardizedFailure;

const ANCHOR_IDL_SEED: &str = "anchor:idl";
const ANCHOR_IDL_PREFIX_LEN: usize = 44;

/// Attempts to replace an unknown Anchor custom error with an on-chain IDL decode.
pub(crate) fn enrich_with_onchain_anchor_idl(
    rpc: &RpcClient,
    failure: StandardizedFailure,
) -> StandardizedFailure {
    if !should_try_anchor_idl(&failure) {
        return failure;
    }

    let Some(program_id) = failure.program_id.clone() else {
        return failure;
    };
    let Some(code) = failure.code_decimal else {
        return failure;
    };

    match fetch_anchor_idl_error(rpc, &program_id, code) {
        IdlErrorLookup::Found(entry) => anchor_idl_failure(failure, &program_id, code, entry),
        IdlErrorLookup::NotFound(reason) | IdlErrorLookup::Unavailable(reason) => {
            with_idl_evidence(failure, reason)
        }
    }
}

fn should_try_anchor_idl(failure: &StandardizedFailure) -> bool {
    failure.name.is_none()
        && failure.confidence == "low"
        && failure.code_decimal.is_some_and(|code| code >= 6000)
}

fn anchor_idl_failure(
    mut failure: StandardizedFailure,
    program_id: &str,
    code: u32,
    entry: AnchorIdlError,
) -> StandardizedFailure {
    let label = failure
        .program_label
        .clone()
        .unwrap_or_else(|| program_label(program_id).to_string());
    let message = entry.message();

    failure.source = "on-chain Anchor IDL".to_string();
    failure.name = Some(entry.name.clone());
    failure.title = format!("{label} rejected the transaction");
    failure.user_message = format!("{label} failed with {}: {message}", entry.name);
    failure.technical_message = format!("{label} custom error {code} ({code:#x}): {message}");
    failure.category = category_for_message(&message).to_string();
    failure.confidence = "high".to_string();
    failure.decode_status = "decoded_onchain_anchor_idl".to_string();
    failure.missing_artifact = None;
    failure.plain_title = Some(plain_title_for_message(program_id, &message));
    failure.plain_explanation = Some(plain_explanation_for_message(program_id, &message));
    failure.primary_action = Some(primary_action_for_message(program_id, &message));
    failure.action_checklist = action_checklist_for_message(program_id, &message);
    failure.decode_explanation = Some(format!(
        "Matched code {code} ({code:#x}) against this program's on-chain Anchor IDL."
    ));
    push_unique(
        &mut failure.decode_attempts,
        format!("Matched code {code} ({code:#x}) against this program's on-chain Anchor IDL."),
    );
    failure.evidence.push(format!(
        "Decoded custom error {code} from the program's on-chain Anchor IDL."
    ));
    failure.suggested_actions = actions_for_message(program_id, &message);
    failure
}

fn with_idl_evidence(mut failure: StandardizedFailure, reason: String) -> StandardizedFailure {
    failure.decode_status = "missing_onchain_idl".to_string();
    failure.missing_artifact = Some("anchor_idl_source_enum_sdk_or_docs".to_string());
    push_unique(
        &mut failure.decode_attempts,
        "Checked the canonical on-chain Anchor IDL account but it did not provide a usable name for this code.".to_string(),
    );
    failure.decode_explanation = Some(
        "The debugger checked known registries and the canonical on-chain Anchor IDL account, but still could not safely name this custom code."
            .to_string(),
    );
    failure.evidence.push(reason);
    failure.suggested_actions.push(
        "Ask the program integrator for the Anchor IDL JSON, source error enum, SDK error map, or docs entry for this custom code."
            .to_string(),
    );
    failure.primary_action = failure.suggested_actions.first().cloned();
    failure.action_checklist = failure.suggested_actions.clone();
    failure
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn fetch_anchor_idl_error(rpc: &RpcClient, program_id: &str, code: u32) -> IdlErrorLookup {
    let Ok(program_id) = program_id.parse::<Pubkey>() else {
        return IdlErrorLookup::Unavailable("Program id was not a valid pubkey.".to_string());
    };
    let Ok(idl_address) = anchor_idl_address(&program_id) else {
        return IdlErrorLookup::Unavailable(
            "Could not derive canonical Anchor IDL address.".to_string(),
        );
    };
    let data = match rpc.get_account_data(&idl_address) {
        Ok(data) => data,
        Err(_) => {
            return IdlErrorLookup::NotFound(format!(
                "No canonical on-chain Anchor IDL account was found at {idl_address}."
            ));
        }
    };
    let idl = match decode_anchor_idl_account(&data) {
        Ok(idl) => idl,
        Err(error) => {
            return IdlErrorLookup::Unavailable(format!(
                "Canonical Anchor IDL account at {idl_address} could not be decoded: {error}."
            ));
        }
    };
    idl.errors
        .unwrap_or_default()
        .into_iter()
        .find(|entry| entry.code == code)
        .map(IdlErrorLookup::Found)
        .unwrap_or_else(|| {
            IdlErrorLookup::NotFound(format!(
                "Canonical on-chain Anchor IDL was found, but it did not contain custom error {code}."
            ))
        })
}

fn anchor_idl_address(program_id: &Pubkey) -> anyhow::Result<Pubkey> {
    let program_signer = Pubkey::find_program_address(&[], program_id).0;
    Ok(Pubkey::create_with_seed(
        &program_signer,
        ANCHOR_IDL_SEED,
        program_id,
    )?)
}

fn decode_anchor_idl_account(data: &[u8]) -> anyhow::Result<AnchorIdl> {
    anyhow::ensure!(
        data.len() >= ANCHOR_IDL_PREFIX_LEN,
        "IDL account is too short"
    );
    let payload_len = u32::from_le_bytes([data[40], data[41], data[42], data[43]]) as usize;
    let payload_end = ANCHOR_IDL_PREFIX_LEN + payload_len;
    anyhow::ensure!(
        payload_end <= data.len(),
        "IDL payload length exceeds account data"
    );

    let mut decoder = ZlibDecoder::new(&data[ANCHOR_IDL_PREFIX_LEN..payload_end]);
    let mut json = String::new();
    decoder.read_to_string(&mut json)?;
    Ok(serde_json::from_str(&json)?)
}

enum IdlErrorLookup {
    Found(AnchorIdlError),
    NotFound(String),
    Unavailable(String),
}

#[derive(Debug, Deserialize)]
struct AnchorIdl {
    #[serde(default)]
    errors: Option<Vec<AnchorIdlError>>,
}

#[derive(Debug, Deserialize)]
struct AnchorIdlError {
    code: u32,
    name: String,
    #[serde(alias = "msg", alias = "message", default)]
    message: Option<String>,
}

impl AnchorIdlError {
    fn message(&self) -> String {
        self.message.clone().unwrap_or_else(|| {
            "The program did not include an error message in its IDL.".to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;

    use super::*;

    #[test]
    fn decodes_anchor_idl_error_table() {
        let json = r#"{
            "version": "0.1.0",
            "name": "test_program",
            "errors": [
                { "code": 6016, "name": "SlippageExceeded", "msg": "Slippage limit exceeded." }
            ]
        }"#;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(json.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut data = vec![0; ANCHOR_IDL_PREFIX_LEN];
        data[40..44].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
        data.extend_from_slice(&compressed);

        let idl = decode_anchor_idl_account(&data).unwrap();
        let error = idl.errors.unwrap().remove(0);

        assert_eq!(error.code, 6016);
        assert_eq!(error.name, "SlippageExceeded");
        assert_eq!(error.message(), "Slippage limit exceeded.");
    }

    #[test]
    fn anchor_idl_success_marks_failure_as_decoded() {
        let failure = StandardizedFailure {
            source: "solana runtime/log heuristic".to_string(),
            program_id: Some("11111111111111111111111111111111".to_string()),
            program_label: Some("Program".to_string()),
            instruction_index: Some(0),
            code_decimal: Some(6016),
            code_hex: Some("0x1780".to_string()),
            name: None,
            title: "Unknown custom program error".to_string(),
            user_message: "unknown".to_string(),
            technical_message: "technical".to_string(),
            category: "program_rejection".to_string(),
            severity: "error".to_string(),
            confidence: "low".to_string(),
            evidence: vec![],
            suggested_actions: vec![],
            decode_status: "missing_registry".to_string(),
            decode_attempts: vec![],
            missing_artifact: Some("program_idl_source_or_docs".to_string()),
            plain_title: None,
            plain_explanation: None,
            primary_action: None,
            action_checklist: vec![],
            evidence_summary: vec![],
            decode_explanation: None,
        };

        let decoded = anchor_idl_failure(
            failure,
            "11111111111111111111111111111111",
            6016,
            AnchorIdlError {
                code: 6016,
                name: "SlippageExceeded".to_string(),
                message: Some("Slippage limit exceeded.".to_string()),
            },
        );

        assert_eq!(decoded.decode_status, "decoded_onchain_anchor_idl");
        assert_eq!(decoded.name.as_deref(), Some("SlippageExceeded"));
        assert!(decoded.missing_artifact.is_none());
        assert!(decoded
            .decode_explanation
            .as_deref()
            .unwrap_or_default()
            .contains("on-chain Anchor IDL"));
    }
}
