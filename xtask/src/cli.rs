use anyhow::{anyhow, Result};

use crate::registry::{errors, instructions};

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Generate,
    Validate,
    Drift,
}

impl Action {
    fn parse(registry: &str, command: Option<&str>) -> Result<Self> {
        match command {
            Some("generate") => Ok(Self::Generate),
            Some("validate" | "check") => Ok(Self::Validate),
            Some("drift") => Ok(Self::Drift),
            other => Err(anyhow!(
                "unknown {registry} command {other:?}; use generate, validate, or drift"
            )),
        }
    }
}

pub(crate) fn run(args: &[String]) -> Result<()> {
    let command = args.get(1).map(String::as_str);
    match args.first().map(String::as_str) {
        Some("raydium-registry") => match Action::parse("raydium-registry", command)? {
            Action::Generate => errors::generate(),
            Action::Validate => errors::validate(),
            Action::Drift => errors::drift(),
        },
        Some("raydium-instructions") => match Action::parse("raydium-instructions", command)? {
            Action::Generate => instructions::generate(),
            Action::Validate => instructions::validate(),
            Action::Drift => instructions::drift(),
        },
        Some("support-knowledge") => raydium_knowledge_builder::run(&args[1..]),
        _ => Err(anyhow!(
            "usage: cargo run -p xtask -- <raydium-registry|raydium-instructions> <command> | support-knowledge import-html <export-directory> [database-path]"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_remains_an_alias_for_validate() {
        for registry in ["raydium-registry", "raydium-instructions"] {
            assert_eq!(
                Action::parse(registry, Some("check")).unwrap(),
                Action::Validate
            );
            assert_eq!(
                Action::parse(registry, Some("validate")).unwrap(),
                Action::Validate
            );
        }
    }

    #[test]
    fn invalid_actions_identify_the_registry_and_command() {
        let error = Action::parse("raydium-instructions", Some("unknown")).unwrap_err();
        assert_eq!(
            error.to_string(),
            "unknown raydium-instructions command Some(\"unknown\"); use generate, validate, or drift"
        );
        assert!(Action::parse("raydium-registry", None).is_err());
    }

    #[test]
    fn missing_or_unknown_task_returns_usage() {
        assert!(run(&[]).unwrap_err().to_string().starts_with("usage:"));
        assert!(run(&["unknown".into()])
            .unwrap_err()
            .to_string()
            .starts_with("usage:"));
    }
}
