//! Transaction debug orchestration from RPC fetch through final report assembly.
//!
//! `debug_transaction` is the main library entrypoint used by the CLI, Axum
//! server, and Tauri commands. It fetches a landed transaction, resolves
//! instructions/accounts, attaches account/rent/token evidence, decodes
//! failures, adds Raydium context, and returns one `TransactionDebugInfo` object
//! for every output path.

use solana_sdk::{account::Account, pubkey::Pubkey, transaction::VersionedTransaction};
use solana_transaction_status::{UiInstruction, UiParsedInstruction};
use std::collections::BTreeSet;

use crate::failures::{
    decode_standardized_failure, enrich_with_onchain_anchor_idl, parse_failing_instruction_index,
    program_label, ASSOCIATED_TOKEN_PROGRAM_ID, COMPUTE_BUDGET_PROGRAM_ID, MEMO_PROGRAM_ID,
    RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID, RAYDIUM_AMM_V4_PROGRAM_ID, RAYDIUM_CLMM_PROGRAM_ID,
    RAYDIUM_CPMM_LEGACY_PROGRAM_ID, RAYDIUM_CPMM_PROGRAM_ID, RAYDIUM_LAUNCHLAB_PROGRAM_ID,
    SPL_TOKEN_PROGRAM_ID, SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};
use crate::rpc::RpcDebugInfo;
use crate::transaction_fetch::{
    fetch_transaction_v1_aware, FetchedSolanaTransaction, ParsedTransactionMessage,
    MAX_SUPPORTED_TX_VERSION,
};

use super::accounts::{
    build_account_evidence, build_rent_evidence, lamport_delta, AccountEvidenceContext,
};
use super::experience::{build_experience_summary, ExperienceInput};
use super::instructions::{
    compute_budget_info, instruction_discriminator, is_signer, is_writable, v1_resource_limits,
};
use super::logs::{
    attach_token_cpi_params, build_execution_tree, classify_root_cause, compute_attribution,
    freshness_note, parse_cpi_tree, recommended_actions, root_cause_from_failure,
};
use super::raydium::classify_raydium_product;
use super::raydium_context::build_raydium_context;
use super::raydium_instructions::{decode as decode_raydium_instruction, protocol_for_program};
use super::types::{
    AccountChange, ComputeBudgetInfo, DecodedInstruction, ExecutionComputeUsage, FreshnessInfo,
    InstructionAccountMeta, InstructionDebugInfo, InstructionSemanticDecode,
    LoadedAccountDataUsage, ProgramInvocationSummary, ProviderDebugInfo, ResourceUsage,
    TransactionDebugInfo, TransactionMetadata, TransactionProgramContext, TransactionSizeUsage,
    TransactionStatusSummary,
};

