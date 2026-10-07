use std::collections::BTreeSet;
use std::path::Path;

use raydium_knowledge::{matching::MatchFeatures, CompiledRegistry};
use serde::{Deserialize, Serialize};

use crate::debug::TransactionDebugInfo;

/// Availability of approved historical guidance for this AI request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiKnowledgeStatus {
    /// The configured artifact could not be loaded or validated.
    #[default]
    Unavailable,
    /// The artifact is valid but contains no approved incidents.
    Empty,
    /// Approved incidents exist, but none match the supplied evidence.
    NoMatch,
    /// One or more approved incidents match without known contradictions.
    Matched,
}

/// Bounded, sanitized historical guidance included in the model prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiIncidentEvidence {
    /// Stable source ID used for citations.
    pub incident_id: String,
    /// Reviewed problem summary.
    pub summary: String,
    /// Reviewed resolution, offered as historical guidance.
    pub resolution: String,
    /// Deterministic match strength, separate from transaction facts.
    pub strength: String,
    /// Reasons for selecting this incident.
    pub reasons: Vec<String>,
    /// Facts still needed to establish that this incident applies.
    pub missing_signals: Vec<String>,
}

/// Reviewed current advice, distinct from a historical incident outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiGuidanceEvidence {
    /// Stable citation ID.
    pub guidance_id: String,
    /// Sanitized problem summary.
    pub summary: String,
    /// Reviewer-approved current guidance.
    pub guidance: String,
    /// Why this advice matched the question.
    pub matched_terms: Vec<String>,
}

/// Server-side retrieval result. It never contains private archive messages.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiKnowledgeContext {
    /// Artifact availability and match outcome.
    pub status: AiKnowledgeStatus,
    /// Number of incidents in the validated artifact before matching.
    pub incident_count: usize,
    /// At most three matching reviewed summaries and resolutions.
    pub incidents: Vec<AiIncidentEvidence>,
    /// Number of separately approved current guidance entries.
    #[serde(default)]
    pub guidance_count: usize,
    /// At most three current guidance matches.
    #[serde(default)]
    pub guidance: Vec<AiGuidanceEvidence>,
}

impl AiKnowledgeContext {
    pub(super) fn load(path: &Path, info: &TransactionDebugInfo, question: &str) -> Self {
        let mut context = match CompiledRegistry::load(path) {
            Ok(registry) => Self::retrieve(&registry, info, question),
            Err(_) => Self::default(),
        };
        if context.status != AiKnowledgeStatus::Unavailable {
            if let Ok((count, guidance)) = load_guidance(path, question, info) {
                context.guidance_count = count;
                context.guidance = guidance;
            }
        }
        context
    }

    fn retrieve(registry: &CompiledRegistry, info: &TransactionDebugInfo, question: &str) -> Self {
        let incident_count = registry.incidents().len();
        if incident_count == 0 {
            return Self {
                status: AiKnowledgeStatus::Empty,
                ..Self::default()
            };
        }

        let features = transaction_features(info);
        let (mut incidents, mut assessments) =
            registry.matches(&bounded(question, 1000), &features);
        if incidents.is_empty() {
            let diagnosis = info
                .failure
                .as_ref()
                .map(|failure| bounded(&failure.user_message, 500))
                .unwrap_or_else(|| bounded(&info.root_cause.summary, 500));
            (incidents, assessments) = registry.matches(&diagnosis, &features);
        }
        let incidents: Vec<_> = incidents
            .into_iter()
            .take(3)
            .filter_map(|incident| {
                let assessment = assessments
                    .iter()
                    .find(|item| item.incident_id == incident.id)?;
                Some(AiIncidentEvidence {
                    incident_id: incident.id,
                    summary: bounded(&incident.summary, 500),
                    resolution: bounded(&incident.resolution, 1000),
                    strength: assessment.strength.clone(),
                    reasons: assessment
                        .reasons
                        .iter()
                        .take(4)
                        .map(|text| bounded(text, 200))
                        .collect(),
                    missing_signals: assessment
                        .missing_signals
                        .iter()
                        .take(4)
                        .map(|text| bounded(text, 200))
                        .collect(),
                })
            })
            .collect();
        Self {
            status: if incidents.is_empty() {
                AiKnowledgeStatus::NoMatch
            } else {
                AiKnowledgeStatus::Matched
            },
            incident_count,
            incidents,
            ..Self::default()
        }
    }
}

#[derive(Deserialize)]
struct GuidanceArtifact {
    #[serde(default)]
    guidance: Vec<CompiledGuidance>,
}

#[derive(Deserialize)]
struct CompiledGuidance {
    id: String,
    product: Option<String>,
    failure_domain: Option<String>,
    category: String,
    summary: String,
    guidance: String,
}

