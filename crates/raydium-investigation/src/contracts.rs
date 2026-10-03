//! Stable serialized contracts. Unknown evidence is represented with Option, never fabricated.
use super::*;
#[derive(Debug, Clone, Deserialize, Serialize)]
/// Investigationlookup.
pub struct InvestigationLookup {
    /// Investigation id.
    pub investigation_id: String,
    /// Stable lifecycle spelling; terminal runs are never silently retried.
    pub status: String,
    /// Result.
    pub result: Option<InvestigationResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// Investigationrequest.
pub struct InvestigationRequest {
    /// Optional public transaction identity; never used as a private archive lookup.
    pub signature: Option<String>,
    /// Operator-reported symptom, bounded to 1,000 Unicode characters.
    pub symptom: Option<String>,
    /// Cluster.
    pub cluster: Option<RpcCluster>,
    #[serde(default)]
    /// Recent fingerprint.
    pub recent_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// Investigationevidence.
pub struct InvestigationEvidence {
    /// Evidence id.
    pub evidence_id: String,
    /// Evidence type.
    pub evidence_type: String,
    /// Source reference.
    pub source_reference: String,
    /// Summary.
    pub summary: String,
    /// Observation time in Unix seconds; None means unavailable from the evidence source.
    pub observed_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// Investigationresult.
pub struct InvestigationResult {
    /// Investigation id.
    pub investigation_id: String,
    /// Stable lifecycle spelling; terminal runs are never silently retried.
    pub status: String,
    /// Signature.
    pub signature: Option<String>,
    /// Operator-reported symptom, bounded to 1,000 Unicode characters.
    pub symptom: Option<String>,
    /// Cluster.
    pub cluster: Option<String>,
    /// Transaction diagnosis.
    pub transaction_diagnosis: Option<DiagnosticResponse>,
    /// Transaction error.
    pub transaction_error: Option<String>,
    /// Related incidents.
    pub related_incidents: Vec<CuratedIncident>,
    #[serde(default)]
    /// Incident matches.
    pub incident_matches: Vec<MatchAssessment>,
    /// Recent observations.
    pub recent_observations: Vec<RecentObservationSummary>,
    /// Evidence.
    pub evidence: Vec<InvestigationEvidence>,
    /// Unknowns.
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// Investigationevent.
pub struct InvestigationEvent {
    /// Monotonic persisted sequence, serialized as a string to avoid JavaScript rounding.
    #[serde(default)]
    pub event_id: String,
    /// Stable progress, complete or error event spelling.
    pub event_type: String,
    /// Investigation id.
    pub investigation_id: String,
    /// Stage.
    pub stage: Option<String>,
    /// Message.
    pub message: Option<String>,
    /// Result.
    pub result: Option<InvestigationResult>,
    /// Error.
    pub error: Option<String>,
}
