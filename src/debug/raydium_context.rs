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
    DecodedInstruction, InstructionDebugInfo, RaydiumAccountRole, RaydiumContext,
    RaydiumInstructionRole, RaydiumProductDebug, RaydiumSwapSummary, TokenMovement,
};

type TokenBalancePair<'a> = (
    Option<&'a UiTransactionTokenBalance>,
    Option<&'a UiTransactionTokenBalance>,
);

/// Builds Raydium-specific account roles and token movement diagnostics.
pub(crate) fn build_raydium_context(
    product: Option<&RaydiumProductDebug>,
    instructions: &[InstructionDebugInfo],
    decoded_instructions: &[DecodedInstruction],
    account_keys: &[String],
    pre_token_balances: &[UiTransactionTokenBalance],
    post_token_balances: &[UiTransactionTokenBalance],
) -> Option<RaydiumContext> {
    let product = product?;
    let mut warnings = Vec::new();
    let movements = token_movements(account_keys, pre_token_balances, post_token_balances);
    let mut instruction_roles = Vec::new();
    let mut account_roles = Vec::new();

    let raydium_decoded = decoded_instructions
        .iter()
        .filter(|instruction| is_raydium_program(&instruction.program_id))
        .cloned()
        .collect::<Vec<_>>();

    for instruction in &raydium_decoded {
        if let Some(semantic) = &instruction.semantic_decode {
            instruction_roles.push(RaydiumInstructionRole {
                instruction_index: instruction.outer_instruction_index,
                program_id: instruction.program_id.clone(),
                instruction_name: semantic.instruction_name.clone(),
                role_source: semantic.source.clone(),
                confidence: semantic.confidence.clone(),
            });
            for role in &semantic.accounts {
                if role.confidence != "high" {
                    continue;
                }
                let account_index = role.account_index.unwrap_or_default();
                let movement = movements
                    .iter()
                    .find(|movement| movement.account_index == account_index);
                account_roles.push(RaydiumAccountRole {
                    instruction_index: instruction.outer_instruction_index,
                    account_index,
                    pubkey: role.pubkey.clone(),
                    role: role.role.clone(),
                    mint: movement.map(|movement| movement.mint.clone()),
                    owner: movement.and_then(|m| m.owner.clone()),
                    writable: false,
                    signer: false,
                    source: role.source.clone(),
                    confidence: role.confidence.clone(),
                });
            }
        } else {
            warnings.push(format!(
                "Raydium instruction {} observed in {} but exact discriminator/account layout decoding is not loaded yet.",
                instruction.id, instruction.program_label
            ));
        }
    }

    for instruction in instructions {
        if is_raydium_program(&instruction.program_id)
            && !raydium_decoded
                .iter()
                .any(|decoded| decoded.outer_instruction_index == instruction.index)
        {
            warnings.push(format!(
                "Raydium instruction #{} was observed, but semantic account roles were not proven.",
                instruction.index
            ));
        }
    }

    let swap_summary = swap_summary(&raydium_decoded, &movements, &account_roles, &mut warnings);
    let has_context =
        !instruction_roles.is_empty() || !movements.is_empty() || !warnings.is_empty();
    has_context.then(|| RaydiumContext {
        product: Some(product.product.clone()),
        phase: product.phase.clone(),
        decoded_instructions: raydium_decoded,
        instruction_roles,
        account_roles,
        token_movements: movements,
        swap_summary,
        warnings,
    })
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
    decoded_instructions: &[DecodedInstruction],
    movements: &[TokenMovement],
    account_roles: &[RaydiumAccountRole],
    warnings: &mut Vec<String>,
) -> Option<RaydiumSwapSummary> {
    let swap_instruction = decoded_instructions.iter().find(|instruction| {
        instruction
            .semantic_decode
            .as_ref()
            .is_some_and(|semantic| is_swap_like(&semantic.instruction_name))
    });
    let roles_are_proven = swap_instruction.is_some()
        && account_roles
            .iter()
            .any(|role| role.confidence == "high" && role.role.contains("token"));
    let instruction_name = swap_instruction
        .and_then(|instruction| instruction.semantic_decode.as_ref())
        .map(|semantic| semantic.instruction_name.as_str())
        .unwrap_or_default();
    let input = movement_for_roles(
        movements,
        account_roles,
        &input_role_candidates(instruction_name),
        |delta| delta < 0,
    );
    let output = movement_for_roles(
        movements,
        account_roles,
        &output_role_candidates(instruction_name),
        |delta| delta > 0,
    );

    if input.is_none() && output.is_none() && !account_roles.is_empty() {
        warnings.push(
            "Token movement was not available from RPC metadata; swap amounts could not be proven."
                .to_string(),
        );
    }
    if !roles_are_proven && !movements.is_empty() {
        warnings.push(
            "Token movements were observed, but user/vault swap legs were not inferred because exact Raydium account roles were not proven."
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

    let limits = decoded_swap_limits(decoded_instructions);
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

fn decoded_swap_limits(instructions: &[DecodedInstruction]) -> DecodedSwapLimits {
    let mut limits = DecodedSwapLimits::default();
    for semantic in instructions
        .iter()
        .filter_map(|instruction| instruction.semantic_decode.as_ref())
        .filter(|semantic| {
            semantic.confidence == "high" && is_swap_like(&semantic.instruction_name)
        })
    {
        let mut amount = None;
        let mut other_threshold = None;
        let mut is_base_input = None;
        for arg in &semantic.arguments {
            match arg.name.as_str() {
                "amount_in" | "input_amount" => limits.input_amount_raw = Some(arg.value.clone()),
                "minimum_amount_out" | "min_amount_out" | "min_out" => {
                    limits.min_output_raw = Some(arg.value.clone())
                }
                "maximum_amount_in" | "max_amount_in" | "max_in" => {
                    limits.max_input_raw = Some(arg.value.clone())
                }
                "amount" => amount = Some(arg.value.clone()),
                "other_amount_threshold" => other_threshold = Some(arg.value.clone()),
                "is_base_input" => is_base_input = Some(arg.value == "true"),
                _ => {}
            }
        }
        match is_base_input {
            Some(true) => {
                limits.input_amount_raw = limits.input_amount_raw.or(amount);
                limits.min_output_raw = limits.min_output_raw.or(other_threshold);
            }
            Some(false) => {
                limits.max_input_raw = limits.max_input_raw.or(other_threshold);
            }
            None => {}
        }
        if limits.input_amount_raw.is_some()
            || limits.min_output_raw.is_some()
            || limits.max_input_raw.is_some()
        {
            return limits;
        }
    }
    limits
}

fn is_swap_like(name: &str) -> bool {
    name.contains("swap") || name.contains("buy") || name.contains("sell")
}

fn movement_for_roles<'a>(
    movements: &'a [TokenMovement],
    account_roles: &[RaydiumAccountRole],
    role_candidates: &[&str],
    delta_predicate: impl Fn(i128) -> bool,
) -> Option<&'a TokenMovement> {
    role_candidates.iter().find_map(|candidate| {
        account_roles
            .iter()
            .filter(|role| role.confidence == "high" && role.role == *candidate)
            .find_map(|role| {
                movements.iter().find(|movement| {
                    movement.account_index == role.account_index
                        && movement
                            .delta_raw
                            .parse::<i128>()
                            .is_ok_and(&delta_predicate)
                })
            })
    })
}

fn input_role_candidates(instruction_name: &str) -> Vec<&'static str> {
    if instruction_name.contains("buy") {
        return vec!["user_quote_token", "input_token_account"];
    }
    if instruction_name.contains("sell") {
        return vec!["user_base_token", "input_token_account"];
    }
    vec![
        "input_token_account",
        "user_input_token",
        "source_token_account",
        "user_source_token",
        "user_source_token_account",
    ]
}

