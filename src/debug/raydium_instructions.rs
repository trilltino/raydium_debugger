//! IDL-backed Raydium instruction semantics.
//!
//! CPMM, CLMM, and LaunchLab publish Anchor-style IDLs with stable
//! discriminators. This module turns that generated snapshot into conservative
//! instruction semantics used by both outer and inner instruction records.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::failures::{
    RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID, RAYDIUM_AMM_V4_PROGRAM_ID, RAYDIUM_CLMM_PROGRAM_ID,
    RAYDIUM_CPMM_LEGACY_PROGRAM_ID, RAYDIUM_CPMM_PROGRAM_ID, RAYDIUM_LAUNCHLAB_PROGRAM_ID,
};

use super::types::{DecodedAccountRole, DecodedArgument, InstructionSemanticDecode};

const GENERATED_INSTRUCTIONS: &str = include_str!("raydium_instructions.generated.json");

#[derive(Debug, Deserialize)]
struct InstructionSnapshot {
    sources: Vec<InstructionSource>,
}

#[derive(Debug, Deserialize)]
struct InstructionSource {
    protocol: String,
    program_id: String,
    source: String,
    instructions: Vec<InstructionEntry>,
}

#[derive(Debug, Deserialize)]
struct InstructionEntry {
    name: String,
    discriminator: String,
    accounts: Vec<AccountEntry>,
    args: Vec<ArgEntry>,
}

#[derive(Debug, Deserialize)]
struct AccountEntry {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ArgEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

fn snapshot() -> &'static InstructionSnapshot {
    static SNAPSHOT: OnceLock<InstructionSnapshot> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        serde_json::from_str(GENERATED_INSTRUCTIONS)
            .expect("generated Raydium instruction registry must be valid JSON")
    })
}

pub(crate) fn decode(
    program_id: &str,
    raw_data_base58: &str,
    accounts: &[String],
    account_indexes: &[u8],
) -> Option<InstructionSemanticDecode> {
    let data = bs58::decode(raw_data_base58).into_vec().ok()?;
    if matches!(
        program_id,
        RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID
    ) {
        return decode_amm_v4(&data, accounts, account_indexes);
    }
    if data.len() < 8 {
        return None;
    }
    let discriminator = hex::encode(&data[..8]);
    let (source, entry) = find_entry(program_id, &discriminator)?;
    let mut offset = 8;
    let arguments = entry
        .args
        .iter()
        .map(|arg| decode_arg(arg, &data, &mut offset))
        .collect();
    let decoded_accounts = entry
        .accounts
        .iter()
        .zip(accounts.iter())
        .enumerate()
        .map(|(position, (role, pubkey))| DecodedAccountRole {
            role: role.name.clone(),
            pubkey: pubkey.clone(),
            account_index: account_indexes.get(position).map(|index| *index as usize),
            source: source.source.clone(),
            confidence: "high".to_string(),
        })
        .collect();
    let remaining_accounts = accounts
        .iter()
        .skip(entry.accounts.len())
        .cloned()
        .collect::<Vec<_>>();

    Some(InstructionSemanticDecode {
        protocol: source.protocol.clone(),
        instruction_name: entry.name.clone(),
        source: source.source.clone(),
        confidence: "high".to_string(),
        arguments,
        accounts: decoded_accounts,
        remaining_accounts,
    })
}

pub(crate) fn protocol_for_program(program_id: &str) -> Option<&'static str> {
    match program_id {
        RAYDIUM_CPMM_PROGRAM_ID | RAYDIUM_CPMM_LEGACY_PROGRAM_ID => Some("raydium_cpmm"),
        RAYDIUM_CLMM_PROGRAM_ID => Some("raydium_clmm"),
        RAYDIUM_LAUNCHLAB_PROGRAM_ID => Some("raydium_launchlab"),
        RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID => Some("raydium_amm_v4"),
        _ => None,
    }
}

