//! Transport-independent lifecycle. Blocking workers own permits until they exit,
//! including after a deadline; subscribers consume committed SQLite events.
use super::*;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Durable lifecycle status; wire names remain compatible with existing results.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvestigationStatus {
    /// Work is executing.
    Running,
    /// Evidence assembly completed.
    Complete,
    /// Assembly completed with missing evidence.
    Partial,
    /// Work failed explicitly.
    Failed,
    /// Process ended before completion; retry requires new work.
    Interrupted,
    /// Deadline elapsed; the blocking worker may still hold capacity.
    TimedOut,
}
impl InvestigationStatus {
    /// Stable persistence and transport spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::TimedOut => "timed_out",
        }
    }
}

/// Typed service failure. Transports map these to HTTP or IPC at their boundary.
#[derive(Debug)]
pub enum ServiceError {
    /// Invalid bounded request or replay cursor.
    Input(String),
    /// Immediate capacity rejection; no work is queued implicitly.
    Capacity(&'static str),
    /// Unknown or pruned investigation.
    NotFound,
    /// Shared service is draining.
    ShuttingDown,
    /// Internal failure with private details excluded.
    Storage,
}
impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(message) => f.write_str(message),
            Self::Capacity(resource) => write!(f, "capacity exhausted: {resource}"),
            Self::NotFound => f.write_str("investigation was not found"),
            Self::ShuttingDown => f.write_str("investigation service is shutting down"),
            Self::Storage => f.write_str("investigation storage unavailable"),
        }
    }
}
impl std::error::Error for ServiceError {}

/// Runtime paths. No private archive path exists in this configuration.
#[derive(Debug, Clone)]
pub struct RuntimePaths {
    /// Isolated investigation history.
    pub ledger: PathBuf,
    /// Sanitized incident artifact.
    pub knowledge: PathBuf,
    /// Isolated operational observations.
    pub observations: PathBuf,
}
impl RuntimePaths {
    /// Resolves explicit overrides against a shell-specific application data directory.
    pub fn from_env(base: &Path) -> Self {
        let path = |name: &str, fallback: PathBuf| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .unwrap_or(fallback)
        };
        Self {
            ledger: path(
                "RAYDIUM_DEBUGGER_INVESTIGATION_PATH",
                base.join("investigations.sqlite"),
            ),
            knowledge: path(
                "RAYDIUM_DEBUGGER_KNOWLEDGE_PATH",
                PathBuf::from("knowledge/incidents.generated.json"),
            ),
            observations: path(
                "RAYDIUM_DEBUGGER_OBSERVATIONS_DATABASE_PATH",
                base.join("observations.sqlite"),
            ),
        }
    }
}

/// Bounded operator workload configuration; all durations use elapsed seconds.
#[derive(Debug, Clone)]
pub struct RuntimeLimits {
    /// Concurrent investigations, including timed-out workers still executing.
    pub investigations: usize,
    /// Concurrent RPC workers.
    pub rpc_jobs: usize,
    /// Concurrent progress subscribers across both shells.
    pub subscribers: usize,
    /// Investigation deadline, measured from acceptance.
    pub deadline: Duration,
}
impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            investigations: 8,
            rpc_jobs: 4,
            subscribers: 64,
            deadline: Duration::from_secs(90),
        }
    }
}

#[derive(Default)]
struct Metrics {
    rejected: AtomicU64,
    completed: AtomicU64,
    provider_failures: AtomicU64,
    storage_failures: AtomicU64,
    duration_millis: AtomicU64,
}

/// Shared service used by Axum and Tauri. Clones share capacity, registry and ledger.
#[derive(Clone)]
pub struct InvestigationService {
    /// Durable ledger; all direct operations require a blocking thread.
    pub store: InvestigationStore,
    /// Runtime paths exclude private support storage.
    pub paths: RuntimePaths,
    limits: RuntimeLimits,
    registry: Arc<raydium_knowledge::RegistryCache>,
    active: Arc<Semaphore>,
    rpc: Arc<Semaphore>,
    subscribers: Arc<Semaphore>,
    draining: Arc<AtomicBool>,
    metrics: Arc<Metrics>,
}

