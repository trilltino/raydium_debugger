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

/// Builds the production-style app: API routes plus static React assets.
pub fn router() -> anyhow::Result<Router> {
    let state = Arc::new(AppState::from_env()?);
    Ok(router_with_state(state))
}

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
    use crate::{state::AppState, store::SignatureStore};
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
        let state = Arc::new(AppState {
            api_token: "test-token".to_string(),
            debug_limit: Arc::new(Semaphore::new(4)),
            store: SignatureStore::from_path(path).unwrap(),
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
}
