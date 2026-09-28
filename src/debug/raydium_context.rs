//! Raydium-specific account roles, token movements, and swap summaries.
//!
//! After the generic Solana transaction is decoded, this module adds
//! Raydium-aware business context: likely account roles, token balance movement,
//! swap input/output evidence, best-effort slippage notes, and explicit warnings
//! when a role or limit cannot be proven from deterministic transaction data.

use std::collections::BTreeMap;

use solana_transaction_status::{option_serializer::OptionSerializer, UiTransactionTokenBalance};

use crate::failures::{
    RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID, RAYDIUM_AMM_V4_PROGRAM_ID, RAYDIUM_CLMM_PROGRAM_ID,
    RAYDIUM_CPMM_LEGACY_PROGRAM_ID, RAYDIUM_CPMM_PROGRAM_ID, RAYDIUM_LAUNCHLAB_PROGRAM_ID,
    TOKEN_2022_PROGRAM_ID,
};

use super::types::{
    InstructionDebugInfo, RaydiumAccountRole, RaydiumContext, RaydiumInstructionRole,
    RaydiumProductDebug, RaydiumSwapSummary, TokenMovement,
};

type TokenBalancePair<'a> = (
    Option<&'a UiTransactionTokenBalance>,
    Option<&'a UiTransactionTokenBalance>,
);

/// Builds Raydium-specific account roles and token movement diagnostics.
pub(crate) fn build_raydium_context(
    product: Option<&RaydiumProductDebug>,
    instructions: &[InstructionDebugInfo],
    account_keys: &[String],
    pre_token_balances: &[UiTransactionTokenBalance],
    post_token_balances: &[UiTransactionTokenBalance],
) -> Option<RaydiumContext> {
    let product = product?;
    let mut warnings = Vec::new();
    let movements = token_movements(account_keys, pre_token_balances, post_token_balances);
    let mut instruction_roles = Vec::new();
    let mut account_roles = Vec::new();

    for instruction in instructions {
        let Some(role_table) = role_table(&instruction.program_id, instruction.accounts.len())
        else {
            if is_raydium_program(&instruction.program_id) {
                warnings.push(format!(
                    "Vault/token role could not be proven for instruction #{}; no static layout matched {} account(s).",
                    instruction.index,
                    instruction.accounts.len()
                ));
            }
            continue;
        };

        instruction_roles.push(RaydiumInstructionRole {
            instruction_index: instruction.index,
            program_id: instruction.program_id.clone(),
            instruction_name: role_table.instruction_name.to_string(),
            role_source: role_table.source.to_string(),
            confidence: role_table.confidence.to_string(),
        });

        for (account, role) in instruction.accounts.iter().zip(role_table.roles.iter()) {
            let movement = movements
                .iter()
                .find(|movement| movement.account_index == account.index);
            account_roles.push(RaydiumAccountRole {
                instruction_index: instruction.index,
                account_index: account.index,
                pubkey: account.pubkey.clone(),
                role: (*role).to_string(),
                mint: movement.map(|movement| movement.mint.clone()),
                owner: account
                    .owner
                    .clone()
                    .or_else(|| movement.and_then(|m| m.owner.clone())),
                writable: account.writable,
                signer: account.signer,
                source: role_table.source.to_string(),
                confidence: role_table.confidence.to_string(),
            });
        }
    }

    let swap_summary = swap_summary(instructions, &movements, &account_roles, &mut warnings);
    let has_context =
        !instruction_roles.is_empty() || !movements.is_empty() || !warnings.is_empty();
    has_context.then(|| RaydiumContext {
        product: Some(product.product.clone()),
        phase: product.phase.clone(),
        instruction_roles,
        account_roles,
        token_movements: movements,
        swap_summary,
        warnings,
    })
}

struct RoleTable {
    instruction_name: &'static str,
    source: &'static str,
    confidence: &'static str,
    roles: &'static [&'static str],
}

