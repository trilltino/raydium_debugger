//! Transaction fetch and decode helpers, including versioned transaction fallback.

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcTransactionConfig};
use solana_commitment_config::CommitmentConfig;
use solana_sdk::{
    pubkey::Pubkey,
    signature::Signature,
    transaction::{TransactionVersion, VersionedTransaction},
};
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction, UiInstruction,
    UiLoadedAddresses, UiMessage, UiParsedInstruction, UiTransactionEncoding,
};

pub const MAX_SUPPORTED_TX_VERSION: u8 = 1;

/// Fully fetched transaction plus decoded message details used by the debugger.
#[derive(Debug)]
pub struct FetchedSolanaTransaction {
    pub signature: Signature,
    pub confirmed: EncodedConfirmedTransactionWithStatusMeta,
    pub decoded: Option<VersionedTransaction>,
    pub parsed_message: Option<ParsedTransactionMessage>,
    pub version: String,
    pub static_account_keys: Vec<Pubkey>,
    pub account_keys: Vec<String>,
    pub loaded_writable_accounts: Vec<String>,
    pub loaded_readonly_accounts: Vec<String>,
    pub transaction_size_bytes: Option<usize>,
    pub fetch_warnings: Vec<String>,
    pub rpc_v1_fetch_supported: bool,
}

/// Best-effort message data recovered from JSON/JSON-parsed transaction output.
#[derive(Debug, Clone, Default)]
pub struct ParsedTransactionMessage {
    pub account_keys: Vec<String>,
    pub payer: Option<String>,
    pub recent_blockhash: Option<String>,
    pub required_signatures: usize,
    pub readonly_signed_accounts: u8,
    pub readonly_unsigned_accounts: u8,
    pub instructions: Vec<ParsedTransactionInstruction>,
}

/// Best-effort top-level instruction data from RPC JSON output.
#[derive(Debug, Clone, Default)]
pub struct ParsedTransactionInstruction {
    pub index: usize,
    pub program_id: String,
    pub account_indexes: Vec<u8>,
    pub accounts: Vec<String>,
    pub data_base58: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Desired transaction format when another caller builds transactions.
pub enum TransactionBuildPolicy {
    Legacy,
    V0,
    V1,
    #[default]
    Auto,
}

pub fn v1_read_required() -> bool {
    std::env::var("RAYDIUM_DEBUGGER_TX_V1_READ_REQUIRED")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// RPC config that requests base64 transaction data with version-1 support.
pub fn transaction_fetch_config() -> RpcTransactionConfig {
    RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(MAX_SUPPORTED_TX_VERSION),
    }
}

fn parsed_transaction_fetch_config() -> RpcTransactionConfig {
    RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::JsonParsed),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(MAX_SUPPORTED_TX_VERSION),
    }
}