impl InvestigationService {
    /// Initializes once per process on a blocking thread. Restart recovery runs here.
    pub fn new(paths: RuntimePaths, limits: RuntimeLimits) -> anyhow::Result<Self> {
        anyhow::ensure!(
            limits.investigations > 0
                && limits.rpc_jobs > 0
                && limits.subscribers > 0
                && !limits.deadline.is_zero(),
            "runtime limits must be positive"
        );
        anyhow::ensure!(
            paths.ledger != paths.observations,
            "ledger and observations require separate paths"
        );
        let store = InvestigationStore::from_path(paths.ledger.clone())?;
        Ok(Self::with_store(paths, limits, store))
    }

    /// Uses an already initialized ledger; useful for isolated shell tests.
    pub fn with_store(
        paths: RuntimePaths,
        limits: RuntimeLimits,
        store: InvestigationStore,
    ) -> Self {
        Self {
            registry: Arc::new(raydium_knowledge::RegistryCache::new(
                paths.knowledge.clone(),
            )),
            active: Arc::new(Semaphore::new(limits.investigations)),
            rpc: Arc::new(Semaphore::new(limits.rpc_jobs)),
            subscribers: Arc::new(Semaphore::new(limits.subscribers)),
            paths,
            limits,
            store,
            draining: Arc::new(AtomicBool::new(false)),
            metrics: Arc::new(Metrics::default()),
        }
    }

    /// Shares RPC capacity with legacy diagnostic entry points in the same shell.
    pub fn rpc_capacity(&self) -> Arc<Semaphore> {
        self.rpc.clone()
    }

    fn acquire(
        &self,
        semaphore: &Arc<Semaphore>,
        resource: &'static str,
    ) -> Result<OwnedSemaphorePermit, ServiceError> {
        semaphore.clone().try_acquire_owned().map_err(|_| {
            self.metrics.rejected.fetch_add(1, Ordering::Relaxed);
            ServiceError::Capacity(resource)
        })
    }

    /// Starts a validated investigation without requiring any connected UI.
    /// Excess work is rejected immediately; a disconnected stream never cancels work.
    pub async fn start(&self, mut request: InvestigationRequest) -> Result<String, ServiceError> {
        if self.draining.load(Ordering::Acquire) {
            return Err(ServiceError::ShuttingDown);
        }
        request.signature = request
            .signature
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        request.symptom = request
            .symptom
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        validate_request(&request).map_err(|e| ServiceError::Input(e.to_string()))?;
        if serde_json::to_vec(&request)
            .map_err(|_| ServiceError::Storage)?
            .len()
            > 64 * 1024
        {
            return Err(ServiceError::Input(
                "investigation request exceeds 64 KiB".into(),
            ));
        }
        let active = self.acquire(&self.active, "investigations")?;
        let rpc = if request.signature.is_some() {
            Some(self.acquire(&self.rpc, "rpc_jobs")?)
        } else {
            None
        };
        let started = Instant::now();
        let id = uuid::Uuid::new_v4().to_string();
        let store = self.store.clone();
        let accepted_id = id.clone();
        let accepted_request = request.clone();
        tokio::task::spawn_blocking(move || store.begin(&accepted_id, &accepted_request))
            .await
            .map_err(|_| ServiceError::Storage)?
            .map_err(storage_error)?;
        let service = self.clone();
        let worker_id = id.clone();
        let observer_id = id.clone();
        let deadline = self.limits.deadline;
        let active = Arc::new(active);
        let terminal_lease = active.clone();
        let worker = tokio::task::spawn_blocking(move || {
            // Permits are inside the blocking closure, so timing out the join does
            // not release capacity until the unabortable RPC call really exits.
            let _active = active;
            let _rpc = rpc;
            let (registry, available) = service.registry.snapshot();
            let store = service.store.clone();
            let sink = |event: InvestigationEvent| store.record_event(&event);
            let mut result = execute_investigation(
                &worker_id,
                &request,
                &registry,
                &service.paths.observations,
                &sink,
            )?;
            if !available {
                result.unknowns.push("Sanitized knowledge artifact unavailable; the last valid generation, if any, was retained.".into());
            }
            if !service.paths.observations.is_file() {
                result
                    .unknowns
                    .push("Operational observation storage is unavailable.".into());
            }
            if result.transaction_error.is_some() {
                service
                    .metrics
                    .provider_failures
                    .fetch_add(1, Ordering::Relaxed);
            }
            Ok::<_, anyhow::Error>(result)
        });
        let service = self.clone();
        tokio::spawn(async move {
            let _terminal_lease = terminal_lease;
            let outcome =
                tokio::time::timeout(deadline.saturating_sub(started.elapsed()), worker).await;
            let store = service.store.clone();
            let committed = tokio::task::spawn_blocking(move || match outcome {
                Ok(Ok(Ok(result))) => store.complete(&result),
                Err(_) => store.terminate(&observer_id, InvestigationStatus::TimedOut),
                _ => store.fail(&observer_id),
            })
            .await;
            if !matches!(committed, Ok(Ok(()))) {
                service
                    .metrics
                    .storage_failures
                    .fetch_add(1, Ordering::Relaxed);
                tracing::error!(
                    component = "ledger",
                    failure = "terminal_commit",
                    "investigation persistence failed"
                );
            }
            service.metrics.completed.fetch_add(1, Ordering::Relaxed);
            service.metrics.duration_millis.fetch_add(
                started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                Ordering::Relaxed,
            );
            tracing::info!(
                component = "investigation",
                duration_ms = started.elapsed().as_millis() as u64,
                "investigation finished"
            );
        });
        Ok(id)
    }