fn role_table(program_id: &str, account_count: usize) -> Option<RoleTable> {
    if matches!(
        program_id,
        RAYDIUM_CPMM_PROGRAM_ID | RAYDIUM_CPMM_LEGACY_PROGRAM_ID
    ) {
        return Some(RoleTable {
            instruction_name: "cpmm_or_swap",
            source: "Raydium CPMM IDL account layout",
            confidence: "medium",
            roles: prefix_roles(
                account_count,
                &[
                    "payer",
                    "authority",
                    "amm_config",
                    "pool_state",
                    "input_token_account",
                    "output_token_account",
                    "input_vault",
                    "output_vault",
                    "input_token_program",
                    "output_token_program",
                    "input_mint",
                    "output_mint",
                    "observation_state",
                ],
            ),
        });
    }

    if program_id == RAYDIUM_CLMM_PROGRAM_ID {
        return Some(RoleTable {
            instruction_name: "clmm_or_swap",
            source: "Raydium CLMM IDL account layout",
            confidence: "medium",
            roles: prefix_roles(
                account_count,
                &[
                    "payer",
                    "amm_config",
                    "pool_state",
                    "input_token_account",
                    "output_token_account",
                    "input_vault",
                    "output_vault",
                    "observation_state",
                    "token_program",
                    "tick_array_0",
                    "tick_array_1",
                    "tick_array_2",
                    "memo_program",
                ],
            ),
        });
    }

    if program_id == RAYDIUM_LAUNCHLAB_PROGRAM_ID {
        return Some(RoleTable {
            instruction_name: "launchlab_flow",
            source: "Raydium LaunchLab IDL account layout",
            confidence: "medium",
            roles: prefix_roles(
                account_count,
                &[
                    "payer",
                    "creator",
                    "platform_config",
                    "pool_state",
                    "base_mint",
                    "quote_mint",
                    "user_base_token",
                    "user_quote_token",
                    "base_vault",
                    "quote_vault",
                    "token_program",
                    "associated_token_program",
                    "system_program",
                ],
            ),
        });
    }

    if matches!(
        program_id,
        RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID
    ) {
        return Some(RoleTable {
            instruction_name: "amm_v4_or_swap",
            source: "Raydium AMM v4 static account layout",
            confidence: "medium",
            roles: prefix_roles(
                account_count,
                &[
                    "token_program",
                    "amm",
                    "amm_authority",
                    "amm_open_orders",
                    "amm_target_orders",
                    "pool_coin_vault",
                    "pool_pc_vault",
                    "serum_program",
                    "serum_market",
                    "serum_bids",
                    "serum_asks",
                    "serum_event_queue",
                    "serum_coin_vault",
                    "serum_pc_vault",
                    "serum_vault_signer",
                    "user_source",
                    "user_destination",
                    "user_owner",
                ],
            ),
        });
    }

    None
}

fn prefix_roles(account_count: usize, known: &'static [&'static str]) -> &'static [&'static str] {
    &known[..known.len().min(account_count)]
}

fn is_raydium_program(program_id: &str) -> bool {
    matches!(
        program_id,
        RAYDIUM_CLMM_PROGRAM_ID
            | RAYDIUM_CPMM_PROGRAM_ID
            | RAYDIUM_CPMM_LEGACY_PROGRAM_ID
            | RAYDIUM_LAUNCHLAB_PROGRAM_ID
            | RAYDIUM_AMM_V4_PROGRAM_ID
            | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID
    )
}

