use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};

pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

pub(crate) fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    let json = serde_json::to_string_pretty(value)? + "\n";
    fs::write(path, json).with_context(|| format!("failed to write {}", path.display()))
}
