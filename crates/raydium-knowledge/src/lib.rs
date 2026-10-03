//! Sanitized incident contracts and deterministic matching; never accesses a private archive.
#![warn(missing_docs)]
use matching::IncidentPredicate;
use serde::{Deserialize, Serialize};
/// Predicate-aware deterministic retrieval.
pub mod matching;
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
/// Curatedincident.
pub struct CuratedIncident {
    /// Id.
    pub id: String,
    /// Product.
    pub product: Option<String>,
    /// Failure domain.
    pub failure_domain: Option<String>,
    /// Summary.
    pub summary: String,
    /// Human-curated sanitized action; excluded from runtime query matching.
    pub resolution: String,
    /// Symptom tags.
    pub symptom_tags: Vec<String>,
    /// Evidence message count.
    pub evidence_message_count: usize,
    #[serde(default)]
    /// Required predicates; unknown facts weaken matches, contradictions reject them.
    pub must: Vec<IncidentPredicate>,
    #[serde(default)]
    /// Should.
    pub should: Vec<IncidentPredicate>,
    #[serde(default)]
    /// Forbidden predicates; known matching facts reject candidates.
    pub must_not: Vec<IncidentPredicate>,
    #[serde(default)]
    /// Retired.
    pub retired: bool,
    #[serde(default)]
    /// Inclusive validity start in Unix seconds; None is unbounded.
    pub valid_from: Option<i64>,
    #[serde(default)]
    /// Valid until.
    pub valid_until: Option<i64>,
}

mod registry;
pub use registry::{CompiledRegistry, RegistryCache};
