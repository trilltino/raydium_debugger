//! HTTP route handlers for debugging, AI, provider status, and casebooks.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use raydium_debugger::{
    redact_url, run_ai_request, run_debug_request_blocking, AiAskRequest, DebugRequest,
};
use serde_json::json;
use std::sync::Arc;

use crate::{
    error::{error_kind_response, error_response},
    state::AppState,
    store::{
        CreateCasebookRequest, CreateIntegratorRequest, SaveSignatureRequest,
        UpdateSavedSignatureRequest,
    },
};

/// Health endpoint used by dev tooling and Playwright smoke tests.
pub async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true }))
}

/// Returns non-secret session data used by the same-origin React app.
pub async fn session(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({
        "api_token": state.api_token,
        "runtime": "axum",
    }))
}

/// Returns redacted Triton provider capabilities.
pub async fn providers() -> Json<serde_json::Value> {
    Json(json!({
        "name": "triton_one",
        "triton": {
            "devnet_rpc": std::env::var("TRITON_DEVNET_RPC_URL").ok().map(|url| redact_url(&url)),
            "mainnet_rpc": std::env::var("TRITON_MAINNET_RPC_URL").ok().map(|url| redact_url(&url)),
            "devnet_fallback_rpc": std::env::var("TRITON_DEVNET_FALLBACK_RPC_URL").ok().map(|url| redact_url(&url)),
            "mainnet_fallback_rpc": std::env::var("TRITON_MAINNET_FALLBACK_RPC_URL").ok().map(|url| redact_url(&url)),
            "devnet_configured": std::env::var("TRITON_DEVNET_RPC_URL").is_ok_and(|url| !url.trim().is_empty()),
            "mainnet_configured": std::env::var("TRITON_MAINNET_RPC_URL").is_ok_and(|url| !url.trim().is_empty()),
            "devnet_grpc_available": std::env::var("TRITON_DEVNET_GRPC_URL").is_ok(),
            "mainnet_grpc_available": std::env::var("TRITON_MAINNET_GRPC_URL").is_ok(),
        }
    }))
}

/// Lists saved integrators and their indexed transaction signatures.
pub async fn integrators(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }

    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || store.list()).await;
    match result {
        Ok(Ok(integrators)) => (StatusCode::OK, Json(json!(integrators))).into_response(),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "integrator_store",
            format!("integrator store worker failed: {error}"),
        ),
        Ok(Err(error)) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "integrator_store",
            error.to_string(),
        ),
    }
}

/// Creates an integrator bucket in the local signature library.
pub async fn create_integrator(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateIntegratorRequest>,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }

    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || store.create_integrator(request)).await;
    match result {
        Ok(Ok(integrator)) => (StatusCode::CREATED, Json(json!(integrator))).into_response(),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "integrator_store",
            format!("integrator store worker failed: {error}"),
        ),
        Ok(Err(error)) => error_kind_response(
            StatusCode::BAD_REQUEST,
            "integrator_store",
            error.to_string(),
        ),
    }
}

/// Saves or updates a transaction signature for an integrator.
pub async fn save_integrator_signature(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SaveSignatureRequest>,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }

    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || store.save_signature(&id, request)).await;
    match result {
        Ok(Ok(integrator)) => (StatusCode::OK, Json(json!(integrator))).into_response(),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "integrator_store",
            format!("integrator store worker failed: {error}"),
        ),
        Ok(Err(error)) => error_kind_response(
            StatusCode::BAD_REQUEST,
            "integrator_store",
            error.to_string(),
        ),
    }
}

/// Lists casebooks for an integrator.
pub async fn casebooks(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }

    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || store.list_casebooks(&id)).await;
    match result {
        Ok(Ok(casebooks)) => (StatusCode::OK, Json(json!(casebooks))).into_response(),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "integrator_store",
            format!("integrator store worker failed: {error}"),
        ),
        Ok(Err(error)) => error_kind_response(
            StatusCode::BAD_REQUEST,
            "integrator_store",
            error.to_string(),
        ),
    }
}

