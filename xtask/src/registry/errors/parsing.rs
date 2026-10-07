use anyhow::Result;
use serde_json::Value;

use super::ErrorEntry;

pub(super) fn parse_idl_errors(body: &str) -> Result<Vec<ErrorEntry>> {
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

pub(super) fn parse_rust_enum_errors(body: &str) -> Vec<ErrorEntry> {
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