fn output_role_candidates(instruction_name: &str) -> Vec<&'static str> {
    if instruction_name.contains("buy") {
        return vec!["user_base_token", "output_token_account"];
    }
    if instruction_name.contains("sell") {
        return vec!["user_quote_token", "output_token_account"];
    }
    vec![
        "output_token_account",
        "user_output_token",
        "destination_token_account",
        "user_destination_token",
        "user_destination_token_account",
    ]
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
            "Observed: output was below the decoded minimum output threshold. Possible causes include market/pool state changing after the quote, an already-stale quote, or another execution difference affecting realized output.".to_string()
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
    use crate::debug::types::{DecodedArgument, InstructionAccountMeta, InstructionSemanticDecode};

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
        let context = build_raydium_context(Some(&product), &[ix], &[], &[], &[], &[]).unwrap();
        assert!(context.account_roles.is_empty());
        assert!(context
            .warnings
            .iter()
            .any(|warning| warning.contains("semantic account roles were not proven")));
    }

    #[test]
    fn swap_summary_uses_decoded_roles_not_largest_token_deltas() {
        let decoded = vec![DecodedInstruction {
            id: "outer_0".to_string(),
            outer_instruction_index: 0,
            inner_instruction_index: None,
            invocation_kind: "outer".to_string(),
            program_id: RAYDIUM_CPMM_PROGRAM_ID.to_string(),
            program_label: "Raydium CPMM".to_string(),
            accounts: Vec::new(),
            account_indexes: Vec::new(),
            raw_data_base58: String::new(),
            discriminator: None,
            stack_height: None,
            semantic_decode: Some(InstructionSemanticDecode {
                protocol: "raydium_cpmm".to_string(),
                instruction_name: "swap_base_input".to_string(),
                source: "test".to_string(),
                confidence: "high".to_string(),
                arguments: vec![
                    DecodedArgument {
                        name: "amount_in".to_string(),
                        value: "100".to_string(),
                    },
                    DecodedArgument {
                        name: "minimum_amount_out".to_string(),
                        value: "95".to_string(),
                    },
                ],
                accounts: Vec::new(),
                remaining_accounts: Vec::new(),
            }),
        }];
        let roles = vec![
            RaydiumAccountRole {
                instruction_index: 0,
                account_index: 4,
                pubkey: "user_in".to_string(),
                role: "input_token_account".to_string(),
                mint: Some("input_mint".to_string()),
                owner: None,
                writable: true,
                signer: false,
                source: "test".to_string(),
                confidence: "high".to_string(),
            },
            RaydiumAccountRole {
                instruction_index: 0,
                account_index: 5,
                pubkey: "user_out".to_string(),
                role: "output_token_account".to_string(),
                mint: Some("output_mint".to_string()),
                owner: None,
                writable: true,
                signer: false,
                source: "test".to_string(),
                confidence: "high".to_string(),
            },
        ];
        let movements = vec![
            TokenMovement {
                account_index: 99,
                account: Some("unrelated".to_string()),
                mint: "unrelated_mint".to_string(),
                owner: None,
                program_id: None,
                pre_amount_raw: "10000".to_string(),
                post_amount_raw: "1".to_string(),
                delta_raw: "-9999".to_string(),
                decimals: 6,
                ui_pre_amount: "10000".to_string(),
                ui_post_amount: "1".to_string(),
            },
            TokenMovement {
                account_index: 4,
                account: Some("user_in".to_string()),
                mint: "input_mint".to_string(),
                owner: None,
                program_id: None,
                pre_amount_raw: "100".to_string(),
                post_amount_raw: "0".to_string(),
                delta_raw: "-100".to_string(),
                decimals: 6,
                ui_pre_amount: "100".to_string(),
                ui_post_amount: "0".to_string(),
            },
            TokenMovement {
                account_index: 5,
                account: Some("user_out".to_string()),
                mint: "output_mint".to_string(),
                owner: None,
                program_id: None,
                pre_amount_raw: "0".to_string(),
                post_amount_raw: "98".to_string(),
                delta_raw: "98".to_string(),
                decimals: 6,
                ui_pre_amount: "0".to_string(),
                ui_post_amount: "98".to_string(),
            },
        ];
        let mut warnings = Vec::new();

        let summary = swap_summary(&decoded, &movements, &roles, &mut warnings).unwrap();

        assert_eq!(summary.input_mint.as_deref(), Some("input_mint"));
        assert_eq!(summary.output_mint.as_deref(), Some("output_mint"));
        assert_eq!(summary.input_amount_raw.as_deref(), Some("100"));
        assert_eq!(summary.output_amount_raw.as_deref(), Some("98"));
        assert_eq!(summary.min_output_raw.as_deref(), Some("95"));
    }

    #[test]
    fn clmm_exact_output_sets_max_input_threshold() {
        let decoded = vec![DecodedInstruction {
            semantic_decode: Some(InstructionSemanticDecode {
                protocol: "raydium_clmm".to_string(),
                instruction_name: "swap".to_string(),
                source: "test".to_string(),
                confidence: "high".to_string(),
                arguments: vec![
                    DecodedArgument {
                        name: "amount".to_string(),
                        value: "50".to_string(),
                    },
                    DecodedArgument {
                        name: "other_amount_threshold".to_string(),
                        value: "60".to_string(),
                    },
                    DecodedArgument {
                        name: "is_base_input".to_string(),
                        value: "false".to_string(),
                    },
                ],
                accounts: Vec::new(),
                remaining_accounts: Vec::new(),
            }),
            ..DecodedInstruction::default()
        }];

        let limits = decoded_swap_limits(&decoded);

        assert_eq!(limits.input_amount_raw, None);
        assert_eq!(limits.min_output_raw, None);
        assert_eq!(limits.max_input_raw.as_deref(), Some("60"));
    }
}