/// Fetches and decodes a transaction while surfacing unsupported-version RPCs clearly.
pub fn fetch_transaction_v1_aware(
    rpc: &RpcClient,
    signature: &Signature,
) -> anyhow::Result<FetchedSolanaTransaction> {
    let confirmed = rpc
        .get_transaction_with_config(signature, transaction_fetch_config())
        .map_err(|err| transaction_fetch_error(signature, &err.to_string()))
        .with_context(|| format!("transaction fetch failed for {signature}"))?;

    let decoded = confirmed.transaction.transaction.decode();
    let (loaded_writable_accounts, loaded_readonly_accounts) = loaded_address_lists(&confirmed);
    let mut fetch_warnings = Vec::new();

    let parsed_fallback = if decoded.is_none() {
        fetch_warnings.push(
            "Binary transaction decode unavailable; using RPC JSON-parsed fallback. Account owner/rent evidence may be partial.".to_string(),
        );
        match fetch_parsed_message(rpc, signature) {
            Ok(Some(message)) => Some(message),
            Ok(None) => {
                fetch_warnings
                    .push("RPC JSON-parsed fallback returned no parseable message.".to_string());
                None
            }
            Err(error) => {
                fetch_warnings.push(format!("RPC JSON-parsed fallback failed: {error:#}"));
                None
            }
        }
    } else {
        None
    };

    let version = decoded
        .as_ref()
        .map(|tx| transaction_version_label(tx.version()))
        .or_else(|| {
            confirmed
                .transaction
                .version
                .clone()
                .map(transaction_version_label)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let static_account_keys = decoded
        .as_ref()
        .map(|tx| tx.message.static_account_keys().to_vec())
        .unwrap_or_default();
    let account_keys = if decoded.is_some() {
        resolved_account_keys(
            &static_account_keys,
            &loaded_writable_accounts,
            &loaded_readonly_accounts,
        )
    } else {
        parsed_fallback
            .as_ref()
            .map(parsed_account_keys)
            .unwrap_or_default()
    };
    let transaction_size_bytes = decoded
        .as_ref()
        .and_then(|tx| bincode::serialize(tx).ok())
        .map(|bytes| bytes.len());

    Ok(FetchedSolanaTransaction {
        signature: *signature,
        confirmed,
        decoded,
        parsed_message: parsed_fallback,
        version,
        static_account_keys,
        account_keys,
        loaded_writable_accounts,
        loaded_readonly_accounts,
        transaction_size_bytes,
        fetch_warnings,
        rpc_v1_fetch_supported: true,
    })
}

fn resolved_account_keys(
    static_account_keys: &[Pubkey],
    loaded_writable_accounts: &[String],
    loaded_readonly_accounts: &[String],
) -> Vec<String> {
    static_account_keys
        .iter()
        .map(|key| key.to_string())
        .chain(loaded_writable_accounts.iter().cloned())
        .chain(loaded_readonly_accounts.iter().cloned())
        .collect()
}

fn fetch_parsed_message(
    rpc: &RpcClient,
    signature: &Signature,
) -> anyhow::Result<Option<ParsedTransactionMessage>> {
    let confirmed = rpc
        .get_transaction_with_config(signature, parsed_transaction_fetch_config())
        .map_err(|err| transaction_fetch_error(signature, &err.to_string()))
        .with_context(|| format!("JSON-parsed transaction fetch failed for {signature}"))?;
    Ok(parsed_message_from_encoded(
        &confirmed.transaction.transaction,
    ))
}

fn parsed_message_from_encoded(
    transaction: &EncodedTransaction,
) -> Option<ParsedTransactionMessage> {
    let EncodedTransaction::Json(tx) = transaction else {
        return None;
    };
    match &tx.message {
        UiMessage::Parsed(message) => {
            let account_keys: Vec<String> = message
                .account_keys
                .iter()
                .map(|account| account.pubkey.clone())
                .collect();
            let required_signatures = message
                .account_keys
                .iter()
                .filter(|account| account.signer)
                .count();
            let readonly_signed_accounts = message
                .account_keys
                .iter()
                .filter(|account| account.signer && !account.writable)
                .count() as u8;
            let readonly_unsigned_accounts = message
                .account_keys
                .iter()
                .filter(|account| !account.signer && !account.writable)
                .count() as u8;
            let instructions = message
                .instructions
                .iter()
                .enumerate()
                .filter_map(|(index, ix)| parsed_instruction(index, ix, &account_keys))
                .collect();
            Some(ParsedTransactionMessage {
                account_keys: account_keys.clone(),
                payer: account_keys.first().cloned(),
                recent_blockhash: Some(message.recent_blockhash.clone()),
                required_signatures,
                readonly_signed_accounts,
                readonly_unsigned_accounts,
                instructions,
            })
        }
        UiMessage::Raw(message) => {
            let instructions = message
                .instructions
                .iter()
                .enumerate()
                .map(|(index, ix)| ParsedTransactionInstruction {
                    index,
                    program_id: message
                        .account_keys
                        .get(ix.program_id_index as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("account_index_{}", ix.program_id_index)),
                    account_indexes: ix.accounts.clone(),
                    accounts: ix
                        .accounts
                        .iter()
                        .filter_map(|idx| message.account_keys.get(*idx as usize).cloned())
                        .collect(),
                    data_base58: ix.data.clone(),
                })
                .collect();
            Some(ParsedTransactionMessage {
                account_keys: message.account_keys.clone(),
                payer: message.account_keys.first().cloned(),
                recent_blockhash: Some(message.recent_blockhash.clone()),
                required_signatures: 0,
                readonly_signed_accounts: 0,
                readonly_unsigned_accounts: 0,
                instructions,
            })
        }
    }
}

fn parsed_instruction(
    index: usize,
    instruction: &UiInstruction,
    account_keys: &[String],
) -> Option<ParsedTransactionInstruction> {
    match instruction {
        UiInstruction::Compiled(ix) => Some(ParsedTransactionInstruction {
            index,
            program_id: account_keys
                .get(ix.program_id_index as usize)
                .cloned()
                .unwrap_or_else(|| format!("account_index_{}", ix.program_id_index)),
            account_indexes: ix.accounts.clone(),
            accounts: ix
                .accounts
                .iter()
                .filter_map(|idx| account_keys.get(*idx as usize).cloned())
                .collect(),
            data_base58: ix.data.clone(),
        }),
        UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(ix)) => {
            let account_indexes = ix
                .accounts
                .iter()
                .filter_map(|account| account_keys.iter().position(|key| key == account))
                .filter_map(|idx| u8::try_from(idx).ok())
                .collect();
            Some(ParsedTransactionInstruction {
                index,
                program_id: ix.program_id.clone(),
                account_indexes,
                accounts: ix.accounts.clone(),
                data_base58: ix.data.clone(),
            })
        }
        UiInstruction::Parsed(UiParsedInstruction::Parsed(ix)) => {
            Some(ParsedTransactionInstruction {
                index,
                program_id: ix.program_id.clone(),
                account_indexes: Vec::new(),
                accounts: Vec::new(),
                data_base58: String::new(),
            })
        }
    }
}

fn parsed_account_keys(message: &ParsedTransactionMessage) -> Vec<String> {
    if !message.account_keys.is_empty() {
        return message.account_keys.clone();
    }
    let mut keys = Vec::new();
    if let Some(payer) = &message.payer {
        keys.push(payer.clone());
    }
    for instruction in &message.instructions {
        if !keys.iter().any(|key| key == &instruction.program_id) {
            keys.push(instruction.program_id.clone());
        }
        for account in &instruction.accounts {
            if !keys.iter().any(|key| key == account) {
                keys.push(account.clone());
            }
        }
    }
    keys
}

fn loaded_address_lists(
    confirmed: &EncodedConfirmedTransactionWithStatusMeta,
) -> (Vec<String>, Vec<String>) {
    let Some(meta) = confirmed.transaction.meta.as_ref() else {
        return (Vec::new(), Vec::new());
    };
    let loaded = Option::<UiLoadedAddresses>::from(meta.loaded_addresses.clone());
    loaded
        .map(|addresses| (addresses.writable, addresses.readonly))
        .unwrap_or_default()
}

fn transaction_version_label(version: TransactionVersion) -> String {
    match version {
        TransactionVersion::LEGACY => "legacy".to_string(),
        TransactionVersion::Number(n) => format!("v{n}"),
    }
}

fn looks_like_tx_version_support_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("maxsupportedtransactionversion")
        || lower.contains("max supported transaction version")
        || lower.contains("unsupported transaction version")
        || lower.contains("transaction version")
}

