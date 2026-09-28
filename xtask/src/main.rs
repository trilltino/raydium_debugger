//! Developer automation tasks for Raydium registry generation and drift checks.

use anyhow::{anyhow, Context};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{env, fs, path::Path};

const OUTPUT: &str = "src/failures/raydium_registry.generated.json";
const INSTRUCTION_OUTPUT: &str = "src/debug/raydium_instructions.generated.json";

const SOURCES: &[Source] = &[
    Source {
        product: "raydium_clmm",
        url: "https://raw.githubusercontent.com/raydium-io/raydium-idl/master/raydium_clmm/raydium_clmm.json",
        kind: SourceKind::Idl,
    },
    Source {
        product: "raydium_cpmm",
        url: "https://raw.githubusercontent.com/raydium-io/raydium-idl/master/raydium_cpmm/raydium_cp_swap.json",
        kind: SourceKind::Idl,
    },
    Source {
        product: "raydium_launchpad",
        url: "https://raw.githubusercontent.com/raydium-io/raydium-idl/master/raydium_launchpad/raydium_launchpad.json",
        kind: SourceKind::Idl,
    },
    Source {
        product: "raydium_amm_v4",
        url: "https://raw.githubusercontent.com/raydium-io/raydium-amm/master/program/src/error.rs",
        kind: SourceKind::RustEnum,
    },
];

#[derive(Clone, Copy)]
struct Source {
    product: &'static str,
    url: &'static str,
    kind: SourceKind,
}

#[derive(Clone, Copy)]
enum SourceKind {
    Idl,
    RustEnum,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Snapshot {
    generated_at: String,
    sources: Vec<SourceSnapshot>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct SourceSnapshot {
    product: String,
    url: String,
    error_count: usize,
    errors: Vec<ErrorEntry>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ErrorEntry {
    code: u32,
    name: String,
    message: String,
}

fn main() -> anyhow::Result<()> {
    let args = env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("raydium-registry") => match args.get(2).map(String::as_str) {
            Some("generate") => generate(),
            Some("validate") | Some("check") => validate(),
            Some("drift") => drift(),
            other => Err(anyhow!(
                "unknown raydium-registry command {:?}; use generate, validate, or drift",
                other
            )),
        },
        _ => Err(anyhow!(
            "usage: cargo run -p xtask -- raydium-registry <generate|validate|drift>"
        )),
    }
}

fn generate() -> anyhow::Result<()> {
    let snapshot = fetch_snapshot()?;
    let json = serde_json::to_string_pretty(&snapshot)? + "\n";
    fs::write(OUTPUT, json).with_context(|| format!("failed to write {OUTPUT}"))?;
    Ok(())
}

fn drift() -> anyhow::Result<()> {
    let expected = normalize_existing(Path::new(OUTPUT))?;
    let actual = normalize_generated(fetch_snapshot()?);
    if expected != actual {
        return Err(anyhow!(
            "{OUTPUT} is stale; run `cargo run -p xtask -- raydium-registry generate`"
        ));
    }
    Ok(())
}

fn validate() -> anyhow::Result<()> {
    let sources = normalize_existing(Path::new(OUTPUT))?;
    let required = [
        "raydium_clmm",
        "raydium_cpmm",
        "raydium_launchpad",
        "raydium_amm_v4",
    ];
    for product in required {
        let Some(source) = sources.iter().find(|source| source.product == product) else {
            return Err(anyhow!("generated registry is missing {product}"));
        };
        if source.error_count != source.errors.len() {
            return Err(anyhow!(
                "{product} error_count {} does not match {} entries",
                source.error_count,
                source.errors.len()
            ));
        }
        let mut codes = std::collections::BTreeSet::new();
        for entry in &source.errors {
            if !codes.insert(entry.code) {
                return Err(anyhow!(
                    "{product} contains duplicate error code {}",
                    entry.code
                ));
            }
            if entry.name.trim().is_empty() {
                return Err(anyhow!("{product} code {} has an empty name", entry.code));
            }
        }
    }
    assert_reference_code(&sources, "raydium_clmm", 6001)?;
    assert_reference_code(&sources, "raydium_cpmm", 6001)?;
    assert_reference_code(&sources, "raydium_launchpad", 6001)?;
    assert_reference_code(&sources, "raydium_amm_v4", 30)?;
    validate_instruction_registry(Path::new(INSTRUCTION_OUTPUT))?;
    Ok(())
}

fn validate_instruction_registry(path: &Path) -> anyhow::Result<()> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let snapshot: Value = serde_json::from_str(&raw)?;
    let sources = snapshot
        .get("sources")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("instruction registry must contain sources[]"))?;
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
            .find(|source| source.get("protocol").and_then(Value::as_str) == Some(protocol))
            .ok_or_else(|| anyhow!("instruction registry is missing {protocol}"))?;
        if source.get("program_id").and_then(Value::as_str) != Some(program_id) {
            return Err(anyhow!("{protocol} has the wrong program id"));
        }
        let instructions = source
            .get("instructions")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("{protocol} is missing instructions[]"))?;
        if !instructions.iter().any(|instruction| {
            instruction.get("name").and_then(Value::as_str) == Some(reference_instruction)
        }) {
            return Err(anyhow!(
                "{protocol} is missing reference instruction {reference_instruction}"
            ));
        }
        let mut discriminators = std::collections::BTreeSet::new();
        for instruction in instructions {
            let name = instruction
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{protocol} instruction has empty name"))?;
            let discriminator = instruction
                .get("discriminator")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{protocol}.{name} has empty discriminator"))?;
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
            let accounts = instruction
                .get("accounts")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("{protocol}.{name} is missing accounts[]"))?;
            for account in accounts {
                if account
                    .get("name")
                    .and_then(Value::as_str)
                    .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(anyhow!("{protocol}.{name} has an account with empty name"));
                }
            }
            let args = instruction
                .get("args")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("{protocol}.{name} is missing args[]"))?;
            for arg in args {
                if arg
                    .get("name")
                    .and_then(Value::as_str)
                    .is_none_or(|value| value.trim().is_empty())
                    || arg
                        .get("type")
                        .and_then(Value::as_str)
                        .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(anyhow!("{protocol}.{name} has an invalid arg"));
                }
            }
        }
    }
    Ok(())
}

