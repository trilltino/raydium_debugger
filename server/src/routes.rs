//! HTTP route handlers for debugging, AI, provider status, and casebooks.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use futures_util::stream;
use raydium_debugger::{
    run_ai_request, run_debug_request_blocking, run_diagnostic_request_blocking, AiAskRequest,
    DebugRequest,
};
use serde_json::json;
use std::{convert::Infallible, sync::Arc};

use crate::{
    error::{error_kind_response, error_response},
    investigation::{InvestigationRequest, ProgressSubscription, ServiceError},
    state::AppState,
    store::{
        CreateCasebookRequest, CreateIntegratorRequest, SaveSignatureRequest,
        UpdateSavedSignatureRequest,
    },
};

/// Runs a deterministic investigation and streams durable progress followed by its result.
pub async fn investigate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<InvestigationRequest>,
) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    match state.investigation_service.start_subscribed(request).await {
        Ok(subscription) => investigation_stream(subscription),
        Err(error) => service_error(error),
    }
}

fn service_error(error: ServiceError) -> Response {
    let (status, kind) = match &error {
        ServiceError::Input(_) => (StatusCode::BAD_REQUEST, "investigation_input"),
        ServiceError::Capacity(_) => (StatusCode::TOO_MANY_REQUESTS, "capacity"),
        ServiceError::NotFound => (StatusCode::NOT_FOUND, "investigation_not_found"),
        ServiceError::ShuttingDown => (StatusCode::SERVICE_UNAVAILABLE, "shutting_down"),
        ServiceError::Storage => (StatusCode::SERVICE_UNAVAILABLE, "investigation_store"),
    };
    error_kind_response(status, kind, error.to_string())
}

fn investigation_stream(subscription: ProgressSubscription) -> Response {
    let event_stream = stream::unfold(subscription, |mut subscription| async move {
        match subscription.next().await {
            Ok(Some(event)) => {
                let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".into());
                Some((
                    Ok::<_, Infallible>(
                        Event::default()
                            .id(&event.event_id)
                            .event(&event.event_type)
                            .data(data),
                    ),
                    subscription,
                ))
            }
            Ok(None) | Err(_) => None,
        }
    });
    Sse::new(event_stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Authenticated durable replay; an explicit query cursor overrides Last-Event-ID.
pub async fn investigation_events(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<EventCursor>,
) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    let cursor = query
        .after
        .as_deref()
        .or_else(|| headers.get("last-event-id").and_then(|v| v.to_str().ok()))
        .unwrap_or("0");
    let after = match cursor.parse::<u64>() {
        Ok(value) if value <= i64::MAX as u64 => value,
        _ => return service_error(ServiceError::Input("invalid event cursor".into())),
    };
    match state.investigation_service.subscribe(id, after).await {
        Ok(subscription) => investigation_stream(subscription),
        Err(error) => service_error(error),
    }
}

/// Replay query names preserve SSE cursor spelling.
#[derive(serde::Deserialize)]
pub struct EventCursor {
    /// Explicit monotonic event cursor; takes precedence over the header.
    pub after: Option<String>,
}

/// Retrieves the committed result without repeating provider work.
pub async fn investigation_result(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    match state.investigation_service.lookup(id).await {
        Ok(Some(result)) => Json(json!(result)).into_response(),
        Ok(None) => service_error(ServiceError::NotFound),
        Err(error) => service_error(error),
    }
}

/// Explicit retry creates a new run and observation time.
pub async fn investigation_retry(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    match state.investigation_service.retry(id).await {
        Ok(subscription) => investigation_stream(subscription),
        Err(error) => service_error(error),
    }
}

/// Authenticated readiness includes availability of optional evidence sources.
pub async fn readiness(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    let service = state.investigation_service.clone();
    match service.lookup("readiness-probe".into()).await {
        Ok(_) => Json(json!({"ready": !service.metrics()["draining"].as_bool().unwrap_or(true), "knowledge_available": service.paths.knowledge.is_file(), "observations_available": service.paths.observations.is_file(), "providers": raydium_investigation::provider_status()})).into_response(),
        Err(error) => service_error(error),
    }
}

/// Authenticated non-sensitive operational counters.
pub async fn metrics(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    Json(state.investigation_service.operational_metrics().await).into_response()
}

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
    Json(raydium_debugger_server::provider_status())
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

/// Runs the v2 diagnosis path that can represent both landed and non-observed signatures.
pub async fn diagnose(
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

    let result =
        tokio::task::spawn_blocking(move || run_diagnostic_request_blocking(request)).await;
    match result {
        Ok(Ok(response)) => (StatusCode::OK, Json(json!(response))).into_response(),
        Ok(Err(error)) => error_response(StatusCode::BAD_REQUEST, error),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "diagnose_failed",
            format!("diagnosis worker failed: {error}"),
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

pub(crate) fn validate_api_token(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<axum::response::Response> {
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

#[derive(serde::Deserialize)]
pub struct RecentGroupsQuery {
    cluster: Option<raydium_debugger::RpcCluster>,
}

pub async fn recent_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<RecentGroupsQuery>,
) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    let database = state.observations_database_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::investigation::recent_observation_groups(&database, query.cluster, None)
    })
    .await;
    match result {
        Ok(Ok(groups)) => Json(json!(groups)).into_response(),
        Ok(Err(_)) | Err(_) => error_kind_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "observations_store",
            "Recent observations could not be read",
        ),
    }
}