fn load_guidance(
    path: &Path,
    question: &str,
    info: &TransactionDebugInfo,
) -> anyhow::Result<(usize, Vec<AiGuidanceEvidence>)> {
    let artifact: GuidanceArtifact = serde_json::from_slice(&std::fs::read(path)?)?;
    let count = artifact.guidance.len();
    let mut query = terms(question);
    if let Some(failure) = &info.failure {
        query.extend(terms(&failure.user_message));
    }
    let mut scored = Vec::new();
    for entry in artifact.guidance {
        let document = terms(&format!(
            "{} {} {} {}",
            entry.summary,
            entry.category,
            entry.product.as_deref().unwrap_or(""),
            entry.failure_domain.as_deref().unwrap_or("")
        ));
        let overlap: Vec<String> = query.intersection(&document).cloned().collect();
        if overlap.len() < 2 {
            continue;
        }
        scored.push((
            overlap.len(),
            AiGuidanceEvidence {
                guidance_id: entry.id,
                summary: bounded(&entry.summary, 500),
                guidance: bounded(&entry.guidance, 1_000),
                matched_terms: overlap.into_iter().take(5).collect(),
            },
        ));
    }
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.guidance_id.cmp(&b.1.guidance_id))
    });
    Ok((
        count,
        scored.into_iter().take(3).map(|(_, item)| item).collect(),
    ))
}

fn terms(text: &str) -> BTreeSet<String> {
    const STOP: &[&str] = &[
        "about", "after", "again", "could", "does", "from", "have", "into", "just", "more",
        "please", "should", "some", "that", "their", "there", "these", "this", "when", "where",
        "which", "with", "would", "your",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 4 && !STOP.contains(word))
        .map(str::to_owned)
        .collect()
}

