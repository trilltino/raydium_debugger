use serde_json::Value;

use super::{InstructionAccountEntry, InstructionArgEntry, InstructionEntry};

pub(super) fn parse_instruction_entry(value: &Value) -> Option<InstructionEntry> {
    let discriminator = value
        .get("discriminator")?
        .as_array()?
        .iter()
        .map(|byte| Some(byte.as_u64()? as u8))
        .collect::<Option<Vec<_>>>()?;
    Some(InstructionEntry {
        name: value.get("name")?.as_str()?.to_string(),
        discriminator: bytes_to_hex(&discriminator),
        accounts: value
            .get("accounts")
            .and_then(Value::as_array)
            .map(|accounts| {
                accounts
                    .iter()
                    .filter_map(|account| {
                        Some(InstructionAccountEntry {
                            name: account.get("name")?.as_str()?.to_string(),
                            writable: account
                                .get("writable")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            signer: account
                                .get("signer")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        args: value
            .get("args")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .filter_map(|arg| {
                        Some(InstructionArgEntry {
                            name: arg.get("name")?.as_str()?.to_string(),
                            kind: idl_type_to_string(arg.get("type")?),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn idl_type_to_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_idl_discriminator_accounts_and_argument_types() {
        let entry = parse_instruction_entry(&json!({
            "name": "swap",
            "discriminator": [0, 1, 15, 16, 127, 128, 254, 255],
            "accounts": [
                { "name": "payer", "writable": true, "signer": true },
                { "name": "config" }
            ],
            "args": [
                { "name": "amount", "type": "u64" },
                { "name": "limit", "type": { "option": "u128" } }
            ]
        }))
        .unwrap();

        assert_eq!(entry.name, "swap");
        assert_eq!(entry.discriminator, "00010f107f80feff");
        assert!(entry.accounts[0].writable);
        assert!(entry.accounts[0].signer);
        assert!(!entry.accounts[1].writable);
        assert!(!entry.accounts[1].signer);
        assert_eq!(entry.args[0].kind, "u64");
        assert_eq!(entry.args[1].kind, r#"{"option":"u128"}"#);
    }

    #[test]
    fn rejects_instructions_missing_required_fields() {
        assert!(parse_instruction_entry(&json!({ "name": "swap" })).is_none());
        assert!(
            parse_instruction_entry(&json!({ "discriminator": [0, 0, 0, 0, 0, 0, 0, 0] }))
                .is_none()
        );
        assert!(parse_instruction_entry(&json!({
            "name": "swap", "discriminator": ["invalid"]
        }))
        .is_none());
    }
}