/// Fetches a transaction and assembles the structured debug report.
pub fn debug_transaction(
    rpc: &solana_client::rpc_client::RpcClient,
    signature: &solana_sdk::signature::Signature,
) -> anyhow::Result<TransactionDebugInfo> {
    let fetched = fetch_transaction_v1_aware(rpc, signature)?;
    let confirmed = &fetched.confirmed;
    let meta = confirmed.transaction.meta.as_ref();
    let success = meta.is_some_and(|m| m.err.is_none());
    let error = meta.and_then(|m| m.err.as_ref()).map(|e| e.to_string());
    let failing_index = error.as_deref().and_then(parse_failing_instruction_index);
    let fee_paid = meta.map(|m| m.fee).unwrap_or(0);
    let compute_units_consumed =
        meta.and_then(|m| Option::<u64>::from(m.compute_units_consumed.clone()));
    let logs: Vec<String> = meta
        .and_then(|m| Option::<Vec<String>>::from(m.log_messages.clone()))
        .unwrap_or_default();
    let pre_token_balances = meta
        .and_then(|m| Option::<Vec<_>>::from(m.pre_token_balances.clone()))
        .unwrap_or_default();
    let post_token_balances = meta
        .and_then(|m| Option::<Vec<_>>::from(m.post_token_balances.clone()))
        .unwrap_or_default();

    let decoded = fetched.decoded.as_ref();
    let account_keys: Vec<String> = fetched.account_keys.clone();
    let account_pubkeys: Vec<Pubkey> = account_keys
        .iter()
        .filter_map(|key| key.parse::<Pubkey>().ok())
        .collect();
    let (account_infos, account_fetch_warning) = fetch_account_infos(rpc, &account_pubkeys);

    let pre_balances = meta.map(|m| m.pre_balances.clone()).unwrap_or_default();
    let post_balances = meta.map(|m| m.post_balances.clone()).unwrap_or_default();
    let account_changes = balance_changes(&account_keys, &pre_balances, &post_balances);

    let mut metadata = decoded
        .map(|tx| build_metadata(tx, &fetched, &account_keys))
        .or_else(|| {
            fetched
                .parsed_message
                .as_ref()
                .map(|message| build_metadata_from_parsed(message, &fetched, &account_keys))
        })
        .unwrap_or_else(|| build_minimal_metadata(&fetched, &account_keys));
    if let Some(warning) = account_fetch_warning {
        metadata.fetch_warnings.push(warning);
    }

    let accounts = build_account_evidence(
        &account_keys,
        &account_infos,
        &pre_balances,
        &post_balances,
        &AccountEvidenceContext {
            static_account_count: metadata.static_account_count,
            loaded_writable_account_count: metadata.loaded_writable_account_count,
            required_signatures: metadata.required_signatures,
            readonly_signed_accounts: metadata.readonly_signed_accounts,
            readonly_unsigned_accounts: metadata.readonly_unsigned_accounts,
        },
    );
    let rent_evidence = build_rent_evidence(rpc, &account_keys, &account_infos);

    let mut outer_instructions: Vec<InstructionDebugInfo> = decoded
        .map(|tx| {
            build_outer_instructions(
                tx,
                &account_keys,
                &account_infos,
                failing_index,
                error.as_deref(),
                &metadata,
            )
        })
        .or_else(|| {
            fetched.parsed_message.as_ref().map(|message| {
                build_outer_instructions_from_parsed(
                    message,
                    &account_keys,
                    &account_infos,
                    failing_index,
                    error.as_deref(),
                    &metadata,
                )
            })
        })
        .unwrap_or_default();

    let cpi_tree = attach_token_cpi_params(parse_cpi_tree(&logs), confirmed, &account_keys);
    let program_ids: Vec<String> = outer_instructions
        .iter()
        .map(|ix| ix.program_id.clone())
        .chain(cpi_tree.iter().map(|frame| frame.program_id.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let program_context = build_program_context(&program_ids);
    let decoded_instructions = build_decoded_instructions(
        decoded,
        fetched.parsed_message.as_ref(),
        confirmed,
        &account_keys,
    );
    let execution_tree = build_execution_tree(&logs, &decoded_instructions, &cpi_tree);
    let compute_budget = decoded
        .map(|tx| {
            compute_budget_info(
                tx.message
                    .instructions()
                    .iter()
                    .map(|ix| (ix.program_id_index, ix.data.as_slice())),
                &account_keys,
            )
        })
        .unwrap_or_default();
    let compute_attribution = compute_attribution(&logs);
    let resource_usage = build_resource_usage(
        compute_units_consumed,
        &compute_budget,
        &metadata,
        &account_infos,
    );
    let raydium_product = classify_raydium_product(&outer_instructions, &program_ids, &logs);
    let raydium_context = build_raydium_context(
        raydium_product.as_ref(),
        &outer_instructions,
        &decoded_instructions,
        &account_keys,
        &pre_token_balances,
        &post_token_balances,
    );
    apply_raydium_roles(&mut outer_instructions, raydium_context.as_ref());
    let failing_instruction = failing_index.and_then(|idx| outer_instructions.get(idx).cloned());
    let current_slot = rpc.get_slot().ok();
    let freshness = build_freshness(confirmed.slot, current_slot);
    let failure = decode_standardized_failure(
        success,
        error.as_deref(),
        failing_index,
        failing_instruction
            .as_ref()
            .map(|ix| ix.program_id.as_str()),
        failing_instruction
            .as_ref()
            .map(|ix| ix.program_label.as_str()),
        &logs,
        compute_units_consumed,
    )
    .map(|failure| enrich_with_onchain_anchor_idl(rpc, failure));
    let (root_cause, recommended_actions) = classify_and_recommend(
        success,
        error.as_deref(),
        failing_instruction.as_ref(),
        &logs,
        compute_units_consumed,
        &metadata,
        &freshness,
        failure.as_ref(),
        &failing_instruction,
        raydium_product.as_ref(),
    );
    let experience = build_experience_summary(ExperienceInput {
        success,
        failure: failure.as_ref(),
        root: &root_cause,
        actions: &recommended_actions,
        failing_ix: failing_instruction.as_ref(),
        metadata: &metadata,
        freshness: &freshness,
        product: raydium_product.as_ref(),
    });

    Ok(TransactionDebugInfo {
        signature: signature.to_string(),
        slot: confirmed.slot,
        slot_exact: confirmed.slot.to_string(),
        timestamp: confirmed.block_time,
        status: TransactionStatusSummary {
            landed: true,
            finalized: false,
            confirmation_status: Some("confirmed".to_string()),
            finalized_known: false,
            err: error.clone(),
        },
        success,
        error,
        metadata,
        outer_instructions,
        failing_instruction,
        cpi_tree,
        decoded_instructions,
        execution_tree,
        accounts,
        rent_evidence,
        logs,
        account_changes,
        compute_units_consumed,
        compute_units_consumed_exact: compute_units_consumed.map(|value| value.to_string()),
        fee_paid,
        fee_paid_exact: fee_paid.to_string(),
        program_ids,
        program_context,
        rpc: RpcDebugInfo::default(),
        provider: ProviderDebugInfo::default(),
        raydium_product: raydium_product.clone(),
        raydium_context,
        freshness,
        compute_budget,
        compute_attribution,
        resource_usage,
        experience,
        failure,
        root_cause,
        recommended_actions,
    })
}

fn build_program_context(program_ids: &[String]) -> TransactionProgramContext {
    let invoked_programs = program_ids
        .iter()
        .map(|program_id| ProgramInvocationSummary {
            program_id: program_id.clone(),
            program_label: program_label(program_id).to_string(),
        })
        .collect();
    TransactionProgramContext {
        invoked_programs,
        token_programs: filter_programs(
            program_ids,
            &[SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID],
        ),
        system_programs: filter_programs(
            program_ids,
            &[
                SYSTEM_PROGRAM_ID,
                ASSOCIATED_TOKEN_PROGRAM_ID,
                COMPUTE_BUDGET_PROGRAM_ID,
                MEMO_PROGRAM_ID,
            ],
        ),
        raydium_programs: filter_programs(
            program_ids,
            &[
                RAYDIUM_AMM_V4_PROGRAM_ID,
                RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID,
                RAYDIUM_CLMM_PROGRAM_ID,
                RAYDIUM_CPMM_PROGRAM_ID,
                RAYDIUM_CPMM_LEGACY_PROGRAM_ID,
                RAYDIUM_LAUNCHLAB_PROGRAM_ID,
            ],
        ),
    }
}

fn filter_programs(program_ids: &[String], candidates: &[&str]) -> Vec<String> {
    program_ids
        .iter()
        .filter(|program_id| candidates.iter().any(|candidate| program_id == candidate))
        .cloned()
        .collect()
}

fn build_decoded_instructions(
    decoded: Option<&VersionedTransaction>,
    parsed_message: Option<&ParsedTransactionMessage>,
    confirmed: &solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta,
    account_keys: &[String],
) -> Vec<DecodedInstruction> {
    let mut instructions = Vec::new();
    if let Some(tx) = decoded {
        for (index, ix) in tx.message.instructions().iter().enumerate() {
            let program_id = account_keys
                .get(ix.program_id_index as usize)
                .cloned()
                .unwrap_or_else(|| format!("account_index_{}", ix.program_id_index));
            let accounts = ix
                .accounts
                .iter()
                .filter_map(|idx| account_keys.get(*idx as usize).cloned())
                .collect::<Vec<_>>();
            let raw_data_base58 = bs58::encode(&ix.data).into_string();
            instructions.push(decoded_instruction(
                format!("outer_{index}"),
                index,
                None,
                "outer",
                program_id,
                ix.accounts.clone(),
                accounts,
                raw_data_base58,
                instruction_discriminator(&ix.data),
            ));
        }
    } else if let Some(message) = parsed_message {
        for ix in &message.instructions {
            instructions.push(decoded_instruction(
                format!("outer_{}", ix.index),
                ix.index,
                None,
                "outer",
                ix.program_id.clone(),
                ix.account_indexes.clone(),
                ix.accounts.clone(),
                ix.data_base58.clone(),
                bs58::decode(&ix.data_base58)
                    .into_vec()
                    .ok()
                    .and_then(|data| instruction_discriminator(&data)),
            ));
        }
    }

    if let Some(meta) = confirmed.transaction.meta.as_ref() {
        if let Some(inner_groups) = Option::<Vec<_>>::from(meta.inner_instructions.clone()) {
            for group in inner_groups {
                for (inner_index, ix) in group.instructions.iter().enumerate() {
                    if let Some(record) = decoded_inner_instruction(
                        group.index as usize,
                        inner_index,
                        ix,
                        account_keys,
                    ) {
                        instructions.push(record);
                    }
                }
            }
        }
    }

    instructions
}

#[allow(clippy::too_many_arguments)]
fn decoded_instruction(
    id: String,
    outer_instruction_index: usize,
    inner_instruction_index: Option<usize>,
    invocation_kind: &str,
    program_id: String,
    account_indexes: Vec<u8>,
    accounts: Vec<String>,
    raw_data_base58: String,
    discriminator: Option<String>,
) -> DecodedInstruction {
    let semantic_decode =
        semantic_decode(&program_id, &raw_data_base58, &accounts, &account_indexes);
    DecodedInstruction {
        id,
        outer_instruction_index,
        inner_instruction_index,
        invocation_kind: invocation_kind.to_string(),
        program_label: program_label(&program_id).to_string(),
        program_id,
        accounts,
        account_indexes,
        raw_data_base58,
        discriminator,
        semantic_decode,
    }
}

fn decoded_inner_instruction(
    outer_index: usize,
    inner_index: usize,
    instruction: &UiInstruction,
    account_keys: &[String],
) -> Option<DecodedInstruction> {
    match instruction {
        UiInstruction::Compiled(ix) => {
            let program_id = account_keys.get(ix.program_id_index as usize)?.clone();
            let accounts = ix
                .accounts
                .iter()
                .filter_map(|idx| account_keys.get(*idx as usize).cloned())
                .collect::<Vec<_>>();
            let data = bs58::decode(&ix.data).into_vec().ok();
            Some(decoded_instruction(
                format!("inner_{outer_index}_{inner_index}"),
                outer_index,
                Some(inner_index),
                "inner",
                program_id,
                ix.accounts.clone(),
                accounts,
                ix.data.clone(),
                data.and_then(|data| instruction_discriminator(&data)),
            ))
        }
        UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(ix)) => {
            let data = bs58::decode(&ix.data).into_vec().ok();
            Some(decoded_instruction(
                format!("inner_{outer_index}_{inner_index}"),
                outer_index,
                Some(inner_index),
                "inner",
                ix.program_id.clone(),
                ix.accounts
                    .iter()
                    .filter_map(|account| account_keys.iter().position(|key| key == account))
                    .filter_map(|idx| u8::try_from(idx).ok())
                    .collect(),
                ix.accounts.clone(),
                ix.data.clone(),
                data.and_then(|data| instruction_discriminator(&data)),
            ))
        }
        UiInstruction::Parsed(UiParsedInstruction::Parsed(ix)) => Some(decoded_instruction(
            format!("inner_{outer_index}_{inner_index}"),
            outer_index,
            Some(inner_index),
            "inner",
            ix.program_id.clone(),
            Vec::new(),
            Vec::new(),
            String::new(),
            None,
        )),
    }
}

fn semantic_decode(
    program_id: &str,
    raw_data_base58: &str,
    accounts: &[String],
    account_indexes: &[u8],
) -> Option<InstructionSemanticDecode> {
    if let Some(decoded) =
        decode_raydium_instruction(program_id, raw_data_base58, accounts, account_indexes)
    {
        return Some(decoded);
    }

    let protocol = protocol_for_program(program_id).or(match program_id {
        RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID => Some("raydium_amm_v4"),
        _ => None,
    })?;
    let discriminator = bs58::decode(raw_data_base58)
        .into_vec()
        .ok()
        .and_then(|data| instruction_discriminator(&data))
        .unwrap_or_else(|| "unknown".to_string());
    Some(InstructionSemanticDecode {
        protocol: protocol.to_string(),
        instruction_name: format!("unknown_{protocol}_instruction"),
        source: "program_id_and_discriminator_observed".to_string(),
        confidence: "unproven".to_string(),
        arguments: vec![super::types::DecodedArgument {
            name: "discriminator".to_string(),
            value: discriminator,
        }],
        accounts: accounts
            .iter()
            .zip(account_indexes.iter())
            .map(|(pubkey, index)| super::types::DecodedAccountRole {
                role: "unclassified".to_string(),
                pubkey: pubkey.clone(),
                account_index: Some(*index as usize),
                source: "exact_role_decoder_not_loaded".to_string(),
                confidence: "unproven".to_string(),
            })
            .collect(),
        remaining_accounts: Vec::new(),
    })
}

fn build_resource_usage(
    compute_units_consumed: Option<u64>,
    compute_budget: &ComputeBudgetInfo,
    metadata: &TransactionMetadata,
    account_infos: &[Option<Account>],
) -> ResourceUsage {
    let observed_account_data_bytes = account_infos
        .iter()
        .filter_map(|account| account.as_ref())
        .map(|account| account.data.len() as u64)
        .sum::<u64>();
    ResourceUsage {
        execution_compute: Some(ExecutionComputeUsage {
            consumed: compute_units_consumed,
            consumed_exact: compute_units_consumed.map(|value| value.to_string()),
            limit: compute_budget.compute_unit_limit,
            limit_exact: compute_budget.compute_unit_limit_exact.clone(),
            price_micro_lamports: compute_budget.compute_unit_price_micro_lamports,
            price_micro_lamports_exact: compute_budget
                .compute_unit_price_micro_lamports_exact
                .clone(),
        }),
        loaded_account_data: Some(LoadedAccountDataUsage {
            limit: compute_budget.loaded_accounts_data_size_limit,
            limit_exact: compute_budget.loaded_accounts_data_size_limit_exact.clone(),
            observed_account_data_bytes: Some(observed_account_data_bytes),
            observed_account_data_bytes_exact: Some(observed_account_data_bytes.to_string()),
        }),
        transaction_size: Some(TransactionSizeUsage {
            serialized_size_bytes: metadata.transaction_size_bytes,
            serialized_size_bytes_exact: metadata.transaction_size_bytes_exact.clone(),
            uses_address_lookup_tables: metadata.uses_address_lookup_tables,
            note: if metadata.uses_address_lookup_tables {
                "Address lookup tables reduce serialized account-address footprint; they do not reduce execution compute.".to_string()
            } else {
                "No address lookup tables were observed in the transaction message.".to_string()
            },
        }),
    }
}

fn fetch_account_infos(
    rpc: &solana_client::rpc_client::RpcClient,
    keys: &[Pubkey],
) -> (Vec<Option<Account>>, Option<String>) {
    if keys.is_empty() {
        (Vec::new(), None)
    } else {
        match rpc.get_multiple_accounts(keys) {
            Ok(accounts) => (accounts, None),
            Err(err) => (
                Vec::new(),
                Some(format!(
                    "Account metadata fetch failed for {} account(s): {err}",
                    keys.len()
                )),
            ),
        }
    }
}

fn balance_changes(keys: &[String], pre: &[u64], post: &[u64]) -> Vec<AccountChange> {
    keys.iter()
        .zip(pre.iter())
        .zip(post.iter())
        .map(|((pubkey, &pre_balance), &post_balance)| {
            let (change, change_exact) = lamport_delta(pre_balance, post_balance);
            AccountChange {
                pubkey: pubkey.clone(),
                pre_balance,
                pre_balance_exact: pre_balance.to_string(),
                post_balance,
                post_balance_exact: post_balance.to_string(),
                change,
                change_exact,
            }
        })
        .collect()
}

fn build_metadata(
    tx: &VersionedTransaction,
    fetched: &FetchedSolanaTransaction,
    keys: &[String],
) -> TransactionMetadata {
    let header = tx.message.header();
    let (cu_limit, data_limit) = v1_resource_limits(
        tx.message
            .instructions()
            .iter()
            .map(|ix| (ix.program_id_index, ix.data.as_slice())),
        keys,
    );

    TransactionMetadata {
        payer: keys.first().cloned(),
        recent_blockhash: Some(tx.message.recent_blockhash().to_string()),
        required_signatures: header.num_required_signatures as usize,
        readonly_signed_accounts: header.num_readonly_signed_accounts,
        readonly_unsigned_accounts: header.num_readonly_unsigned_accounts,
        transaction_version: fetched.version.clone(),
        max_supported_transaction_version: MAX_SUPPORTED_TX_VERSION,
        rpc_v1_fetch_supported: fetched.rpc_v1_fetch_supported,
        transaction_size_bytes: fetched.transaction_size_bytes,
        transaction_size_bytes_exact: fetched
            .transaction_size_bytes
            .map(|value| value.to_string()),
        uses_address_lookup_tables: tx
            .message
            .address_table_lookups()
            .is_some_and(|lookups| !lookups.is_empty()),
        static_account_count: fetched.static_account_keys.len(),
        resolved_account_count: keys.len(),
        loaded_writable_account_count: fetched.loaded_writable_accounts.len(),
        loaded_readonly_account_count: fetched.loaded_readonly_accounts.len(),
        v1_compute_unit_limit: cu_limit,
        v1_compute_unit_limit_exact: cu_limit.map(|value| value.to_string()),
        v1_loaded_accounts_data_size_limit: data_limit,
        v1_loaded_accounts_data_size_limit_exact: data_limit.map(|value| value.to_string()),
        fetch_warnings: fetched.fetch_warnings.clone(),
    }
}

fn build_metadata_from_parsed(
    message: &ParsedTransactionMessage,
    fetched: &FetchedSolanaTransaction,
    keys: &[String],
) -> TransactionMetadata {
    let limits = parsed_v1_resource_limits(message);
    TransactionMetadata {
        payer: message.payer.clone().or_else(|| keys.first().cloned()),
        recent_blockhash: message.recent_blockhash.clone(),
        required_signatures: message.required_signatures,
        readonly_signed_accounts: message.readonly_signed_accounts,
        readonly_unsigned_accounts: message.readonly_unsigned_accounts,
        transaction_version: fetched.version.clone(),
        max_supported_transaction_version: MAX_SUPPORTED_TX_VERSION,
        rpc_v1_fetch_supported: fetched.rpc_v1_fetch_supported,
        transaction_size_bytes: fetched.transaction_size_bytes,
        transaction_size_bytes_exact: fetched
            .transaction_size_bytes
            .map(|value| value.to_string()),
        uses_address_lookup_tables: !fetched.loaded_writable_accounts.is_empty()
            || !fetched.loaded_readonly_accounts.is_empty(),
        static_account_count: message.account_keys.len(),
        resolved_account_count: keys.len(),
        loaded_writable_account_count: fetched.loaded_writable_accounts.len(),
        loaded_readonly_account_count: fetched.loaded_readonly_accounts.len(),
        v1_compute_unit_limit: limits.0,
        v1_compute_unit_limit_exact: limits.0.map(|value| value.to_string()),
        v1_loaded_accounts_data_size_limit: limits.1,
        v1_loaded_accounts_data_size_limit_exact: limits.1.map(|value| value.to_string()),
        fetch_warnings: fetched.fetch_warnings.clone(),
    }
}

fn build_minimal_metadata(
    fetched: &FetchedSolanaTransaction,
    keys: &[String],
) -> TransactionMetadata {
    TransactionMetadata {
        transaction_version: fetched.version.clone(),
        max_supported_transaction_version: MAX_SUPPORTED_TX_VERSION,
        rpc_v1_fetch_supported: fetched.rpc_v1_fetch_supported,
        static_account_count: fetched.static_account_keys.len().max(keys.len()),
        resolved_account_count: keys.len(),
        loaded_writable_account_count: fetched.loaded_writable_accounts.len(),
        loaded_readonly_account_count: fetched.loaded_readonly_accounts.len(),
        fetch_warnings: fetched.fetch_warnings.clone(),
        ..TransactionMetadata::default()
    }
}

fn parsed_v1_resource_limits(message: &ParsedTransactionMessage) -> (Option<u64>, Option<u64>) {
    let mut compute_unit_limit = None;
    let mut loaded_accounts_data_size_limit = None;
    for ix in &message.instructions {
        if ix.program_id != crate::failures::COMPUTE_BUDGET_PROGRAM_ID {
            continue;
        }
        let Ok(data) = bs58::decode(&ix.data_base58).into_vec() else {
            continue;
        };
        match data.as_slice() {
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

fn build_outer_instructions(
    tx: &VersionedTransaction,
    keys: &[String],
    infos: &[Option<Account>],
    failing_index: Option<usize>,
    raw_error: Option<&str>,
    meta: &TransactionMetadata,
) -> Vec<InstructionDebugInfo> {
    tx.message
        .instructions()
        .iter()
        .enumerate()
        .map(|(idx, ix)| {
            let program_id = keys
                .get(ix.program_id_index as usize)
                .cloned()
                .unwrap_or_else(|| format!("account_index_{}", ix.program_id_index));
            InstructionDebugInfo {
                index: idx,
                program_label: program_label(&program_id).to_string(),
                accounts: ix
                    .accounts
                    .iter()
                    .map(|account_index| {
                        instruction_account_meta(*account_index as usize, keys, infos, meta)
                    })
                    .collect(),
                account_indexes: ix.accounts.clone(),
                discriminator: instruction_discriminator(&ix.data),
                data_base58: bs58::encode(&ix.data).into_string(),
                error: (Some(idx) == failing_index)
                    .then(|| raw_error.map(str::to_string))
                    .flatten(),
                program_id,
            }
        })
        .collect()
}

fn build_outer_instructions_from_parsed(
    message: &ParsedTransactionMessage,
    keys: &[String],
    infos: &[Option<Account>],
    failing_index: Option<usize>,
    raw_error: Option<&str>,
    meta: &TransactionMetadata,
) -> Vec<InstructionDebugInfo> {
    message
        .instructions
        .iter()
        .map(|ix| InstructionDebugInfo {
            index: ix.index,
            program_id: ix.program_id.clone(),
            program_label: program_label(&ix.program_id).to_string(),
            account_indexes: ix.account_indexes.clone(),
            accounts: ix
                .accounts
                .iter()
                .enumerate()
                .map(|(pos, account)| {
                    let index = keys.iter().position(|key| key == account).unwrap_or(pos);
                    instruction_account_meta(index, keys, infos, meta)
                })
                .collect(),
            data_base58: ix.data_base58.clone(),
            discriminator: bs58::decode(&ix.data_base58)
                .into_vec()
                .ok()
                .and_then(|data| instruction_discriminator(&data)),
            error: (Some(ix.index) == failing_index)
                .then(|| raw_error.map(str::to_string))
                .flatten(),
        })
        .collect()
}

fn instruction_account_meta(
    index: usize,
    keys: &[String],
    infos: &[Option<Account>],
    meta: &TransactionMetadata,
) -> InstructionAccountMeta {
    let owner = infos
        .get(index)
        .and_then(|a| a.as_ref())
        .map(|a| a.owner.to_string());

    InstructionAccountMeta {
        index,
        pubkey: keys
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("account_index_{index}")),
        signer: is_signer(index, meta.required_signatures),
        writable: is_writable(
            index,
            keys.len(),
            meta.static_account_count,
            meta.loaded_writable_account_count,
            meta.required_signatures,
            meta.readonly_signed_accounts,
            meta.readonly_unsigned_accounts,
        ),
        owner_label: owner.as_deref().map(program_label).map(str::to_string),
        owner,
        raydium_role: None,
        raydium_role_confidence: None,
    }
}

fn apply_raydium_roles(
    instructions: &mut [InstructionDebugInfo],
    context: Option<&super::types::RaydiumContext>,
) {
    let Some(context) = context else {
        return;
    };
    for role in &context.account_roles {
        let Some(instruction) = instructions
            .iter_mut()
            .find(|instruction| instruction.index == role.instruction_index)
        else {
            continue;
        };
        if let Some(account) = instruction
            .accounts
            .iter_mut()
            .find(|account| account.index == role.account_index)
        {
            account.raydium_role = Some(role.role.clone());
            account.raydium_role_confidence = Some(role.confidence.clone());
        }
    }
}

fn build_freshness(execution_slot: u64, current_slot: Option<u64>) -> FreshnessInfo {
    let slot_age = current_slot.map(|slot| slot.saturating_sub(execution_slot));
    FreshnessInfo {
        execution_slot,
        execution_slot_exact: execution_slot.to_string(),
        current_slot,
        current_slot_exact: current_slot.map(|slot| slot.to_string()),
        slot_age,
        slot_age_exact: slot_age.map(|age| age.to_string()),
        note: freshness_note(slot_age),
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_and_recommend(
    success: bool,
    error: Option<&str>,
    failing_ix_ref: Option<&InstructionDebugInfo>,
    logs: &[String],
    compute_units: Option<u64>,
    meta: &TransactionMetadata,
    freshness: &FreshnessInfo,
    failure: Option<&crate::failures::StandardizedFailure>,
    failing_ix: &Option<InstructionDebugInfo>,
    product: Option<&super::types::RaydiumProductDebug>,
) -> (super::types::RootCause, Vec<String>) {
    let root = failure.map(root_cause_from_failure).unwrap_or_else(|| {
        classify_root_cause(
            success,
            error,
            failing_ix_ref,
            logs,
            compute_units,
            meta,
            freshness,
        )
    });
    let actions = failure
        .map(|failure| failure.suggested_actions.clone())
        .unwrap_or_else(|| recommended_actions(&root, failing_ix, success, product));
    (root, dedupe_actions(actions))
}

fn dedupe_actions(actions: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for action in actions {
        if !deduped.iter().any(|existing| existing == &action) {
            deduped.push(action);
        }
    }
    deduped
}