fn token_movements(
    account_keys: &[String],
    pre: &[UiTransactionTokenBalance],
    post: &[UiTransactionTokenBalance],
) -> Vec<TokenMovement> {
    let mut balances: BTreeMap<(u8, String), TokenBalancePair<'_>> = BTreeMap::new();
    for balance in pre {
        balances
            .entry((balance.account_index, balance.mint.clone()))
            .or_default()
            .0 = Some(balance);
    }
    for balance in post {
        balances
            .entry((balance.account_index, balance.mint.clone()))
            .or_default()
            .1 = Some(balance);
    }

    balances
        .into_iter()
        .map(|((account_index, mint), (pre, post))| {
            let pre_amount = pre
                .map(|balance| balance.ui_token_amount.amount.clone())
                .unwrap_or_else(|| "0".to_string());
            let post_amount = post
                .map(|balance| balance.ui_token_amount.amount.clone())
                .unwrap_or_else(|| "0".to_string());
            let decimals = post
                .or(pre)
                .map(|balance| balance.ui_token_amount.decimals)
                .unwrap_or_default();
            TokenMovement {
                account_index: account_index as usize,
                account: account_keys.get(account_index as usize).cloned(),
                mint,
                owner: post.or(pre).and_then(|balance| opt_string(&balance.owner)),
                program_id: post
                    .or(pre)
                    .and_then(|balance| opt_string(&balance.program_id)),
                pre_amount_raw: pre_amount.clone(),
                post_amount_raw: post_amount.clone(),
                delta_raw: signed_delta(&pre_amount, &post_amount),
                decimals,
                ui_pre_amount: pre
                    .map(|balance| balance.ui_token_amount.ui_amount_string.clone())
                    .unwrap_or_else(|| "0".to_string()),
                ui_post_amount: post
                    .map(|balance| balance.ui_token_amount.ui_amount_string.clone())
                    .unwrap_or_else(|| "0".to_string()),
            }
        })
        .collect()
}

fn opt_string(value: &OptionSerializer<String>) -> Option<String> {
    match value {
        OptionSerializer::Some(value) => Some(value.clone()),
        OptionSerializer::None | OptionSerializer::Skip => None,
    }
}

fn signed_delta(pre: &str, post: &str) -> String {
    let pre = pre.parse::<i128>().unwrap_or_default();
    let post = post.parse::<i128>().unwrap_or_default();
    (post - pre).to_string()
}

fn swap_summary(
    instructions: &[InstructionDebugInfo],
    movements: &[TokenMovement],
    account_roles: &[RaydiumAccountRole],
    warnings: &mut Vec<String>,
) -> Option<RaydiumSwapSummary> {
    let input = movements
        .iter()
        .filter(|movement| movement.delta_raw.starts_with('-'))
        .min_by_key(|movement| movement.delta_raw.parse::<i128>().unwrap_or_default());
    let output = movements
        .iter()
        .filter(|movement| movement.delta_raw.parse::<i128>().unwrap_or_default() > 0)
        .max_by_key(|movement| movement.delta_raw.parse::<i128>().unwrap_or_default());

    if input.is_none() && output.is_none() && !account_roles.is_empty() {
        warnings.push(
            "Token movement was not available from RPC metadata; swap amounts could not be proven."
                .to_string(),
        );
    }

    let transfer_fee_notes = movements
        .iter()
        .filter(|movement| movement.program_id.as_deref() == Some(TOKEN_2022_PROGRAM_ID))
        .map(|movement| {
            format!(
                "Token-2022 mint {} may apply transfer fees or hooks; compare spendable amount rather than displayed balance.",
                movement.mint
            )
        })
        .collect::<Vec<_>>();

    let limits = decoded_swap_limits(instructions);
    let output_amount = output.map(|movement| movement.delta_raw.clone());
    let slippage_result = slippage_result(
        output_amount.as_deref(),
        limits.min_output_raw.as_deref(),
        input.is_some() && output.is_some(),
    );

    Some(RaydiumSwapSummary {
        route_kind: "single_transaction".to_string(),
        input_mint: input.map(|movement| movement.mint.clone()),
        output_mint: output.map(|movement| movement.mint.clone()),
        input_amount_raw: input
            .map(|movement| movement.delta_raw.trim_start_matches('-').to_string())
            .or(limits.input_amount_raw),
        output_amount_raw: output_amount,
        min_output_raw: limits.min_output_raw,
        max_input_raw: limits.max_input_raw,
        slippage_result: Some(slippage_result),
        transfer_fee_notes,
        route_leg_status: account_roles
            .iter()
            .filter(|role| role.role.contains("vault") || role.role.contains("token"))
            .map(|role| {
                format!(
                    "Instruction #{} role {} mapped to {} with {} confidence.",
                    role.instruction_index, role.role, role.pubkey, role.confidence
                )
            })
            .collect(),
    })
}