fn assert_reference_code(
    sources: &[SourceSnapshot],
    product: &str,
    code: u32,
) -> anyhow::Result<()> {
    let source = sources
        .iter()
        .find(|source| source.product == product)
        .ok_or_else(|| anyhow!("generated registry is missing {product}"))?;
    let entry = source
        .errors
        .iter()
        .find(|entry| entry.code == code)
        .ok_or_else(|| anyhow!("{product} is missing reference code {code}"))?;
    if entry.name.trim().is_empty() {
        return Err(anyhow!("{product} reference code {code} has an empty name"));
    }
    Ok(())
}

fn fetch_snapshot() -> anyhow::Result<Snapshot> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("raydium-debugger-xtask")
        .build()?;
    let mut sources = Vec::new();
    for source in SOURCES {
        let body = client
            .get(source.url)
            .send()
            .with_context(|| format!("failed to fetch {}", source.url))?
            .error_for_status()
            .with_context(|| format!("failed to fetch {}", source.url))?
            .text()?;
        let errors = match source.kind {
            SourceKind::Idl => parse_idl_errors(&body)?,
            SourceKind::RustEnum => parse_rust_enum_errors(&body),
        };
        sources.push(SourceSnapshot {
            product: source.product.to_string(),
            url: source.url.to_string(),
            error_count: errors.len(),
            errors,
        });
    }
    Ok(Snapshot {
        generated_at: Utc::now().to_rfc3339(),
        sources,
    })
}

fn parse_idl_errors(body: &str) -> anyhow::Result<Vec<ErrorEntry>> {
    let value: Value = serde_json::from_str(body)?;
    let Some(errors) = value.get("errors").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    Ok(errors
        .iter()
        .filter_map(|entry| {
            Some(ErrorEntry {
                code: entry.get("code")?.as_u64()? as u32,
                name: entry.get("name")?.as_str()?.to_string(),
                message: entry
                    .get("msg")
                    .or_else(|| entry.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect())
}

fn parse_rust_enum_errors(body: &str) -> Vec<ErrorEntry> {
    let mut entries = Vec::new();
    let mut next_code = 0u32;
    let mut pending_msg = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(msg) = trimmed
            .strip_prefix("#[error(\"")
            .and_then(|tail| tail.strip_suffix("\")]"))
        {
            pending_msg = msg.to_string();
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with("#[")
            || trimmed.starts_with("pub enum")
            || trimmed.starts_with("}")
            || trimmed.starts_with("//")
        {
            continue;
        }
        let name = trimmed
            .trim_end_matches(',')
            .split('=')
            .next()
            .unwrap_or_default()
            .trim();
        if name.chars().next().is_some_and(char::is_uppercase) {
            entries.push(ErrorEntry {
                code: next_code,
                name: name.to_string(),
                message: pending_msg.clone(),
            });
            next_code += 1;
            pending_msg.clear();
        }
    }
    entries
}

fn normalize_existing(path: &Path) -> anyhow::Result<Vec<SourceSnapshot>> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let snapshot: Snapshot = serde_json::from_str(&raw)?;
    Ok(snapshot.sources)
}

fn normalize_generated(snapshot: Snapshot) -> Vec<SourceSnapshot> {
    snapshot.sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_idl_errors() {
        let errors = parse_idl_errors(
            r#"{"errors":[{"code":6001,"name":"TooLittleOutput","msg":"slippage"}]}"#,
        )
        .unwrap();
        assert_eq!(errors[0].code, 6001);
        assert_eq!(errors[0].name, "TooLittleOutput");
    }

    #[test]
    fn parses_rust_enum_ordinals() {
        let errors = parse_rust_enum_errors(
            r#"
            pub enum AmmError {
              #[error("first")]
              First,
              #[error("second")]
              Second,
            }
            "#,
        );
        assert_eq!(errors[1].code, 1);
        assert_eq!(errors[1].name, "Second");
    }
}
