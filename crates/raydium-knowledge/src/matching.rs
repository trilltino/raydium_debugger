//! Retrieval ranks candidates; known contradictions reject them before ranking.
use super::CuratedIncident;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Incidentpredicate.
pub struct IncidentPredicate {
    /// Feature.
    pub feature: String,
    /// Value.
    pub value: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
/// Matchfeatures.
pub struct MatchFeatures {
    #[serde(default)]
    /// Facts.
    pub facts: BTreeMap<String, Vec<String>>,
    /// Observed at.
    pub observed_at: Option<i64>,
}

impl MatchFeatures {
    /// Set.
    pub fn set(&mut self, feature: &str, values: impl IntoIterator<Item = String>) {
        self.facts.insert(
            feature.into(),
            values
                .into_iter()
                .map(|value| normalize(feature, &value))
                .collect(),
        );
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// Matchassessment.
pub struct MatchAssessment {
    /// Incident id.
    pub incident_id: String,
    /// Strength.
    pub strength: String,
    /// Score.
    pub score: f64,
    /// Reasons.
    pub reasons: Vec<String>,
    /// Missing signals.
    pub missing_signals: Vec<String>,
    /// Contradictions.
    pub contradictions: Vec<String>,
}

fn terms(value: &str) -> BTreeSet<String> {
    let normalized = value
        .to_lowercase()
        .replace("not showing", "visibility")
        .replace("not visible", "visibility")
        .replace("not appearing", "visibility")
        .replace("missing pool", "pool visibility")
        .replace("pool missing", "pool visibility")
        .replace("can't see", "visibility")
        .replace("cannot see", "visibility")
        .replace("insufficient funds", "balance funds")
        .replace("not enough funds", "balance funds")
        .replace("out of funds", "balance funds");
    normalized
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| {
            term.len() > 1
                && ![
                    "the", "this", "that", "my", "is", "it", "in", "on", "an", "and", "or", "of",
                    "to", "for", "after", "with", "please", "why", "does", "do", "was", "are",
                    "when", "can", "not", "no",
                ]
                .contains(term)
        })
        .map(|term| {
            match term {
                "indexer" | "indexing" => "index",
                "creation" | "created" => "create",
                "failed" | "failure" => "fail",
                "fund" => "funds",
                _ => term,
            }
            .to_string()
        })
        .collect()
}

fn normalize(feature: &str, value: &str) -> String {
    if matches!(feature, "program" | "error_program") {
        value.to_string()
    } else {
        value.to_ascii_lowercase()
    }
}

fn known(feature: &str) -> bool {
    matches!(
        feature,
        "cluster" | "program" | "product" | "instruction" | "error" | "outcome"
    )
}

/// Match incidents.
pub fn match_incidents(
    incidents: &[CuratedIncident],
    query: &str,
    features: &MatchFeatures,
) -> (Vec<CuratedIncident>, Vec<MatchAssessment>) {
    let documents = incidents.iter().map(document_terms).collect::<Vec<_>>();
    match_prepared(incidents, &documents, query, features)
}

pub(crate) fn document_terms(incident: &CuratedIncident) -> BTreeSet<String> {
    terms(&format!(
        "{} {} {} {}",
        incident.summary,
        incident.symptom_tags.join(" "),
        incident.product.as_deref().unwrap_or_default(),
        incident.failure_domain.as_deref().unwrap_or_default()
    ))
}

pub(crate) fn match_prepared(
    incidents: &[CuratedIncident],
    documents: &[BTreeSet<String>],
    query: &str,
    features: &MatchFeatures,
) -> (Vec<CuratedIncident>, Vec<MatchAssessment>) {
    let query_terms = terms(query);
    let mut ranked = Vec::new();
    let mut rejected = Vec::new();
    for (incident, document) in incidents.iter().zip(documents) {
        let mut assessment = MatchAssessment {
            incident_id: incident.id.clone(),
            strength: "weak".into(),
            score: 0.0,
            reasons: Vec::new(),
            missing_signals: Vec::new(),
            contradictions: Vec::new(),
        };
        if incident.retired {
            assessment.contradictions.push("Incident is retired".into());
        }
        if incident.valid_from.is_some() || incident.valid_until.is_some() {
            if let Some(time) = features.observed_at {
                if incident.valid_from.is_some_and(|from| time < from)
                    || incident.valid_until.is_some_and(|until| time > until)
                {
                    assessment
                        .contradictions
                        .push("Evidence date is outside incident validity".into());
                }
            } else {
                assessment
                    .missing_signals
                    .push("Evidence date for incident validity".into());
            }
        }
        let mut exact = 0;
        let mut specific = 0;
        for (predicates, kind) in [
            (&incident.must, "must"),
            (&incident.should, "should"),
            (&incident.must_not, "must_not"),
        ] {
            for predicate in predicates {
                let label = format!("{}={}", predicate.feature, predicate.value);
                if !known(&predicate.feature) {
                    assessment
                        .contradictions
                        .push(format!("Unsupported predicate: {label}"));
                    continue;
                }
                // An error number has no program identity. Never promote an unscoped code.
                if predicate.feature == "error"
                    && !incident.must.iter().any(|item| item.feature == "program")
                {
                    assessment
                        .contradictions
                        .push("Error predicate requires an explicit program predicate".into());
                    continue;
                }
                if predicate.feature == "error" {
                    let expected_program = &incident
                        .must
                        .iter()
                        .find(|item| item.feature == "program")
                        .unwrap()
                        .value;
                    match features.facts.get("error_program") {
                        Some(values) if !values.iter().any(|value| value == expected_program) => {
                            assessment
                                .contradictions
                                .push("Custom error belongs to a different program".into())
                        }
                        None => assessment
                            .missing_signals
                            .push("Program attribution for custom error".into()),
                        _ => {}
                    }
                }
                match features.facts.get(&predicate.feature) {
                    Some(values) => {
                        let matches = values.iter().any(|value| {
                            normalize(&predicate.feature, value)
                                == normalize(&predicate.feature, &predicate.value)
                        });
                        if (kind == "must" && !matches) || (kind == "must_not" && matches) {
                            assessment
                                .contradictions
                                .push(format!("{kind} contradicted: {label}"));
                        } else if matches && kind != "must_not" {
                            assessment.reasons.push(format!("Observed {label}"));
                            exact += 1;
                            if matches!(
                                predicate.feature.as_str(),
                                "program" | "error" | "instruction" | "product"
                            ) {
                                specific += 1;
                            }
                        }
                    }
                    None => assessment
                        .missing_signals
                        .push(format!("Unknown {kind}: {label}")),
                }
            }
        }
        if let (Some(product), Some(values)) = (&incident.product, features.facts.get("product")) {
            let normalized = product
                .strip_prefix("raydium_")
                .unwrap_or(product)
                .to_lowercase();
            if !values
                .iter()
                .any(|value| value.strip_prefix("raydium_").unwrap_or(value) == normalized)
            {
                assessment
                    .contradictions
                    .push("Observed product is outside incident scope".into());
            }
        }
        // Successful current execution cannot substantiate a historical execution failure.
        if features
            .facts
            .get("outcome")
            .is_some_and(|values| values.iter().any(|value| value == "success"))
            && matches!(
                incident.failure_domain.as_deref(),
                Some("execution" | "on_chain" | "program_error")
            )
        {
            assessment
                .contradictions
                .push("Current transaction succeeded; execution failure contradicted".into());
        }
        // Match the reported problem, not incidental words in the proposed resolution.
        let overlap: Vec<_> = query_terms.intersection(document).cloned().collect();
        let coverage = overlap.len() as f64 / query_terms.len().max(1) as f64;
        let eligible_text = !overlap.is_empty()
            && coverage >= 0.5
            && (overlap.len() >= 2 || query_terms.len() == 1);
        if !eligible_text && specific == 0 {
            continue;
        }
        if !overlap.is_empty() {
            assessment
                .reasons
                .push(format!("Problem terms: {}", overlap.join(", ")));
        }
        // Basic negation protects against reporting the explicitly denied symptom as a cause.
        let lowered = query.to_lowercase();
        for term in &overlap {
            if lowered.contains(&format!("no {term}")) || lowered.contains(&format!("not {term}")) {
                assessment
                    .contradictions
                    .push(format!("Reported symptom denies {term}"));
            }
        }
        assessment.score = coverage + exact as f64 * 2.0;
        if !assessment.contradictions.is_empty() {
            assessment.strength = "rejected".into();
            rejected.push(assessment);
            continue;
        }
        assessment.strength = if assessment.missing_signals.is_empty() && exact >= 2 {
            "strong"
        } else if assessment.missing_signals.is_empty() && (exact > 0 || coverage >= 0.75) {
            "moderate"
        } else {
            "weak"
        }
        .into();
        ranked.push((incident, assessment));
    }
    ranked.sort_by(|a, b| {
        b.1.score
            .total_cmp(&a.1.score)
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    ranked.truncate(10);
    let (incidents, mut assessments): (Vec<_>, Vec<_>) = ranked
        .into_iter()
        .map(|(incident, assessment)| (incident.clone(), assessment))
        .unzip();
    rejected.sort_by(|a, b| a.incident_id.cmp(&b.incident_id));
    rejected.truncate(10);
    assessments.extend(rejected);
    (incidents, assessments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_requirements_weaken_and_known_forbidden_facts_reject() {
        let incident: CuratedIncident = serde_json::from_value(serde_json::json!({
            "id":"scope", "summary":"Pool visibility indexing", "resolution":"Check visibility",
            "product":null, "failure_domain":"indexing", "symptom_tags":[], "evidence_message_count":1,
            "must":[{"feature":"instruction","value":"initialize"}],
            "must_not":[{"feature":"cluster","value":"mainnet"}],
            "should":[{"feature":"outcome","value":"success"}]
        })).unwrap();
        let (matches, assessments) = match_incidents(
            std::slice::from_ref(&incident),
            "pool visibility",
            &MatchFeatures::default(),
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(assessments[0].strength, "weak");
        assert_eq!(assessments[0].missing_signals.len(), 3);
        let mut facts = MatchFeatures::default();
        facts.set("instruction", ["initialize".into()]);
        facts.set("cluster", ["mainnet".into()]);
        let (matches, assessments) = match_incidents(&[incident], "pool visibility", &facts);
        assert!(matches.is_empty());
        assert_eq!(assessments[0].strength, "rejected");
        assert!(assessments[0].contradictions[0].contains("must_not"));
    }

    #[test]
    fn program_identity_preserves_case() {
        let mut facts = MatchFeatures::default();
        facts.set("program", ["CaseSensitiveProgram".into()]);
        assert_eq!(facts.facts["program"], vec!["CaseSensitiveProgram"]);
    }
}

/// Validated predicate key, preserving existing JSON feature names.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateKey {
    /// Network scope.
    Cluster,
    /// Case-sensitive program identity.
    Program,
    /// Raydium product.
    Product,
    /// Decoded instruction semantic name.
    Instruction,
    /// Numeric custom error, scoped by program evidence.
    Error,
    /// Success or failure.
    Outcome,
}
impl IncidentPredicate {
    /// Validates supported keys and non-empty bounded values at artifact publication/load.
    pub fn validate(&self) -> anyhow::Result<PredicateKey> {
        anyhow::ensure!(
            !self.value.trim().is_empty() && self.value.len() <= 256,
            "invalid predicate value"
        );
        let key = match self.feature.as_str() {
            "cluster" => PredicateKey::Cluster,
            "program" => PredicateKey::Program,
            "product" => PredicateKey::Product,
            "instruction" => PredicateKey::Instruction,
            "error" => PredicateKey::Error,
            "outcome" => PredicateKey::Outcome,
            _ => anyhow::bail!("unknown predicate feature"),
        };
        if matches!(key, PredicateKey::Program) {
            anyhow::ensure!(
                self.value.chars().all(|c| {
                    "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".contains(c)
                }),
                "invalid program predicate"
            );
        }
        Ok(key)
    }
}
