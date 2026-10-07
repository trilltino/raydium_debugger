//! Bounded public upgrade notices supplied separately from reviewed incidents.
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::debug::TransactionDebugInfo;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiUpdateStatus {
    #[default]
    Unavailable,
    Empty,
    NoMatch,
    Matched,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiUpdateEvidence {
    pub update_id: String,
    pub date: String,
    pub announced_at: Option<String>,
    /// Planned, live, delayed, or unknown; never inferred from a future date.
    pub status: String,
    pub summary: String,
    pub excerpt: String,
    pub source_url: String,
    pub reference_repo: Option<String>,
    pub reference_commit: Option<String>,
    pub reference_path: Option<String>,
    pub reference_excerpt: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiUpdateContext {
    pub status: AiUpdateStatus,
    pub update_count: usize,
    pub updates: Vec<AiUpdateEvidence>,
}

#[derive(Deserialize)]
struct UpdateArtifact {
    schema_version: u32,
    entries: Vec<UpdateEntry>,
}

#[derive(Deserialize)]
struct UpdateEntry {
    id: String,
    date: String,
    #[serde(default)]
    announced_at: Option<String>,
    status: String,
    summary: String,
    body: String,
    source_url: String,
    reference_repo: Option<String>,
    reference_commit: Option<String>,
    reference_path: Option<String>,
    #[serde(default)]
    reference_body: Option<String>,
}

impl AiUpdateContext {
    pub(super) fn load(path: &Path, info: &TransactionDebugInfo, question: &str) -> Self {
        Self::try_load(path, info, question).unwrap_or_default()
    }

    fn try_load(path: &Path, info: &TransactionDebugInfo, question: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            std::fs::metadata(path)?.len() <= 4 * 1024 * 1024,
            "update artifact too large"
        );
        let artifact: UpdateArtifact = serde_json::from_slice(&std::fs::read(path)?)?;
        anyhow::ensure!(
            artifact.schema_version == 1,
            "unsupported update artifact schema"
        );
        if artifact.entries.is_empty() {
            return Ok(Self {
                status: AiUpdateStatus::Empty,
                ..Self::default()
            });
        }
        let count = artifact.entries.len();
        let query = words(question);
        let scope = product_scope(&query);
        if query.is_empty() {
            return Ok(Self {
                status: AiUpdateStatus::NoMatch,
                update_count: count,
                ..Self::default()
            });
        }
        let tx_date = info
            .timestamp
            .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
            .map(|date| date.format("%Y-%m-%d").to_string());
        let mut scored = Vec::new();
        for entry in artifact.entries {
            if let (Some(scope), Some(path)) = (scope, entry.reference_path.as_deref()) {
                if !path.contains(scope) {
                    continue;
                }
            }
            anyhow::ensure!(
                matches!(
                    entry.status.as_str(),
                    "planned" | "live" | "delayed" | "unknown"
                ),
                "invalid update status"
            );
            anyhow::ensure!(
                entry.date.len() == 10
                    && ((entry.id.starts_with("announcement:")
                        && entry.source_url.starts_with("https://t.me/"))
                        || (entry.id.starts_with("reference:")
                            && entry.source_url.starts_with("https://docs.raydium.io/"))),
                "invalid update provenance"
            );
            let announced_at = entry
                .announced_at
                .as_deref()
                .map(DateTime::parse_from_rfc3339)
                .transpose()?;
            if info.timestamp.is_some_and(|timestamp| {
                announced_at
                    .as_ref()
                    .is_some_and(|date| date.timestamp() > timestamp)
            }) || (announced_at.is_none()
                && tx_date
                    .as_ref()
                    .is_some_and(|date| entry.date.as_str() > date.as_str()))
            {
                continue;
            }
            let title_words = words(&entry.summary);
            let body_words = words(&entry.body);
            let reference_words = words(entry.reference_body.as_deref().unwrap_or(""));
            let score = query
                .iter()
                .filter(|term| title_words.contains(*term))
                .count()
                * 3
                + query
                    .iter()
                    .filter(|term| body_words.contains(*term))
                    .count()
                + query
                    .iter()
                    .filter(|term| reference_words.contains(*term))
                    .count()
                    * 2;
            if score > 0 {
                scored.push((score, entry));
            }
        }
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.date.cmp(&a.1.date))
                .then_with(|| a.1.id.cmp(&b.1.id))
        });
        let updates = scored
            .into_iter()
            .take(3)
            .map(|(_, entry)| AiUpdateEvidence {
                update_id: entry.id,
                date: entry.date,
                announced_at: entry.announced_at,
                status: entry.status,
                summary: bounded(&entry.summary, 300),
                excerpt: bounded(&entry.body, 1000),
                source_url: entry.source_url,
                reference_repo: entry.reference_repo,
                reference_commit: entry.reference_commit,
                reference_path: entry.reference_path,
                reference_excerpt: entry
                    .reference_body
                    .as_deref()
                    .map(|body| bounded(body, 1800)),
            })
            .collect::<Vec<_>>();
        Ok(Self {
            status: if updates.is_empty() {
                AiUpdateStatus::NoMatch
            } else {
                AiUpdateStatus::Matched
            },
            update_count: count,
            updates,
        })
    }
}

