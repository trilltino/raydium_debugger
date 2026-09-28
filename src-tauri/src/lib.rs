//! Tauri command bridge over the shared debugger service.

use raydium_debugger::{run_debug_request_blocking, AiAskRequest, DebugRequest, DebugResponse};

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
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![debug_transaction_cmd, ask_ai_cmd])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