fn bounded(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn transaction_features(info: &TransactionDebugInfo) -> MatchFeatures {
    let mut features = MatchFeatures {
        observed_at: info.timestamp,
        ..MatchFeatures::default()
    };
    if let Some(cluster) = &info.provider.cluster {
        features.set("cluster", [cluster.clone()]);
    }
    features.set("program", info.program_ids.clone());
    features.set(
        "outcome",
        [if info.success { "success" } else { "failure" }.into()],
    );
    if let Some(product) = &info.raydium_product {
        if let Ok(serde_json::Value::String(product)) = serde_json::to_value(&product.product) {
            features.set("product", [product]);
        }
    }
    if !info.decoded_instructions.is_empty()
        && info
            .decoded_instructions
            .iter()
            .all(|instruction| instruction.semantic_decode.is_some())
    {
        features.set(
            "instruction",
            info.decoded_instructions.iter().filter_map(|instruction| {
                instruction
                    .semantic_decode
                    .as_ref()
                    .map(|decoded| decoded.instruction_name.clone())
            }),
        );
    }
    let failed_program = info
        .logs
        .iter()
        .find_map(|line| {
            let (program, error) = line.strip_prefix("Program ")?.split_once(" failed: ")?;
            Some((program.to_string(), crate::parse_custom_error_code(error)))
        })
        .or_else(|| {
            info.failure.as_ref().and_then(|failure| {
                failure
                    .program_id
                    .clone()
                    .map(|program| (program, failure.code_decimal))
            })
        });
    if let Some((program, code)) = failed_program {
        features.set("error_program", [program]);
        if let Some(code) = code {
            features.set("error", [code.to_string()]);
        }
    }
    features
}

#[cfg(test)]
mod tests {
    use super::*;
    use raydium_knowledge::{matching::IncidentPredicate, CuratedIncident};

    fn incident(id: &str) -> CuratedIncident {
        serde_json::from_value(serde_json::json!({
            "id": id, "product": null, "failure_domain": "indexing",
            "summary": "pool visibility", "resolution": "Check the pool account and indexer refresh.",
            "symptom_tags": ["pool_visibility"], "evidence_message_count": 4
        })).unwrap()
    }

    #[test]
    fn distinguishes_empty_missing_and_nonmatching_knowledge() {
        let info = TransactionDebugInfo::default();
        let empty = CompiledRegistry::new(vec![]).unwrap();
        assert_eq!(
            AiKnowledgeContext::retrieve(&empty, &info, "pool visibility").status,
            AiKnowledgeStatus::Empty
        );
        let registry = CompiledRegistry::new(vec![incident("approved-case")]).unwrap();
        assert_eq!(
            AiKnowledgeContext::retrieve(&registry, &info, "unrelated wallet recovery").status,
            AiKnowledgeStatus::NoMatch
        );
        assert_eq!(
            AiKnowledgeContext::load(
                Path::new("missing-knowledge-artifact.json"),
                &info,
                "pool visibility"
            )
            .status,
            AiKnowledgeStatus::Unavailable
        );
    }

    #[test]
    fn includes_reviewed_guidance_with_stable_citations_and_limits() {
        let mut incidents: Vec<_> = (0..6).map(|n| incident(&format!("approved-{n}"))).collect();
        incidents[0].resolution = "é".repeat(2000);
        let registry = CompiledRegistry::new(incidents).unwrap();
        let context = AiKnowledgeContext::retrieve(
            &registry,
            &TransactionDebugInfo::default(),
            "pool visibility",
        );
        assert_eq!(context.status, AiKnowledgeStatus::Matched);
        assert_eq!(context.incident_count, 6);
        assert_eq!(context.incidents.len(), 3);
        assert!(context
            .incidents
            .iter()
            .all(|item| item.resolution.chars().count() <= 1000));
        assert!(context
            .incidents
            .iter()
            .all(|item| !item.reasons.is_empty()));
    }

    #[test]
    fn rejects_contradicted_programs_retired_cases_and_successful_execution() {
        let mut scoped = incident("wrong-program");
        scoped.must = vec![IncidentPredicate {
            feature: "program".into(),
            value: "2".repeat(32),
        }];
        let mut retired = incident("retired");
        retired.retired = true;
        let mut execution = incident("execution-failure");
        execution.failure_domain = Some("execution".into());
        let registry = CompiledRegistry::new(vec![scoped, retired, execution]).unwrap();
        let info = TransactionDebugInfo {
            success: true,
            program_ids: vec!["1".repeat(32)],
            ..Default::default()
        };
        let context = AiKnowledgeContext::retrieve(&registry, &info, "pool visibility");
        assert_eq!(context.status, AiKnowledgeStatus::NoMatch);
        assert!(context.incidents.is_empty());
    }

    #[test]
    fn attributes_custom_errors_to_the_failed_program_and_keeps_unknown_instructions_unknown() {
        let info = TransactionDebugInfo {
            logs: vec![
                "Program 11111111111111111111111111111111 failed: custom program error: 0x1771"
                    .into(),
            ],
            ..Default::default()
        };
        let features = transaction_features(&info);
        assert_eq!(features.facts["error_program"], vec!["1".repeat(32)]);
        assert_eq!(features.facts["error"], vec!["6001"]);
        assert!(!features.facts.contains_key("instruction"));
    }

    #[test]
    fn loads_only_sanitized_fields_and_handles_malformed_artifacts() {
        let path = std::env::temp_dir().join(format!(
            "raydium-ai-knowledge-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let artifact = serde_json::json!({ "incidents": [incident("reviewed-case")],
            "private_messages": ["PRIVATE_ARCHIVE_SENTINEL"] });
        std::fs::write(&path, serde_json::to_vec(&artifact).unwrap()).unwrap();
        let info = TransactionDebugInfo::default();
        let context = AiKnowledgeContext::load(&path, &info, "pool visibility");
        std::fs::write(&path, b"not valid JSON PRIVATE_ARCHIVE_SENTINEL").unwrap();
        let malformed = AiKnowledgeContext::load(&path, &info, "pool visibility");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(context.status, AiKnowledgeStatus::Matched);
        assert!(!serde_json::to_string(&context)
            .unwrap()
            .contains("PRIVATE_ARCHIVE_SENTINEL"));
        assert_eq!(malformed.status, AiKnowledgeStatus::Unavailable);
    }

    #[test]
    fn current_guidance_is_retrieved_separately_from_historical_incidents() {
        let path = std::env::temp_dir().join(format!(
            "raydium-guidance-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let artifact = serde_json::json!({"schema_version":1,"incidents":[],"guidance":[{
            "id":"orphan-pool","product":"CLMM","failure_domain":"pool visibility",
            "category":"pool search","summary":"Find a CLMM pool",
            "guidance":"Search by pool address in the Raydium interface.",
            "evidence_message_count":1,"reference_count":0
        }]});
        std::fs::write(&path, serde_json::to_vec(&artifact).unwrap()).unwrap();
        let context = AiKnowledgeContext::load(
            &path,
            &TransactionDebugInfo::default(),
            "How can I find a CLMM pool?",
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(context.status, AiKnowledgeStatus::Empty);
        assert!(context.incidents.is_empty());
        assert_eq!(context.guidance_count, 1);
        assert_eq!(context.guidance[0].guidance_id, "orphan-pool");
    }

    #[test]
    fn uses_diagnosis_when_a_general_question_has_no_text_match() {
        let registry = CompiledRegistry::new(vec![incident("reviewed-case")]).unwrap();
        let mut info = TransactionDebugInfo::default();
        info.root_cause.summary = "pool visibility".into();
        let context = AiKnowledgeContext::retrieve(&registry, &info, "What should I check?");
        assert_eq!(context.status, AiKnowledgeStatus::Matched);
    }
}
