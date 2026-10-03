//! Generation validation and normalized retrieval documents. Reload failures retain the last valid Arc.
use crate::{
    matching::{self, MatchAssessment, MatchFeatures},
    CuratedIncident,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::SystemTime,
};

/// Validated immutable generation. Only selected results are cloned during matching.
#[derive(Default)]
pub struct CompiledRegistry {
    incidents: Vec<CuratedIncident>,
    documents: Vec<BTreeSet<String>>,
}

impl CompiledRegistry {
    /// Validates IDs, predicates and validity periods, and normalizes each document once.
    pub fn new(incidents: Vec<CuratedIncident>) -> anyhow::Result<Self> {
        let mut ids = BTreeSet::new();
        for incident in &incidents {
            anyhow::ensure!(
                !incident.id.trim().is_empty() && ids.insert(&incident.id),
                "missing or duplicate incident ID"
            );
            anyhow::ensure!(
                !matches!((incident.valid_from,incident.valid_until),(Some(start),Some(end)) if start>end),
                "invalid incident validity period"
            );
            for predicate in incident
                .must
                .iter()
                .chain(&incident.should)
                .chain(&incident.must_not)
            {
                predicate.validate()?;
            }
        }
        let documents = incidents.iter().map(matching::document_terms).collect();
        Ok(Self {
            incidents,
            documents,
        })
    }

    /// Borrows sanitized contracts from this generation.
    pub fn incidents(&self) -> &[CuratedIncident] {
        &self.incidents
    }

    /// Returns bounded deterministic matches and contradiction assessments.
    pub fn matches(
        &self,
        query: &str,
        features: &MatchFeatures,
    ) -> (Vec<CuratedIncident>, Vec<MatchAssessment>) {
        matching::match_prepared(&self.incidents, &self.documents, query, features)
    }

    /// Loads only a sanitized JSON artifact; absent artifacts are explicitly unavailable.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        #[derive(serde::Deserialize)]
        struct Artifact {
            incidents: Vec<CuratedIncident>,
        }
        anyhow::ensure!(
            std::fs::metadata(path)?.len() <= 16 * 1024 * 1024,
            "knowledge artifact exceeds 16 MiB"
        );
        let artifact: Artifact = serde_json::from_slice(&std::fs::read(path)?)?;
        Self::new(artifact.incidents)
    }
}

struct Generation {
    marker: Option<(SystemTime, u64)>,
    registry: Arc<CompiledRegistry>,
    available: bool,
}

/// Shared last-valid generation cache. Loading is blocking and must run off executor threads.
pub struct RegistryCache {
    path: PathBuf,
    generation: RwLock<Generation>,
}

impl RegistryCache {
    /// Starts with an empty unavailable generation; the first snapshot loads the artifact.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            generation: RwLock::new(Generation {
                marker: None,
                registry: Arc::new(CompiledRegistry::default()),
                available: false,
            }),
        }
    }

    /// Shares a normalized generation. `available` is false if a reload failed; the
    /// previous valid generation remains usable and is never replaced by malformed JSON.
    pub fn snapshot(&self) -> (Arc<CompiledRegistry>, bool) {
        let marker = std::fs::metadata(&self.path)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        let mut generation = self
            .generation
            .write()
            .expect("registry generation lock poisoned");
        if marker != generation.marker {
            match CompiledRegistry::load(&self.path) {
                Ok(registry) => {
                    generation.registry = Arc::new(registry);
                    generation.marker = marker;
                    generation.available = true;
                }
                Err(_) => {
                    generation.marker = marker;
                    generation.available = false;
                }
            }
        }
        (generation.registry.clone(), generation.available)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_reload_retains_previous_generation() {
        let path = std::env::temp_dir().join(format!(
            "registry-cache-{}.json",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, r#"{"incidents":[]}"#).unwrap();
        let cache = RegistryCache::new(path.clone());
        let (first, available) = cache.snapshot();
        assert!(available);
        std::fs::write(&path, "malformed").unwrap();
        let (retained, available) = cache.snapshot();
        assert!(!available);
        assert!(Arc::ptr_eq(&first, &retained));
        std::fs::remove_file(path).unwrap();
    }
}
