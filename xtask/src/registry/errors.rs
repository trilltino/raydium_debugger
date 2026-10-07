use std::{collections::BTreeSet, path::Path};

use anyhow::anyhow;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::instructions;
use crate::{
    http, snapshot,
    sources::{SourceKind, SOURCES},
};

mod parsing;
use parsing::{parse_idl_errors, parse_rust_enum_errors};

const OUTPUT: &str = "src/failures/raydium_registry.generated.json";

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

pub(crate) fn generate() -> anyhow::Result<()> {
    snapshot::write(Path::new(OUTPUT), &fetch_snapshot()?)
}

pub(crate) fn drift() -> anyhow::Result<()> {
    let expected = snapshot::read::<Snapshot>(Path::new(OUTPUT))?.sources;
    let actual = fetch_snapshot()?.sources;
    if expected != actual {
        return Err(anyhow!(
            "{OUTPUT} is stale; run `cargo run -p xtask -- raydium-registry generate`"
        ));
    }
    Ok(())
}

pub(crate) fn validate() -> anyhow::Result<()> {
    let sources = snapshot::read::<Snapshot>(Path::new(OUTPUT))?.sources;
    validate_sources(&sources)?;
    instructions::validate()
}

fn validate_sources(sources: &[SourceSnapshot]) -> anyhow::Result<()> {
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
        let mut codes = BTreeSet::new();
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
    assert_reference_code(sources, "raydium_clmm", 6001)?;
    assert_reference_code(sources, "raydium_cpmm", 6001)?;
    assert_reference_code(sources, "raydium_launchpad", 6001)?;
    assert_reference_code(sources, "raydium_amm_v4", 30)?;
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
    let client = http::client()?;
    let mut sources = Vec::new();
    for source in SOURCES {
        let body = http::fetch(&client, source.url)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_in_snapshot() -> Snapshot {
        serde_json::from_str(include_str!(
            "../../../src/failures/raydium_registry.generated.json"
        ))
        .unwrap()
    }

    #[test]
    fn checked_in_registry_retains_its_schema_and_validates() {
        let snapshot = checked_in_snapshot();
        validate_sources(&snapshot.sources).unwrap();
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../../../src/failures/raydium_registry.generated.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(&snapshot).unwrap(), original);
    }

    #[test]
    fn validation_rejects_incorrect_counts_and_duplicate_codes() {
        let mut snapshot = checked_in_snapshot();
        snapshot.sources[0].error_count += 1;
        assert!(validate_sources(&snapshot.sources)
            .unwrap_err()
            .to_string()
            .contains("does not match"));

        let mut snapshot = checked_in_snapshot();
        snapshot.sources[0].errors[1].code = snapshot.sources[0].errors[0].code;
        assert!(validate_sources(&snapshot.sources)
            .unwrap_err()
            .to_string()
            .contains("duplicate error code"));
    }
}