fn bounded(value: &str, chars: usize) -> String {
    value.chars().take(chars).collect()
}

fn product_scope(query: &std::collections::BTreeSet<String>) -> Option<&'static str> {
    let named = ["clmm", "cpmm", "launchlab", "ammv4"]
        .iter()
        .filter(|name| query.contains(**name))
        .count();
    if named > 1 {
        return None;
    }
    if query.contains("clmm") {
        Some("clmm")
    } else if query.contains("cpmm") {
        Some("cpmm")
    } else if query.contains("launchlab") {
        Some("launchlab")
    } else if query.contains("ammv4") || (query.contains("amm") && query.contains("v4")) {
        Some("amm-v4")
    } else if query.contains("stable") && query.contains("amm") {
        Some("stable-amm")
    } else {
        None
    }
}

fn words(value: &str) -> std::collections::BTreeSet<String> {
    value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| {
            (word.len() >= 3 || matches!(*word, "v2" | "v3" | "v4"))
                && !matches!(
                    *word,
                    "the"
                        | "and"
                        | "for"
                        | "with"
                        | "from"
                        | "this"
                        | "that"
                        | "what"
                        | "when"
                        | "where"
                        | "does"
                        | "have"
                        | "been"
                        | "will"
                        | "would"
                        | "could"
                        | "about"
                        | "your"
                        | "their"
                        | "into"
                        | "after"
                        | "before"
                        | "user"
                        | "users"
                        | "update"
                        | "upgrade"
                        | "raydium"
                )
        })
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planned_and_live_notices_keep_status_and_transaction_date() {
        let path =
            std::env::temp_dir().join(format!("upgrade-context-{}.json", std::process::id()));
        let artifact = serde_json::json!({"schema_version":1,"entries":[
            {"id":"announcement:message25","date":"2026-07-13","status":"planned","summary":"AMMv4 OpenBook removal planned","body":"AMMv4 OpenBook accounts change after deployment","source_url":"https://t.me/RaydiumDeveloperUpdates/25","reference_repo":null,"reference_commit":null,"reference_path":null},
            {"id":"announcement:message26","date":"2026-07-22","status":"live","summary":"AMMv4 OpenBook removal deployed","body":"AMMv4 OpenBook dependency removed","source_url":"https://t.me/RaydiumDeveloperUpdates/26","reference_repo":null,"reference_commit":null,"reference_path":null}
        ]});
        std::fs::write(&path, serde_json::to_vec(&artifact).unwrap()).unwrap();
        let before_live = TransactionDebugInfo {
            timestamp: Some(1784419200),
            ..TransactionDebugInfo::default()
        };
        let result = AiUpdateContext::load(&path, &before_live, "AMMv4 OpenBook");
        assert_eq!(result.status, AiUpdateStatus::Matched);
        assert_eq!(result.updates.len(), 1);
        assert_eq!(result.updates[0].status, "planned");
        let current =
            AiUpdateContext::load(&path, &TransactionDebugInfo::default(), "AMMv4 OpenBook");
        assert_eq!(current.updates.len(), 2);
        assert_eq!(current.updates[0].status, "live");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn explicit_product_excludes_other_split_announcements() {
        let path = std::env::temp_dir().join(format!("upgrade-scope-{}.json", std::process::id()));
        let artifact = serde_json::json!({"schema_version":1,"entries":[
            {"id":"announcement:message28:clmm","date":"2026-08-10","status":"planned","summary":"CLMM fix planned","body":"CLMM CPMM changes","source_url":"https://t.me/RaydiumDeveloperUpdates/28","reference_repo":null,"reference_commit":null,"reference_path":"reference/changelog/2026-08-17-clmm-fix.mdx"},
            {"id":"announcement:message28:cpmm","date":"2026-08-10","status":"planned","summary":"CPMM fix planned","body":"CLMM CPMM changes","source_url":"https://t.me/RaydiumDeveloperUpdates/28","reference_repo":null,"reference_commit":null,"reference_path":"reference/changelog/2026-08-17-cpmm-fix.mdx"}
        ]});
        std::fs::write(&path, serde_json::to_vec(&artifact).unwrap()).unwrap();
        let result = AiUpdateContext::load(&path, &TransactionDebugInfo::default(), "CLMM fix");
        assert_eq!(result.updates.len(), 1);
        assert_eq!(result.updates[0].update_id, "announcement:message28:clmm");
        std::fs::remove_file(&path).unwrap();
    }
}
