//! Account, balance-change, and rent evidence builders.
//!
//! This module enriches transaction account keys with fetched account metadata,
//! signer/writable flags, lamport deltas, owner labels, and rent-exemption
//! evidence. The output feeds the account tab, evidence summaries, and Raydium
//! role labeling without performing failure decoding itself.

use std::collections::HashMap;

use crate::failures::program_label;

use super::instructions::{is_signer, is_writable};
use super::types::{AccountEvidence, RentEvidence};

pub(crate) struct AccountEvidenceContext {
    pub static_account_count: usize,
    pub loaded_writable_account_count: usize,
    pub required_signatures: usize,
    pub readonly_signed_accounts: u8,
    pub readonly_unsigned_accounts: u8,
}

pub(crate) fn lamport_delta(pre: u64, post: u64) -> (i64, String) {
    let delta = i128::from(post) - i128::from(pre);
    let compat = i64::try_from(delta).unwrap_or_else(|_| {
        if delta.is_negative() {
            i64::MIN
        } else {
            i64::MAX
        }
    });
    (compat, delta.to_string())
}

/// Builds account, rent, and balance evidence around a fetched transaction.
pub(crate) fn build_account_evidence(
    account_keys: &[String],
    account_infos: &[Option<solana_sdk::account::Account>],
    pre_balances: &[u64],
    post_balances: &[u64],
    context: &AccountEvidenceContext,
) -> Vec<AccountEvidence> {
    account_keys
        .iter()
        .enumerate()
        .map(|(index, pubkey)| {
            let info = account_infos.get(index).and_then(|a| a.as_ref());
            let owner = info.map(|a| a.owner.to_string());
            let pre = pre_balances.get(index).copied();
            let post = post_balances.get(index).copied();
            let delta = pre.zip(post).map(|(a, b)| lamport_delta(a, b));
            AccountEvidence {
                index,
                pubkey: pubkey.clone(),
                owner_label: owner.as_deref().map(program_label).map(str::to_string),
                owner,
                executable: info.map(|a| a.executable),
                data_len: info.map(|a| a.data.len()),
                lamports_pre: pre,
                lamports_pre_exact: pre.map(|value| value.to_string()),
                lamports_post: post,
                lamports_post_exact: post.map(|value| value.to_string()),
                lamports_change: delta.as_ref().map(|(compat, _)| *compat),
                lamports_change_exact: delta.map(|(_, exact)| exact),
                signer: is_signer(index, context.required_signatures),
                writable: is_writable(
                    index,
                    account_keys.len(),
                    context.static_account_count,
                    context.loaded_writable_account_count,
                    context.required_signatures,
                    context.readonly_signed_accounts,
                    context.readonly_unsigned_accounts,
                ),
            }
        })
        .collect()
}

pub(crate) fn build_rent_evidence(
    rpc: &solana_client::rpc_client::RpcClient,
    account_keys: &[String],
    account_infos: &[Option<solana_sdk::account::Account>],
) -> Vec<RentEvidence> {
    let mut minimum_by_len = HashMap::<usize, Option<u64>>::new();
    account_keys
        .iter()
        .zip(account_infos.iter())
        .filter_map(|(pubkey, info)| {
            let info = info.as_ref()?;
            let data_len = info.data.len();
            let minimum = *minimum_by_len
                .entry(data_len)
                .or_insert_with(|| rpc.get_minimum_balance_for_rent_exemption(data_len).ok());
            Some(RentEvidence {
                pubkey: pubkey.clone(),
                lamports: info.lamports,
                lamports_exact: info.lamports.to_string(),
                data_len,
                rent_exempt_minimum: minimum,
                rent_exempt_minimum_exact: minimum.map(|value| value.to_string()),
                reclaimable_surplus: minimum.map(|min| info.lamports.saturating_sub(min)),
                reclaimable_surplus_exact: minimum
                    .map(|min| info.lamports.saturating_sub(min).to_string()),
                below_rent_exempt: minimum.map(|min| info.lamports < min),
            })
        })
        .collect()
}