fn decode_amm_v4(
    data: &[u8],
    accounts: &[String],
    account_indexes: &[u8],
) -> Option<InstructionSemanticDecode> {
    let (&tag, rest) = data.split_first()?;
    let source = "https://github.com/raydium-io/raydium-amm/blob/master/program/src/instruction.rs";
    let (name, arg_specs, account_roles): (&str, &[&str], &[&str]) = match tag {
        1 => (
            "initialize2",
            &[
                "nonce:u8",
                "open_time:u64",
                "init_pc_amount:u64",
                "init_coin_amount:u64",
            ],
            &[
                "token_program",
                "associated_token_program",
                "system_program",
                "rent",
                "amm",
                "amm_authority",
                "amm_open_orders",
                "amm_lp_mint",
                "amm_coin_mint",
                "amm_pc_mint",
                "amm_coin_vault",
                "amm_pc_vault",
                "amm_target_orders",
                "amm_config",
                "create_pool_fee_destination",
                "market_program",
                "market",
                "user_wallet",
                "user_token_coin",
                "user_token_pc",
                "user_token_lp",
            ],
        ),
        9 => (
            "swap_base_in",
            &["amount_in:u64", "minimum_amount_out:u64"],
            AMM_V4_SWAP_ACCOUNTS,
        ),
        11 => (
            "swap_base_out",
            &["max_amount_in:u64", "amount_out:u64"],
            AMM_V4_SWAP_ACCOUNTS,
        ),
        16 => (
            "swap_base_in_v2",
            &["amount_in:u64", "minimum_amount_out:u64"],
            AMM_V4_SWAP_V2_ACCOUNTS,
        ),
        17 => (
            "swap_base_out_v2",
            &["max_amount_in:u64", "amount_out:u64"],
            AMM_V4_SWAP_V2_ACCOUNTS,
        ),
        18 => (
            "withdraw_excess_lamports",
            &[],
            &["collector", "amm_authority", "token_program"],
        ),
        _ => return None,
    };
    let mut offset = 0;
    let mut arguments = vec![DecodedArgument {
        name: "opcode".to_string(),
        value: tag.to_string(),
    }];
    for spec in arg_specs {
        let Some((name, kind)) = spec.split_once(':') else {
            continue;
        };
        arguments.push(DecodedArgument {
            name: name.to_string(),
            value: decode_amm_arg(kind, rest, &mut offset),
        });
    }
    let decoded_accounts = account_roles
        .iter()
        .zip(accounts.iter())
        .enumerate()
        .map(|(position, (role, pubkey))| DecodedAccountRole {
            role: (*role).to_string(),
            pubkey: pubkey.clone(),
            account_index: account_indexes.get(position).map(|index| *index as usize),
            source: source.to_string(),
            confidence: "high".to_string(),
        })
        .collect();
    let remaining_accounts = accounts
        .iter()
        .skip(account_roles.len())
        .cloned()
        .collect::<Vec<_>>();

    Some(InstructionSemanticDecode {
        protocol: "raydium_amm_v4".to_string(),
        instruction_name: name.to_string(),
        source: source.to_string(),
        confidence: "high".to_string(),
        arguments,
        accounts: decoded_accounts,
        remaining_accounts,
    })
}

const AMM_V4_SWAP_ACCOUNTS: &[&str] = &[
    "token_program",
    "amm",
    "amm_authority",
    "amm_open_orders",
    "amm_target_orders",
    "amm_coin_vault",
    "amm_pc_vault",
    "market_program",
    "market",
    "market_bids",
    "market_asks",
    "market_event_queue",
    "market_coin_vault",
    "market_pc_vault",
    "market_vault_signer",
    "user_source_token_account",
    "user_destination_token_account",
    "user_source_owner",
];

const AMM_V4_SWAP_V2_ACCOUNTS: &[&str] = &[
    "token_program",
    "amm",
    "amm_authority",
    "amm_coin_vault",
    "amm_pc_vault",
    "user_source_token_account",
    "user_destination_token_account",
    "user_source_owner",
];

fn decode_amm_arg(kind: &str, data: &[u8], offset: &mut usize) -> String {
    match kind {
        "u8" => read_bytes(data, offset, 1)
            .map(|bytes| bytes[0].to_string())
            .unwrap_or_else(|| "undecoded:truncated_u8".to_string()),
        "u64" => read_array::<8>(data, offset)
            .map(u64::from_le_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "undecoded:truncated_u64".to_string()),
        other => format!("undecoded:{other}"),
    }
}

fn find_entry<'a>(
    program_id: &str,
    discriminator: &str,
) -> Option<(&'a InstructionSource, &'a InstructionEntry)> {
    let protocol = protocol_for_program(program_id)?;
    snapshot()
        .sources
        .iter()
        .find(|source| source.protocol == protocol && source.program_id == program_id)
        .or_else(|| {
            snapshot()
                .sources
                .iter()
                .find(|source| source.protocol == protocol)
        })
        .and_then(|source| {
            source
                .instructions
                .iter()
                .find(|entry| entry.discriminator == discriminator)
                .map(|entry| (source, entry))
        })
}

fn decode_arg(arg: &ArgEntry, data: &[u8], offset: &mut usize) -> DecodedArgument {
    let value = match arg.kind.as_str() {
        "bool" => read_bytes(data, offset, 1)
            .map(|bytes| (bytes[0] != 0).to_string())
            .unwrap_or_else(|| "undecoded:truncated_bool".to_string()),
        "u8" => read_bytes(data, offset, 1)
            .map(|bytes| bytes[0].to_string())
            .unwrap_or_else(|| "undecoded:truncated_u8".to_string()),
        "i8" => read_bytes(data, offset, 1)
            .map(|bytes| (bytes[0] as i8).to_string())
            .unwrap_or_else(|| "undecoded:truncated_i8".to_string()),
        "u16" => read_array::<2>(data, offset)
            .map(u16::from_le_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "undecoded:truncated_u16".to_string()),
        "i16" => read_array::<2>(data, offset)
            .map(i16::from_le_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "undecoded:truncated_i16".to_string()),
        "u32" => read_array::<4>(data, offset)
            .map(u32::from_le_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "undecoded:truncated_u32".to_string()),
        "i32" => read_array::<4>(data, offset)
            .map(i32::from_le_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "undecoded:truncated_i32".to_string()),
        "u64" => read_array::<8>(data, offset)
            .map(u64::from_le_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "undecoded:truncated_u64".to_string()),
        "i64" => read_array::<8>(data, offset)
            .map(i64::from_le_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "undecoded:truncated_i64".to_string()),
        "u128" => read_array::<16>(data, offset)
            .map(u128::from_le_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "undecoded:truncated_u128".to_string()),
        "i128" => read_array::<16>(data, offset)
            .map(i128::from_le_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "undecoded:truncated_i128".to_string()),
        "pubkey" | "publicKey" => read_bytes(data, offset, 32)
            .map(bs58::encode)
            .map(|value| value.into_string())
            .unwrap_or_else(|| "undecoded:truncated_pubkey".to_string()),
        other => format!("undecoded:{other}"),
    };
    DecodedArgument {
        name: arg.name.clone(),
        value,
    }
}

