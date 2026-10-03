//! Axum router assembly for API routes and static React assets.

use axum::{
    http::{header, HeaderValue, Method},
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    services::{ServeDir, ServeFile},
};

use crate::{routes, state::AppState};

const WEB_DIST: &str = "web/dist";
const INDEX_HTML: &str = "web/dist/index.html";

/// Builds the router around supplied state for tests and alternate launchers.
pub(crate) fn router_with_state(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/session", get(routes::session))
        .route("/api/providers", get(routes::providers))
        .route(
            "/api/integrators",
            get(routes::integrators).post(routes::create_integrator),
        )
        .route(
            "/api/integrators/:id/signatures",
            post(routes::save_integrator_signature),
        )
        .route(
            "/api/integrators/:id/casebooks",
            get(routes::casebooks).post(routes::create_casebook),
        )
        .route("/api/casebooks/:id", get(routes::casebook))
        .route(
            "/api/casebooks/:id/signatures",
            post(routes::save_casebook_signature),
        )
        .route(
            "/api/casebooks/:id/signatures/:signature_id",
            axum::routing::patch(routes::update_casebook_signature),
        )
        .route("/api/debug", post(routes::debug))
        .route("/api/diagnose", post(routes::diagnose))
        .route(
            "/api/investigate",
            post(routes::investigate).layer(axum::extract::DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/api/observations/groups", get(routes::recent_groups))
        .route("/api/investigations/:id", get(routes::investigation_result))
        .route(
            "/api/investigations/:id/events",
            get(routes::investigation_events),
        )
        .route(
            "/api/investigations/:id/retry",
            post(routes::investigation_retry),
        )
        .route("/api/ready", get(routes::readiness))
        .route("/api/metrics", get(routes::metrics))
        .route("/api/ask", post(routes::ask))
        .fallback_service(ServeDir::new(WEB_DIST).not_found_service(ServeFile::new(INDEX_HTML)))
        .layer(cors())
        .with_state(state)
}

fn cors() -> CorsLayer {
    let origins = [
        "http://127.0.0.1:8787",
        "http://localhost:8787",
        "http://127.0.0.1:5173",
        "http://localhost:5173",
    ]
    .into_iter()
    .map(|origin| origin.parse::<HeaderValue>().expect("valid local origin"))
    .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-raydium-debugger-token"),
        ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{investigation::InvestigationStore, state::AppState, store::SignatureStore};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use std::{path::PathBuf, sync::Arc};
    use tokio::sync::Semaphore;
    use tower::ServiceExt;

    fn test_store_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "raydium-debugger-route-{name}-{}.json",
            uuid::Uuid::new_v4()
        ))
    }

    fn test_app(path: &PathBuf) -> Router {
        test_app_with_evidence_paths(
            path,
            PathBuf::from("target/nonexistent-test-registry.json"),
            PathBuf::from("target/nonexistent-test-support.sqlite"),
        )
    }

    fn test_app_with_evidence_paths(
        path: &PathBuf,
        knowledge_registry_path: PathBuf,
        observations_database_path: PathBuf,
    ) -> Router {
        let investigations =
            InvestigationStore::from_path(path.with_extension("investigations.sqlite")).unwrap();
        let investigation_service = crate::investigation::InvestigationService::with_store(
            crate::investigation::RuntimePaths {
                ledger: path.with_extension("investigations.sqlite"),
                knowledge: knowledge_registry_path.clone(),
                observations: observations_database_path.clone(),
            },
            crate::investigation::RuntimeLimits::default(),
            investigations.clone(),
        );
        let state = Arc::new(AppState {
            api_token: "test-token".to_string(),
            debug_limit: Arc::new(Semaphore::new(4)),
            store: SignatureStore::from_path(path).unwrap(),
            investigation_service,
            observations_database_path,
        });
        router_with_state(state)
    }

    #[tokio::test]
    async fn integrator_routes_require_token() {
        let path = test_store_path("auth");
        let response = test_app(&path)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/integrators")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"Integrator"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn invalid_saved_signature_returns_store_error() {
        let path = test_store_path("invalid-signature");
        let app = test_app(&path);
        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/integrators")
                    .header("content-type", "application/json")
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::from(r#"{"name":"Integrator"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let body = to_bytes(create_response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_str().unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/integrators/{id}/signatures"))
                    .header("content-type", "application/json")
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::from(
                        r#"{"signature":"not-a-signature","cluster":"devnet"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(error["error_kind"], "integrator_store");
        assert!(error["error"]
            .as_str()
            .unwrap()
            .contains("invalid Solana transaction signature"));

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn symptom_only_investigation_streams_progress_and_persists_result() {
        let path = test_store_path("investigation");
        let response = test_app(&path)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/investigate")
                    .header("content-type", "application/json")
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::from(
                        r#"{"symptom":"pool not showing","cluster":"mainnet"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let events = String::from_utf8(body.to_vec()).unwrap();
        assert!(events.contains("accepted"));
        assert!(events.contains("knowledge"));
        assert!(events.contains("complete"));
        assert!(events.contains("No approved historical incident matched"));
        assert!(events.contains("No transaction signature was supplied"));
        let _ = std::fs::remove_file(path.with_extension("investigations.sqlite"));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn investigation_returns_curated_and_recent_evidence_without_raw_logs() {
        let path = test_store_path("investigation-evidence");
        let knowledge_path = path.with_extension("incidents.json");
        let support_path = path.with_extension("support.sqlite");
        std::fs::write(
            &knowledge_path,
            r#"{"schema_version":1,"source_revision":1,"incidents":[{"id":"case-approved","product":"raydium_cpmm","failure_domain":"indexing","summary":"Pool not showing after creation","resolution":"Check indexer refresh and pool account visibility.","symptom_tags":["pool_visibility"],"evidence_message_count":4}]}"#,
        )
        .unwrap();
        let support = rusqlite::Connection::open(&support_path).unwrap();
        support
            .execute_batch(
                "CREATE TABLE recent_observations (
                    source TEXT, cluster TEXT, observed_at INTEGER, slot INTEGER,
                    program_id TEXT, instruction TEXT, error_code TEXT,
                    fingerprint TEXT, logs_text TEXT
                );",
            )
            .unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        support
            .execute(
                "INSERT INTO recent_observations VALUES (
                    'indexer', 'mainnet', ?1, 123, NULL, 'pool_refresh', NULL,
                    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'pool not showing raw private log details'
                )",
                [now],
            )
            .unwrap();
        drop(support);

        let app = test_app_with_evidence_paths(&path, knowledge_path.clone(), support_path.clone());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/investigate")
                    .header("content-type", "application/json")
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::from(
                        r#"{"symptom":"pool not showing","cluster":"mainnet"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let stream_text = String::from_utf8(body.to_vec()).unwrap();
        let complete_data = stream_text
            .lines()
            .find(|line| line.starts_with("data: ") && line.contains("\"event_type\":\"complete\""))
            .unwrap()
            .trim_start_matches("data: ");
        let complete: serde_json::Value = serde_json::from_str(complete_data).unwrap();
        let investigation_id = complete["result"]["investigation_id"].as_str().unwrap();
        assert_eq!(
            complete["result"]["related_incidents"][0]["id"],
            "case-approved"
        );
        assert_eq!(
            complete["result"]["recent_observations"][0]["source"],
            "indexer"
        );
        assert!(complete["result"]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["evidence_type"] == "recent_observation"));
        assert!(!stream_text.contains("raw private log details"));

        let groups = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/observations/groups?cluster=mainnet")
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(groups.status(), StatusCode::OK);
        let groups: serde_json::Value =
            serde_json::from_slice(&to_bytes(groups.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(
            groups[0]["fingerprint"],
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(groups[0].get("logs_text").is_none());
        let group_run = app.clone().oneshot(Request::builder().method("POST").uri("/api/investigate")
            .header("content-type", "application/json").header("x-raydium-debugger-token", "test-token")
            .body(Body::from(r#"{"recent_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","cluster":"mainnet"}"#)).unwrap()).await.unwrap();
        let stream = String::from_utf8(
            to_bytes(group_run.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let complete: serde_json::Value = serde_json::from_str(
            stream
                .lines()
                .find(|line| {
                    line.starts_with("data: ") && line.contains("\"event_type\":\"complete\"")
                })
                .unwrap()
                .trim_start_matches("data: "),
        )
        .unwrap();
        assert!(complete["result"]["transaction_diagnosis"].is_null());
        assert!(complete["result"]["signature"].is_null());
        assert_eq!(
            complete["result"]["recent_observations"][0]["source"],
            "indexer"
        );
        assert!(!stream.contains("raw private log details"));
        let scoped = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/observations/groups?cluster=devnet")
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let scoped: serde_json::Value =
            serde_json::from_slice(&to_bytes(scoped.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert!(scoped.as_array().unwrap().is_empty());
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/observations/groups")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let lookup = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/investigations/{investigation_id}"))
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(lookup.status(), StatusCode::OK);
        let body = to_bytes(lookup.into_body(), 1024 * 1024).await.unwrap();
        let lookup: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(lookup["result"]["investigation_id"], investigation_id);

        let investigation_store_path = path.with_extension("investigations.sqlite");
        let investigation_store = rusqlite::Connection::open(&investigation_store_path).unwrap();
        let (status, result_json): (String, String) = investigation_store
            .query_row(
                "SELECT status, result_json FROM investigations ORDER BY rowid LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "complete");
        assert!(result_json.contains("case-approved"));
        assert!(!result_json.contains("raw private log details"));

        drop(investigation_store);
        let _ = std::fs::remove_file(investigation_store_path);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(knowledge_path);
        let _ = std::fs::remove_file(support_path);
    }
    #[tokio::test]
    async fn replay_requires_auth_and_query_cursor_overrides_header() {
        let path = test_store_path("replay");
        let app = test_app(&path);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/investigate")
                    .header("content-type", "application/json")
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::from(
                        r#"{"symptom":"pool visibility","cluster":"devnet"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let text = String::from_utf8(
            to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let events: Vec<serde_json::Value> = text
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(events.len() >= 3);
        let id = events[0]["investigation_id"].as_str().unwrap();
        assert_eq!(
            text.lines().filter(|line| line.starts_with("id:")).count(),
            events.len()
        );
        let first = events[0]["event_id"].as_str().unwrap();
        let last = events.last().unwrap()["event_id"].as_str().unwrap();
        let unauthenticated = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/investigations/{id}/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        let replay = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/investigations/{id}/events?after={first}"))
                    .header("last-event-id", last)
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let text = String::from_utf8(
            to_bytes(replay.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert_eq!(
            text.lines().filter(|line| line.starts_with("id:")).count(),
            events.len() - 1
        );
        let invalid = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/investigations/{id}/events?after=invalid"))
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        let acknowledged = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/investigations/{id}/events"))
                    .header("last-event-id", last)
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(to_bytes(acknowledged.into_body(), 1024)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn request_limit_and_monitoring_authentication_are_enforced() {
        let path = test_store_path("limits");
        let app = test_app(&path);
        let oversized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/investigate")
                    .header("content-type", "application/json")
                    .header("x-raydium-debugger-token", "test-token")
                    .body(Body::from(vec![b' '; 64 * 1024 + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
        for endpoint in ["/api/ready", "/api/metrics"] {
            let unauth = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(endpoint)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
            let auth = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(endpoint)
                        .header("x-raydium-debugger-token", "test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(auth.status(), StatusCode::OK);
        }
    }
}