#[derive(Default)]
struct DecodedSwapLimits {
    input_amount_raw: Option<String>,
    min_output_raw: Option<String>,
    max_input_raw: Option<String>,
}

fn decoded_swap_limits(instructions: &[InstructionDebugInfo]) -> DecodedSwapLimits {
    instructions
        .iter()
        .find_map(|instruction| {
            if !is_raydium_program(&instruction.program_id) {
                return None;
            }
            let data = bs58::decode(&instruction.data_base58).into_vec().ok()?;
            if matches!(
                instruction.program_id.as_str(),
                RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID
            ) {
                return decode_two_u64(data.get(1..17)?);
            }
            decode_two_u64(data.get(8..24)?)
        })
        .unwrap_or_default()
}

fn decode_two_u64(data: &[u8]) -> Option<DecodedSwapLimits> {
    if data.len() < 16 {
        return None;
    }
    let amount = u64::from_le_bytes(data[0..8].try_into().ok()?);
    let threshold = u64::from_le_bytes(data[8..16].try_into().ok()?);
    Some(DecodedSwapLimits {
        input_amount_raw: (amount > 0).then(|| amount.to_string()),
        min_output_raw: (threshold > 0).then(|| threshold.to_string()),
        max_input_raw: None,
    })
}

fn slippage_result(
    output_amount_raw: Option<&str>,
    min_output_raw: Option<&str>,
    has_movement: bool,
) -> String {
    match (
        output_amount_raw.and_then(|value| value.parse::<u128>().ok()),
        min_output_raw.and_then(|value| value.parse::<u128>().ok()),
    ) {
        (Some(output), Some(minimum)) if output < minimum => {
            "Output was below minimum; the quote moved before execution or the transaction used stale pool state.".to_string()
        }
        (Some(output), Some(minimum)) => format!(
            "Output satisfied the decoded minimum output check ({output} >= {minimum})."
        ),
        _ if has_movement => {
            "Token balance movement was observed; decoded min/max slippage limits were not available for this instruction layout.".to_string()
        }
        _ => {
            "Slippage comparison unavailable because token movement or decoded limits were missing."
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::types::InstructionAccountMeta;

    #[test]
    fn role_labeling_handles_cpmm_layout() {
        let ix = InstructionDebugInfo {
            index: 0,
            program_id: RAYDIUM_CPMM_PROGRAM_ID.to_string(),
            program_label: "Raydium CPMM".to_string(),
            account_indexes: vec![0, 1, 2, 3],
            accounts: (0..4)
                .map(|index| InstructionAccountMeta {
                    index,
                    pubkey: format!("account_{index}"),
                    signer: index == 0,
                    writable: true,
                    owner: None,
                    owner_label: None,
                    raydium_role: None,
                    raydium_role_confidence: None,
                })
                .collect(),
            data_base58: String::new(),
            discriminator: None,
            error: None,
        };
        let product = RaydiumProductDebug {
            product: crate::debug::types::RaydiumProduct::Cpmm,
            phase: None,
            matched_program_ids: vec![RAYDIUM_CPMM_PROGRAM_ID.to_string()],
            evidence: Vec::new(),
        };
        let context = build_raydium_context(Some(&product), &[ix], &[], &[], &[]).unwrap();
        assert_eq!(context.account_roles[3].role, "pool_state");
    }
}