    /// Reserves a stream before accepting work, preventing orphan acceptance on subscriber overload.
    pub async fn start_subscribed(
        &self,
        request: InvestigationRequest,
    ) -> Result<ProgressSubscription, ServiceError> {
        let permit = self.acquire(&self.subscribers, "stream_subscribers")?;
        let id = self.start(request).await?;
        self.subscribe_reserved(id, 0, permit).await
    }

    /// Authenticated shells call this for replay. Subscribe first, then read persisted batches.
    pub async fn subscribe(
        &self,
        id: String,
        after: u64,
    ) -> Result<ProgressSubscription, ServiceError> {
        let permit = self.acquire(&self.subscribers, "stream_subscribers")?;
        self.subscribe_reserved(id, after, permit).await
    }

    async fn subscribe_reserved(
        &self,
        id: String,
        after: u64,
        permit: OwnedSemaphorePermit,
    ) -> Result<ProgressSubscription, ServiceError> {
        if after > i64::MAX as u64 {
            return Err(ServiceError::Input("invalid event cursor".into()));
        }
        let notifications = self.store.subscribe();
        if self.lookup(id.clone()).await?.is_none() {
            return Err(ServiceError::NotFound);
        }
        Ok(ProgressSubscription {
            investigation_id: id,
            store: self.store.clone(),
            after,
            notifications,
            buffer: VecDeque::new(),
            terminal: false,
            _permit: permit,
        })
    }

