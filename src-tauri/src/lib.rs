//! Tauri command bridge over the shared debugger service.

use raydium_investigation::{
    recent_observation_groups, InvestigationEvent, InvestigationLookup, InvestigationRequest,
    InvestigationResult, InvestigationService, ProgressSubscription, RecentObservationSummary,
    RuntimeLimits, RuntimePaths,
};
use tauri::Manager;

use raydium_debugger::{run_debug_request_blocking, AiAskRequest, DebugRequest, DebugResponse};
use raydium_knowledge_builder::{ReviewDecision, ReviewQuery, ReviewStore};

#[tauri::command]
async fn list_corpus_reviews_cmd(
    query: ReviewQuery,
) -> Result<raydium_knowledge_builder::ReviewList, String> {
    tokio::task::spawn_blocking(move || ReviewStore::from_env().list(query))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_corpus_review_cmd(
    id: String,
) -> Result<raydium_knowledge_builder::ReviewDetail, String> {
    tokio::task::spawn_blocking(move || ReviewStore::from_env().detail(&id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_corpus_review_cmd(
    id: String,
    decision: ReviewDecision,
) -> Result<raydium_knowledge_builder::ReviewDetail, String> {
    tokio::task::spawn_blocking(move || ReviewStore::from_env().submit(&id, decision))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_corpus_media_cmd(
    id: String,
    revision: i64,
    index: usize,
) -> Result<raydium_knowledge_builder::ReviewMedia, String> {
    tokio::task::spawn_blocking(move || ReviewStore::from_env().media(&id, revision, index))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn debug_transaction_cmd(request: DebugRequest) -> Result<DebugResponse, String> {
    tokio::task::spawn_blocking(move || run_debug_request_blocking(request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[cfg(feature = "ai")]
#[tauri::command]
async fn ask_ai_cmd(request: AiAskRequest) -> Result<raydium_debugger::ai::AiResponse, String> {
    raydium_debugger::run_ai_request(request)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "ai"))]
#[tauri::command]
async fn ask_ai_cmd(_request: AiAskRequest) -> Result<serde_json::Value, String> {
    Err("AI question requested, but this desktop build does not include the `ai` feature.".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();
    raydium_investigation::init_tracing();
    let mut context = tauri::generate_context!();
    if let Some(base) = std::env::var_os("RAYDIUM_DEBUGGER_APPLICATION_DATA_PATH") {
        context.config_mut().app.windows[0].data_directory =
            Some(std::path::PathBuf::from(base).join("webview"));
    }
    tauri::Builder::default()
        .setup(|app| {
            let base = std::env::var_os("RAYDIUM_DEBUGGER_APPLICATION_DATA_PATH")
                .map(std::path::PathBuf::from)
                .map(Ok)
                .unwrap_or_else(|| app.path().app_data_dir())?;
            let service =
                InvestigationService::new(RuntimePaths::from_env(&base), RuntimeLimits::default())?;
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            debug_transaction_cmd,
            investigate_cmd,
            recent_groups_cmd,
            providers_cmd,
            investigation_lookup_cmd,
            investigation_events_cmd,
            investigation_retry_cmd,
            ask_ai_cmd,
            list_corpus_reviews_cmd,
            get_corpus_review_cmd,
            save_corpus_review_cmd,
            get_corpus_media_cmd
        ])
        .build(context)
        .expect("error while building Tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                let service = app.state::<InvestigationService>().inner().clone();
                if !service.metrics()["draining"].as_bool().unwrap_or(true) {
                    api.prevent_exit();
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        service.shutdown().await;
                        handle.exit(0);
                    });
                }
            }
        });
}

#[tauri::command]
async fn recent_groups_cmd(
    service: tauri::State<'_, InvestigationService>,
    cluster: Option<raydium_debugger::RpcCluster>,
) -> Result<Vec<RecentObservationSummary>, String> {
    let database = service.paths.observations.clone();
    tokio::task::spawn_blocking(move || recent_observation_groups(&database, cluster, None))
        .await
        .map_err(|_| "observation worker unavailable".to_owned())?
        .map_err(|_| "operational observation storage unavailable".to_owned())
}

async fn forward_progress(
    mut subscription: ProgressSubscription,
    on_progress: tauri::ipc::Channel<InvestigationEvent>,
) -> Result<InvestigationResult, String> {
    while let Some(event) = subscription
        .next()
        .await
        .map_err(|error| error.to_string())?
    {
        let _ = on_progress.send(event.clone());
        if let Some(result) = event.result {
            return Ok(result);
        }
        if let Some(error) = event.error {
            return Err(error);
        }
    }
    Err("investigation ended without a result".into())
}

#[tauri::command]
async fn investigate_cmd(
    service: tauri::State<'_, InvestigationService>,
    request: InvestigationRequest,
    on_progress: tauri::ipc::Channel<InvestigationEvent>,
) -> Result<InvestigationResult, String> {
    let subscription = service
        .start_subscribed(request)
        .await
        .map_err(|error| error.to_string())?;
    forward_progress(subscription, on_progress).await
}

#[tauri::command]
async fn investigation_lookup_cmd(
    service: tauri::State<'_, InvestigationService>,
    id: String,
) -> Result<Option<InvestigationLookup>, String> {
    service.lookup(id).await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn investigation_events_cmd(
    service: tauri::State<'_, InvestigationService>,
    id: String,
    after: Option<String>,
) -> Result<Vec<InvestigationEvent>, String> {
    let cursor = after
        .unwrap_or_else(|| "0".into())
        .parse::<u64>()
        .map_err(|_| "invalid event cursor".to_owned())?;
    service
        .events_after(id, cursor)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn investigation_retry_cmd(
    service: tauri::State<'_, InvestigationService>,
    id: String,
    on_progress: tauri::ipc::Channel<InvestigationEvent>,
) -> Result<InvestigationResult, String> {
    let subscription = service.retry(id).await.map_err(|error| error.to_string())?;
    forward_progress(subscription, on_progress).await
}

#[tauri::command]
fn providers_cmd() -> serde_json::Value {
    raydium_investigation::provider_status()
}