fn transaction_fetch_error(signature: &Signature, message: &str) -> anyhow::Error {
    if looks_like_null_transaction_response(message) {
        anyhow!(
            "transaction {signature} was not found on the selected cluster/RPC endpoint; check that the signature belongs to the selected cluster and that the endpoint has transaction history"
        )
    } else if looks_like_tx_version_support_error(message) {
        anyhow!(
            "RPC does not appear to support Solana transaction v1 reads \
             with maxSupportedTransactionVersion={}: {}",
            MAX_SUPPORTED_TX_VERSION,
            message
        )
    } else {
        anyhow!(message.to_string())
    }
}

fn looks_like_null_transaction_response(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("invalid type: null")
        || lower.contains("expected struct encodedconfirmedtransactionwithstatusmeta")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_fetch_config_requests_v1() {
        let cfg = transaction_fetch_config();
        assert_eq!(
            cfg.max_supported_transaction_version,
            Some(MAX_SUPPORTED_TX_VERSION)
        );
        assert_eq!(cfg.encoding, Some(UiTransactionEncoding::Base64));
    }

    #[test]
    fn resolved_keys_include_loaded_alt_addresses() {
        let static_key = Pubkey::new_unique();
        let keys = resolved_account_keys(
            &[static_key],
            &["loaded_writable".to_string()],
            &["loaded_readonly".to_string()],
        );
        assert_eq!(
            keys,
            vec![
                static_key.to_string(),
                "loaded_writable".to_string(),
                "loaded_readonly".to_string()
            ]
        );
    }

    #[test]
    fn null_transaction_response_is_cluster_hint() {
        let sig = Signature::new_unique();
        let err = transaction_fetch_error(
            &sig,
            "invalid type: null, expected struct EncodedConfirmedTransactionWithStatusMeta",
        );

        assert!(err
            .to_string()
            .contains("was not found on the selected cluster/RPC endpoint"));
    }
}