fn read_array<const N: usize>(data: &[u8], offset: &mut usize) -> Option<[u8; N]> {
    read_bytes(data, offset, N).map(|bytes| {
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        out
    })
}

fn read_bytes<'a>(data: &'a [u8], offset: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = offset.checked_add(len)?;
    let bytes = data.get(*offset..end)?;
    *offset = end;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_cpmm_swap_base_input_from_idl_registry() {
        let mut data = hex::decode("8fbe5adac41e33de").unwrap();
        data.extend_from_slice(&123_u64.to_le_bytes());
        data.extend_from_slice(&100_u64.to_le_bytes());
        let accounts = (0..13)
            .map(|index| format!("account_{index}"))
            .collect::<Vec<_>>();
        let indexes = (0_u8..13).collect::<Vec<_>>();

        let decoded = decode(
            RAYDIUM_CPMM_PROGRAM_ID,
            &bs58::encode(data).into_string(),
            &accounts,
            &indexes,
        )
        .expect("CPMM swap discriminator should decode");

        assert_eq!(decoded.protocol, "raydium_cpmm");
        assert_eq!(decoded.instruction_name, "swap_base_input");
        assert_eq!(decoded.confidence, "high");
        assert_eq!(decoded.arguments[0].name, "amount_in");
        assert_eq!(decoded.arguments[0].value, "123");
        assert_eq!(decoded.arguments[1].name, "minimum_amount_out");
        assert_eq!(decoded.arguments[1].value, "100");
        assert_eq!(decoded.accounts[4].role, "input_token_account");
        assert_eq!(decoded.accounts[5].role, "output_token_account");
    }

    #[test]
    fn preserves_clmm_remaining_accounts() {
        let mut data = hex::decode("f8c69e91e17587c8").unwrap();
        data.extend_from_slice(&500_u64.to_le_bytes());
        data.extend_from_slice(&450_u64.to_le_bytes());
        data.extend_from_slice(&0_u128.to_le_bytes());
        data.push(1);
        let accounts = (0..12)
            .map(|index| format!("account_{index}"))
            .collect::<Vec<_>>();
        let indexes = (0_u8..12).collect::<Vec<_>>();

        let decoded = decode(
            RAYDIUM_CLMM_PROGRAM_ID,
            &bs58::encode(data).into_string(),
            &accounts,
            &indexes,
        )
        .expect("CLMM swap discriminator should decode");

        assert_eq!(decoded.protocol, "raydium_clmm");
        assert_eq!(decoded.instruction_name, "swap");
        assert_eq!(decoded.arguments[3].name, "is_base_input");
        assert_eq!(decoded.arguments[3].value, "true");
        assert_eq!(decoded.remaining_accounts, vec!["account_10", "account_11"]);
    }

    #[test]
    fn decodes_amm_v4_swap_base_in_opcode_and_roles() {
        let mut data = vec![9_u8];
        data.extend_from_slice(&1_000_u64.to_le_bytes());
        data.extend_from_slice(&900_u64.to_le_bytes());
        let accounts = (0..18)
            .map(|index| format!("account_{index}"))
            .collect::<Vec<_>>();
        let indexes = (0_u8..18).collect::<Vec<_>>();

        let decoded = decode(
            RAYDIUM_AMM_V4_PROGRAM_ID,
            &bs58::encode(data).into_string(),
            &accounts,
            &indexes,
        )
        .expect("AMM v4 swap opcode should decode");

        assert_eq!(decoded.protocol, "raydium_amm_v4");
        assert_eq!(decoded.instruction_name, "swap_base_in");
        assert_eq!(decoded.arguments[1].name, "amount_in");
        assert_eq!(decoded.arguments[1].value, "1000");
        assert_eq!(decoded.arguments[2].name, "minimum_amount_out");
        assert_eq!(decoded.arguments[2].value, "900");
        assert_eq!(decoded.accounts[15].role, "user_source_token_account");
        assert_eq!(decoded.accounts[16].role, "user_destination_token_account");
    }
}
