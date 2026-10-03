//! Reviewed-query evaluation is operator tooling, separate from runtime retrieval.
use raydium_knowledge::{
    matching::{match_incidents, MatchFeatures},
    CuratedIncident,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Deserialize)]
struct Benchmark {
    review_status: String,
    reviewer: Option<String>,
    incidents: Vec<CuratedIncident>,
    queries: Vec<LabeledQuery>,
}

#[derive(Deserialize)]
struct LabeledQuery {
    id: String,
    query: String,
    #[serde(default)]
    features: MatchFeatures,
    relevant: Vec<String>,
    #[serde(default)]
    reviewer: Option<String>,
    #[serde(default)]
    forbidden: Vec<String>,
}

#[derive(Debug, Serialize)]
/// Benchmarkreport.
pub struct BenchmarkReport {
    /// Reviewed.
    pub reviewed: bool,
    /// Queries.
    pub queries: usize,
    /// Precision at 3.
    pub precision_at_3: f64,
    /// Recall at 3.
    pub recall_at_3: f64,
    /// Mean reciprocal rank.
    pub mean_reciprocal_rank: f64,
    /// Baseline recall at 3.
    pub baseline_recall_at_3: f64,
    /// Correct abstentions.
    pub correct_abstentions: usize,
    pub coverage: f64,
    pub abstention_rate: f64,
    /// Contradiction violations.
    pub contradiction_violations: usize,
    /// P50 micros.
    pub p50_micros: u128,
    /// P95 micros.
    pub p95_micros: u128,
    /// Quality gate.
    pub quality_gate: bool,
    /// Failures.
    pub failures: Vec<String>,
}

/// Evaluate benchmark.
pub fn evaluate_benchmark(path: &std::path::Path) -> anyhow::Result<BenchmarkReport> {
    let benchmark: Benchmark = serde_json::from_slice(&std::fs::read(path)?)?;
    anyhow::ensure!(!benchmark.queries.is_empty(), "benchmark has no queries");
    let ids: BTreeSet<_> = benchmark
        .incidents
        .iter()
        .map(|incident| incident.id.as_str())
        .collect();
    anyhow::ensure!(
        ids.len() == benchmark.incidents.len(),
        "duplicate benchmark incident IDs"
    );
    let mut report = BenchmarkReport {
        reviewed: benchmark.review_status == "reviewed"
            && benchmark
                .reviewer
                .as_ref()
                .is_some_and(|reviewer| !reviewer.trim().is_empty()),
        queries: benchmark.queries.len(),
        precision_at_3: 0.0,
        recall_at_3: 0.0,
        mean_reciprocal_rank: 0.0,
        baseline_recall_at_3: 0.0,
        correct_abstentions: 0,
        coverage: 0.0,
        abstention_rate: 0.0,
        contradiction_violations: 0,
        p50_micros: 0,
        p95_micros: 0,
        quality_gate: false,
        failures: Vec::new(),
    };
    let mut times = Vec::new();
    let mut positive_queries = 0;
    let mut retrieved_queries = 0;
    for query in &benchmark.queries {
        anyhow::ensure!(
            query
                .relevant
                .iter()
                .chain(&query.forbidden)
                .all(|id| ids.contains(id.as_str())),
            "{} references unknown incident IDs",
            query.id
        );
        let start = std::time::Instant::now();
        let (matched, _) = match_incidents(&benchmark.incidents, &query.query, &query.features);
        times.push(start.elapsed().as_micros());
        let top: Vec<_> = matched
            .iter()
            .take(3)
            .map(|incident| incident.id.as_str())
            .collect();
        let hits = top
            .iter()
            .filter(|id| query.relevant.iter().any(|relevant| relevant == **id))
            .count();
        if !top.is_empty() {
            retrieved_queries += 1;
            report.precision_at_3 += hits as f64 / top.len() as f64;
        }
        if query.relevant.is_empty() {
            if top.is_empty() {
                report.correct_abstentions += 1;
            } else {
                report.failures.push(format!("{} should abstain", query.id));
            }
        } else {
            positive_queries += 1;
            report.recall_at_3 += hits as f64 / query.relevant.len() as f64;
            report.mean_reciprocal_rank += top
                .iter()
                .position(|id| query.relevant.iter().any(|relevant| relevant == *id))
                .map(|position| 1.0 / (position + 1) as f64)
                .unwrap_or_default();
            if hits < query.relevant.len().min(3) {
                report
                    .failures
                    .push(format!("{} missed relevant incidents", query.id));
            }
            // Preserve the previous all-query-terms-in-case-text implementation as a frozen baseline.
            let words: BTreeSet<_> = query
                .query
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| word.chars().count() > 1)
                .map(str::to_ascii_lowercase)
                .collect();
            let baseline: Vec<_> = benchmark
                .incidents
                .iter()
                .filter(|incident| {
                    let text = format!(
                        "{} {} {} {} {}",
                        incident.summary,
                        incident.resolution,
                        incident.product.as_deref().unwrap_or_default(),
                        incident.failure_domain.as_deref().unwrap_or_default(),
                        incident.symptom_tags.join(" ")
                    )
                    .to_ascii_lowercase();
                    !words.is_empty() && words.iter().all(|word| text.contains(word))
                })
                .take(3)
                .collect();
            report.baseline_recall_at_3 += baseline
                .iter()
                .filter(|incident| query.relevant.contains(&incident.id))
                .count() as f64
                / query.relevant.len() as f64;
        }
        let violations = matched
            .iter()
            .filter(|incident| query.forbidden.contains(&incident.id))
            .count();
        report.contradiction_violations += violations;
        if violations > 0 {
            report
                .failures
                .push(format!("{} returned forbidden incidents", query.id));
        }
    }
    report.coverage = retrieved_queries as f64 / benchmark.queries.len() as f64;
    report.abstention_rate = 1.0 - report.coverage;
    if benchmark.review_status == "reviewed" {
        report.reviewed &= benchmark
            .queries
            .iter()
            .all(|q| q.reviewer.as_ref().is_some_and(|r| !r.trim().is_empty()));
    }
    report.precision_at_3 /= retrieved_queries.max(1) as f64;
    report.recall_at_3 /= positive_queries.max(1) as f64;
    report.mean_reciprocal_rank /= positive_queries.max(1) as f64;
    report.baseline_recall_at_3 /= positive_queries.max(1) as f64;
    times.sort();
    report.p50_micros = times[times.len() / 2];
    report.p95_micros = times[((times.len() * 95).div_ceil(100) - 1).min(times.len() - 1)];
    report.quality_gate = report.failures.is_empty()
        && report.contradiction_violations == 0
        && report.precision_at_3 >= 0.90
        && report.recall_at_3 >= 0.80
        && report.recall_at_3 >= report.baseline_recall_at_3;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labeled_queries_gate_relevance_abstention_and_contradictions() {
        let report = evaluate_benchmark(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/incident-match-benchmark.json"),
        )
        .unwrap();
        assert!(report.quality_gate, "{report:?}");
        assert_eq!(report.contradiction_violations, 0);
        assert!(
            !report.reviewed,
            "engineering fixtures must not impersonate domain review"
        );
    }
}
