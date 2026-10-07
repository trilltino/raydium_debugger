use std::{collections::BTreeSet, path::Path};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{http, snapshot, sources::INSTRUCTION_SOURCES};

mod parsing;
use parsing::parse_instruction_entry;

const OUTPUT: &str = "src/debug/raydium_instructions.generated.json";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct InstructionSnapshot {
    generated_from: String,
    sources: Vec<InstructionSourceSnapshot>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct InstructionSourceSnapshot {
    protocol: String,
    program_id: String,
    source: String,
    instructions: Vec<InstructionEntry>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct InstructionEntry {
    name: String,
    discriminator: String,
    accounts: Vec<InstructionAccountEntry>,
    args: Vec<InstructionArgEntry>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct InstructionAccountEntry {
    name: String,
    writable: bool,
    signer: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct InstructionArgEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

pub(crate) fn validate() -> anyhow::Result<()> {
    validate_snapshot(&snapshot::read::<InstructionSnapshot>(Path::new(OUTPUT))?)
}

pub(crate) fn generate() -> anyhow::Result<()> {
    snapshot::write(Path::new(OUTPUT), &fetch_snapshot()?)
}

pub(crate) fn drift() -> anyhow::Result<()> {
    let expected = snapshot::read::<InstructionSnapshot>(Path::new(OUTPUT))?;
    let actual = fetch_snapshot()?;
    if expected != actual {
        return Err(anyhow!(
            "{OUTPUT} is stale; run `cargo run -p xtask -- raydium-instructions generate`"
        ));
    }
    Ok(())
}

fn validate_snapshot(snapshot: &InstructionSnapshot) -> anyhow::Result<()> {
    let sources = &snapshot.sources;
    let required = [
        (
            "raydium_cpmm",
            "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C",
            "swap_base_input",
        ),
        (
            "raydium_clmm",
            "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",
            "swap",
        ),
        (
            "raydium_launchlab",
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj",
            "buy_exact_in",
        ),
    ];
    for (protocol, program_id, reference_instruction) in required {
        let source = sources
            .iter()
            .find(|source| source.protocol == protocol)
            .ok_or_else(|| anyhow!("instruction registry is missing {protocol}"))?;
        if source.program_id != program_id {
            return Err(anyhow!("{protocol} has the wrong program id"));
        }
        let instructions = &source.instructions;
        if !instructions
            .iter()
            .any(|instruction| instruction.name == reference_instruction)
        {
            return Err(anyhow!(
                "{protocol} is missing reference instruction {reference_instruction}"
            ));
        }
        let mut discriminators = BTreeSet::new();
        for instruction in instructions {
            let name = instruction.name.as_str();
            if name.trim().is_empty() {
                return Err(anyhow!("{protocol} instruction has empty name"));
            }
            let discriminator = instruction.discriminator.as_str();
            if discriminator.len() != 16 || !discriminator.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(anyhow!(
                    "{protocol}.{name} discriminator is not 8 bytes of hex"
                ));
            }
            if !discriminators.insert(discriminator) {
                return Err(anyhow!(
                    "{protocol} contains duplicate discriminator {discriminator}"
                ));
            }
            for account in &instruction.accounts {
                if account.name.trim().is_empty() {
                    return Err(anyhow!("{protocol}.{name} has an account with empty name"));
                }
            }
            for arg in &instruction.args {
                if arg.name.trim().is_empty() || arg.kind.trim().is_empty() {
                    return Err(anyhow!("{protocol}.{name} has an invalid arg"));
                }
            }
        }
    }
    Ok(())
}

fn fetch_snapshot() -> anyhow::Result<InstructionSnapshot> {
    let client = http::client()?;
    let mut sources = Vec::new();
    for source in INSTRUCTION_SOURCES {
        let body = http::fetch(&client, source.url)?;
        let value: Value = serde_json::from_str(&body)?;
        let program_id = value
            .get("address")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{} is missing address", source.protocol))?
            .to_string();
        let instructions = value
            .get("instructions")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("{} is missing instructions[]", source.protocol))?
            .iter()
            .filter_map(parse_instruction_entry)
            .collect::<Vec<_>>();
        sources.push(InstructionSourceSnapshot {
            protocol: source.protocol.to_string(),
            program_id,
            source: source.source.to_string(),
            instructions,
        });
    }
    Ok(InstructionSnapshot {
        generated_from: "raydium-idl".to_string(),
        sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_in_snapshot() -> InstructionSnapshot {
        serde_json::from_str(include_str!(
            "../../../src/debug/raydium_instructions.generated.json"
        ))
        .unwrap()
    }

    #[test]
    fn checked_in_registry_retains_its_schema_and_validates() {
        let snapshot = checked_in_snapshot();
        validate_snapshot(&snapshot).unwrap();
        let serialized = serde_json::to_value(&snapshot).unwrap();
        let original: Value = serde_json::from_str(include_str!(
            "../../../src/debug/raydium_instructions.generated.json"
        ))
        .unwrap();
        assert_eq!(serialized, original);
    }

    #[test]
    fn validation_rejects_wrong_program_ids_and_duplicate_discriminators() {
        let mut snapshot = checked_in_snapshot();
        snapshot.sources[0].program_id = "incorrect".into();
        assert!(validate_snapshot(&snapshot)
            .unwrap_err()
            .to_string()
            .contains("wrong program id"));

        let mut snapshot = checked_in_snapshot();
        snapshot.sources[0].instructions[1].discriminator =
            snapshot.sources[0].instructions[0].discriminator.clone();
        assert!(validate_snapshot(&snapshot)
            .unwrap_err()
            .to_string()
            .contains("duplicate discriminator"));
    }
}