/// Creates a casebook for an integrator.
pub async fn create_casebook(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateCasebookRequest>,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }

    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || store.create_casebook(&id, request)).await;
    match result {
        Ok(Ok(casebook)) => (StatusCode::CREATED, Json(json!(casebook))).into_response(),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "integrator_store",
            format!("integrator store worker failed: {error}"),
        ),
        Ok(Err(error)) => error_kind_response(
            StatusCode::BAD_REQUEST,
            "integrator_store",
            error.to_string(),
        ),
    }
}

/// Loads one casebook and its signatures.
pub async fn casebook(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }

    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || store.get_casebook(&id)).await;
    match result {
        Ok(Ok(casebook)) => (StatusCode::OK, Json(json!(casebook))).into_response(),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "integrator_store",
            format!("integrator store worker failed: {error}"),
        ),
        Ok(Err(error)) => error_kind_response(
            StatusCode::BAD_REQUEST,
            "integrator_store",
            error.to_string(),
        ),
    }
}

/// Saves or updates a transaction signature in a casebook.
pub async fn save_casebook_signature(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SaveSignatureRequest>,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }

    let store = state.store.clone();
    let result =
        tokio::task::spawn_blocking(move || store.save_casebook_signature(&id, request)).await;
    match result {
        Ok(Ok(casebook)) => (StatusCode::OK, Json(json!(casebook))).into_response(),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "integrator_store",
            format!("integrator store worker failed: {error}"),
        ),
        Ok(Err(error)) => error_kind_response(
            StatusCode::BAD_REQUEST,
            "integrator_store",
            error.to_string(),
        ),
    }
}

/// Updates saved signature context inside a casebook.
pub async fn update_casebook_signature(
    Path((id, signature_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<UpdateSavedSignatureRequest>,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }

    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        store.update_casebook_signature(&id, &signature_id, request)
    })
    .await;
    match result {
        Ok(Ok(casebook)) => (StatusCode::OK, Json(json!(casebook))).into_response(),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "integrator_store",
            format!("integrator store worker failed: {error}"),
        ),
        Ok(Err(error)) => error_kind_response(
            StatusCode::BAD_REQUEST,
            "integrator_store",
            error.to_string(),
        ),
    }
}

/// Runs a real transaction debug request through the shared library service.
pub async fn debug(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<DebugRequest>,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    if request
        .rpc_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty())
    {
        return error_kind_response(
            StatusCode::BAD_REQUEST,
            "rpc_override_disabled",
            "RPC overrides are disabled; this app always uses the configured Triton endpoint for the selected cluster",
        );
    }

    let mut request = request;
    request.rpc_url = None;
    if request.no_fallback {
        request.no_fallback = false;
    }

    let Ok(_permit) = state.debug_limit.clone().try_acquire_owned() else {
        return error_kind_response(
            StatusCode::TOO_MANY_REQUESTS,
            "busy",
            "too many debug requests are already running",
        );
    };

    let result = tokio::task::spawn_blocking(move || run_debug_request_blocking(request)).await;
    match result {
        Ok(Ok(response)) => (StatusCode::OK, Json(json!(response))).into_response(),
        Ok(Err(error)) => error_response(StatusCode::BAD_REQUEST, error),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "debug_failed",
            format!("debug worker failed: {error}"),
        ),
    }
}

/// Answers a user question from a deterministic debug result when AI is enabled.
pub async fn ask(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<AiAskRequest>,
) -> impl IntoResponse {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }

    match run_ai_request(request).await {
        Ok(response) => (StatusCode::OK, Json(json!(response))).into_response(),
        Err(error) => error_response(StatusCode::BAD_REQUEST, error),
    }
}

fn validate_api_token(state: &AppState, headers: &HeaderMap) -> Option<axum::response::Response> {
    let token = headers
        .get("x-raydium-debugger-token")
        .and_then(|value| value.to_str().ok());
    if token != Some(state.api_token.as_str()) {
        return Some(error_kind_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or invalid local API token",
        ));
    }

    None
}
