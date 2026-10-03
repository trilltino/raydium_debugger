//! Recovery and replay tests use real SQLite and the shared service.
use super::*;

fn paths() -> RuntimePaths {
    let base = std::env::temp_dir().join(format!("investigation-tests-{}", uuid::Uuid::new_v4()));
    RuntimePaths {
        ledger: base.join("ledger.sqlite"),
        observations: base.join("observations.sqlite"),
        knowledge: base.join("knowledge.json"),
    }
}
fn request() -> InvestigationRequest {
    InvestigationRequest {
        signature: None,
        symptom: Some("pool visibility".into()),
        cluster: Some(RpcCluster::Devnet),
        recent_fingerprint: None,
    }
}
fn progress(id: &str) -> InvestigationEvent {
    InvestigationEvent {
        event_id: String::new(),
        event_type: "progress".into(),
        investigation_id: id.into(),
        stage: Some("test".into()),
        message: Some("persisted".into()),
        result: None,
        error: None,
    }
}
fn result(id: &str) -> InvestigationResult {
    InvestigationResult {
        investigation_id: id.into(),
        status: "partial".into(),
        signature: None,
        symptom: Some("pool visibility".into()),
        cluster: Some("devnet".into()),
        transaction_diagnosis: None,
        transaction_error: None,
        related_incidents: vec![],
        incident_matches: vec![],
        recent_observations: vec![],
        evidence: vec![InvestigationEvidence {
            evidence_id: format!("{id}:evidence"),
            evidence_type: "fixture".into(),
            source_reference: "local".into(),
            summary: "bounded evidence".into(),
            observed_at: None,
        }],
        unknowns: vec![],
    }
}

#[tokio::test]
async fn slow_subscriber_recovers_beyond_notification_capacity_without_gaps() {
    let paths = paths();
    let service = InvestigationService::new(paths, RuntimeLimits::default()).unwrap();
    service.store.begin("replay", &request()).unwrap();
    let mut subscription = service.subscribe("replay".into(), 0).await.unwrap();
    let store = service.store.clone();
    tokio::task::spawn_blocking(move || {
        for _ in 0..300 {
            store.record_event(&progress("replay")).unwrap();
        }
        store.complete(&result("replay")).unwrap();
        store.complete(&result("replay")).unwrap();
    })
    .await
    .unwrap();
    let mut ids = vec![];
    let mut terminal = 0;
    while let Some(event) = subscription.next().await.unwrap() {
        ids.push(event.event_id.parse::<u64>().unwrap());
        if event.result.is_some() {
            terminal += 1;
        }
    }
    assert_eq!(ids.len(), 302);
    assert_eq!(terminal, 1);
    assert!(ids.windows(2).all(|pair| pair[1] == pair[0] + 1));
    let lookup = service.lookup("replay".into()).await.unwrap().unwrap();
    assert_eq!(lookup.result.unwrap().evidence.len(), 1);
    let mut acknowledged = service
        .subscribe("replay".into(), *ids.last().unwrap())
        .await
        .unwrap();
    assert!(acknowledged.next().await.unwrap().is_none());
}

#[test]
fn restart_interrupts_once_and_late_completion_cannot_overwrite_terminal() {
    let paths = paths();
    let store = InvestigationStore::from_path(&paths.ledger).unwrap();
    store.begin("unfinished", &request()).unwrap();
    drop(store);
    let recovered = InvestigationStore::from_path(&paths.ledger).unwrap();
    assert_eq!(
        recovered.lookup("unfinished").unwrap().unwrap().status,
        "interrupted"
    );
    recovered.complete(&result("unfinished")).unwrap();
    recovered.fail("unfinished").unwrap();
    assert_eq!(recovered.events_after("unfinished", 0).unwrap().len(), 2);
    assert!(recovered
        .lookup("unfinished")
        .unwrap()
        .unwrap()
        .result
        .is_none());
    drop(recovered);
    let reopened = InvestigationStore::from_path(&paths.ledger).unwrap();
    assert_eq!(reopened.events_after("unfinished", 0).unwrap().len(), 2);
}

#[tokio::test]
async fn explicit_retry_uses_new_identity_and_disconnect_keeps_work_running() {
    let paths = paths();
    let service = InvestigationService::new(paths, RuntimeLimits::default()).unwrap();
    service.store.begin("interrupted", &request()).unwrap();
    service
        .store
        .terminate("interrupted", InvestigationStatus::Interrupted)
        .unwrap();
    let subscription = service.retry("interrupted".into()).await.unwrap();
    let id = subscription.investigation_id.clone();
    assert_ne!(id, "interrupted");
    drop(subscription);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if service
                .lookup(id.clone())
                .await
                .unwrap()
                .unwrap()
                .result
                .is_some()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let events = service.events_after(id.clone(), 0).await.unwrap();
    assert_eq!(
        events.iter().filter(|e| e.event_type == "complete").count(),
        1
    );
}

#[tokio::test]
async fn subscriber_limit_rejects_before_accepting_new_work() {
    let paths = paths();
    let service = InvestigationService::new(
        paths,
        RuntimeLimits {
            subscribers: 1,
            ..RuntimeLimits::default()
        },
    )
    .unwrap();
    service.store.begin("held", &request()).unwrap();
    let held = service.subscribe("held".into(), 0).await.unwrap();
    assert!(matches!(
        service.start_subscribed(request()).await,
        Err(ServiceError::Capacity("stream_subscribers"))
    ));
    assert_eq!(service.metrics()["rejected_requests"], 1);
    drop(held);
}

#[test]
fn timeout_terminal_rejects_late_results_and_progress() {
    let paths = paths();
    let store = InvestigationStore::from_path(paths.ledger).unwrap();
    store.begin("deadline", &request()).unwrap();
    store
        .terminate("deadline", InvestigationStatus::TimedOut)
        .unwrap();
    store.record_event(&progress("deadline")).unwrap();
    store.complete(&result("deadline")).unwrap();
    assert_eq!(
        store.lookup("deadline").unwrap().unwrap().status,
        "timed_out"
    );
    assert_eq!(store.events_after("deadline", 0).unwrap().len(), 2);
}