    /// Reads committed results off the executor thread.
    pub async fn lookup(&self, id: String) -> Result<Option<InvestigationLookup>, ServiceError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.lookup(&id))
            .await
            .map_err(|_| ServiceError::Storage)?
            .map_err(|_| ServiceError::Storage)
    }

    /// Replays one bounded batch through the same ledger for native IPC.
    pub async fn events_after(
        &self,
        id: String,
        after: u64,
    ) -> Result<Vec<InvestigationEvent>, ServiceError> {
        if after > i64::MAX as u64 {
            return Err(ServiceError::Input("invalid event cursor".into()));
        }
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.events_after(&id, after))
            .await
            .map_err(|_| ServiceError::Storage)?
            .map_err(|_| ServiceError::Storage)
    }

    /// Explicitly retries a terminal investigation using a fresh ID and acceptance time.
    pub async fn retry(&self, id: String) -> Result<ProgressSubscription, ServiceError> {
        let previous = self
            .lookup(id.clone())
            .await?
            .ok_or(ServiceError::NotFound)?;
        if previous.status == "running" {
            return Err(ServiceError::Input(
                "running investigations cannot be retried".into(),
            ));
        }
        let store = self.store.clone();
        let request = tokio::task::spawn_blocking(move || store.request(&id))
            .await
            .map_err(|_| ServiceError::Storage)?
            .map_err(|_| ServiceError::Storage)?
            .ok_or(ServiceError::NotFound)?;
        self.start_subscribed(request).await
    }

    /// Non-sensitive metrics. No request, signature, provider URL or private text is included.
    pub fn metrics(&self) -> serde_json::Value {
        serde_json::json!({"active_investigations":self.limits.investigations-self.active.available_permits(),"active_rpc_jobs":self.limits.rpc_jobs-self.rpc.available_permits(),"stream_subscribers":self.limits.subscribers-self.subscribers.available_permits(),"rejected_requests":self.metrics.rejected.load(Ordering::Relaxed),"completed_runs":self.metrics.completed.load(Ordering::Relaxed),"duration_millis_total":self.metrics.duration_millis.load(Ordering::Relaxed),"provider_failures":self.metrics.provider_failures.load(Ordering::Relaxed),"storage_failures":self.metrics.storage_failures.load(Ordering::Relaxed),"draining":self.draining.load(Ordering::Acquire)})
    }

    /// Collects aggregate collector health off the executor thread.
    pub async fn operational_metrics(&self) -> serde_json::Value {
        let mut metrics = self.metrics();
        let path = self.paths.observations.clone();
        let health =
            tokio::task::spawn_blocking(move || raydium_observability::observation_health(&path))
                .await;
        metrics["sqlite_contention"] = self.store.contention_count().into();
        metrics["collector"] = match health {
            Ok(Ok(health)) => serde_json::to_value(health).unwrap_or_default(),
            _ => {
                serde_json::json!({"available":false,"checkpoint_age_seconds":null,"collector_lag_seconds":null})
            }
        };
        metrics
    }

    /// Stops acceptance and drains for at most 15 seconds. Remaining durable runs
    /// are marked interrupted on the next process startup, never silently repeated.
    pub async fn shutdown(&self) {
        self.draining.store(true, Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_secs(15), async {
            while self.active.available_permits() != self.limits.investigations {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
    }
}

/// Persisted progress stream with a reserved subscriber slot and bounded backlog buffer.
pub struct ProgressSubscription {
    /// Accepted or replayed run identity.
    pub investigation_id: String,
    store: InvestigationStore,
    after: u64,
    notifications: tokio::sync::broadcast::Receiver<()>,
    buffer: VecDeque<InvestigationEvent>,
    terminal: bool,
    _permit: OwnedSemaphorePermit,
}
impl ProgressSubscription {
    /// Replays committed events in batches of 128, then waits for notification hints.
    /// Broadcast lag is recovered by querying SQLite after the last delivered ID.
    pub async fn next(&mut self) -> Result<Option<InvestigationEvent>, ServiceError> {
        loop {
            if let Some(event) = self.buffer.pop_front() {
                self.after = event.event_id.parse().map_err(|_| ServiceError::Storage)?;
                self.terminal = event.event_type == "complete" || event.event_type == "error";
                return Ok(Some(event));
            }
            if self.terminal {
                return Ok(None);
            }
            let store = self.store.clone();
            let id = self.investigation_id.clone();
            let after = self.after;
            let events = tokio::task::spawn_blocking(move || store.events_after(&id, after))
                .await
                .map_err(|_| ServiceError::Storage)?
                .map_err(|_| ServiceError::Storage)?;
            if !events.is_empty() {
                self.buffer = events.into();
                continue;
            }
            // Reconnecting with the terminal event already acknowledged ends cleanly.
            let store = self.store.clone();
            let id = self.investigation_id.clone();
            let lookup = tokio::task::spawn_blocking(move || store.lookup(&id))
                .await
                .map_err(|_| ServiceError::Storage)?
                .map_err(|_| ServiceError::Storage)?;
            if lookup.is_none_or(|run| run.status != "running") {
                let store = self.store.clone();
                let id = self.investigation_id.clone();
                let after = self.after;
                let final_events =
                    tokio::task::spawn_blocking(move || store.events_after(&id, after))
                        .await
                        .map_err(|_| ServiceError::Storage)?
                        .map_err(|_| ServiceError::Storage)?;
                if final_events.is_empty() {
                    return Ok(None);
                }
                self.buffer = final_events.into();
                continue;
            }
            // Periodic recovery also covers notification races and failed receivers.
            let _ = tokio::time::timeout(Duration::from_secs(1), self.notifications.recv()).await;
        }
    }
}

fn storage_error(error: anyhow::Error) -> ServiceError {
    match error.downcast::<ServiceError>() {
        Ok(error) => error,
        Err(_) => ServiceError::Storage,
    }
}
