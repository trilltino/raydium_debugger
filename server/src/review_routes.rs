//! Authenticated local endpoints for private archive review.
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use raydium_knowledge_builder::{ReviewDecision, ReviewQuery};
use serde_json::json;
use std::sync::Arc;

use crate::{error::error_kind_response, routes::validate_api_token, state::AppState};

pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ReviewQuery>,
) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    let store = state.review_store.clone();
    match tokio::task::spawn_blocking(move || store.list(query)).await {
        Ok(Ok(result)) => Json(json!(result)).into_response(),
        Ok(Err(error)) => review_error(error),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "review_worker",
            error.to_string(),
        ),
    }
}

pub(crate) async fn detail(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    let store = state.review_store.clone();
    match tokio::task::spawn_blocking(move || store.detail(&id)).await {
        Ok(Ok(result)) => Json(json!(result)).into_response(),
        Ok(Err(error)) => review_error(error),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "review_worker",
            error.to_string(),
        ),
    }
}

pub(crate) async fn submit(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(decision): Json<ReviewDecision>,
) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    let store = state.review_store.clone();
    match tokio::task::spawn_blocking(move || store.submit(&id, decision)).await {
        Ok(Ok(result)) => Json(json!(result)).into_response(),
        Ok(Err(error)) => review_error(error),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "review_worker",
            error.to_string(),
        ),
    }
}

pub(crate) async fn media(
    Path((id, revision, index)): Path<(String, i64, usize)>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = validate_api_token(&state, &headers) {
        return response;
    }
    let store = state.review_store.clone();
    match tokio::task::spawn_blocking(move || store.media(&id, revision, index)).await {
        Ok(Ok(media)) => {
            let mut response = media.bytes.into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&media.mime).unwrap(),
            );
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response.headers_mut().insert(
                header::HeaderName::from_static("x-content-type-options"),
                HeaderValue::from_static("nosniff"),
            );
            response
        }
        Ok(Err(error)) => review_error(error),
        Err(error) => error_kind_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "review_worker",
            error.to_string(),
        ),
    }
}

fn review_error(error: anyhow::Error) -> Response {
    let message = error.to_string();
    let status = if message.contains("not found") {
        StatusCode::NOT_FOUND
    } else if message.contains("changed") || message.contains("stale") {
        StatusCode::CONFLICT
    } else if message.contains("cannot open private review file")
        || message.contains("does not exist")
    {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::BAD_REQUEST
    };
    error_kind_response(status, "knowledge_review", message)
}
