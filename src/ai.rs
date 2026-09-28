//! Optional AI Q&A support over deterministic transaction debug context.

use anyhow::{anyhow, Context};
use genai::{
    chat::{ChatMessage, ChatRequest},
    Client,
};
use serde::{Deserialize, Serialize};

use crate::debug::TransactionDebugInfo;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Minimal transaction context sent to the configured AI model.
pub struct AiDebugContext {
    pub signature: String,
    pub slot: u64,
    pub success: bool,
    pub experience: serde_json::Value,
    pub error: Option<String>,
    pub failure_json: Option<serde_json::Value>,
    pub failing_instruction: Option<serde_json::Value>,
    pub raydium_product: Option<serde_json::Value>,
    pub raydium_context: Option<serde_json::Value>,
    pub registry_snapshot: Option<serde_json::Value>,
    pub cpi_failures: Vec<String>,
    pub selected_logs: Vec<String>,
    pub recommended_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Provider/model response returned by the optional AI question path.
pub struct AiResponse {
    pub model: String,
    pub answer: String,
}

impl AiDebugContext {
    /// Builds a compact, prompt-safe context from the deterministic debugger output.
    pub fn from_debug_info(info: &TransactionDebugInfo) -> anyhow::Result<Self> {
        let cpi_failures = info
            .cpi_tree
            .iter()
            .filter(|frame| frame.status == "failed")
            .map(|frame| frame.message.clone())
            .collect();
        let selected_logs = info
            .logs
            .iter()
            .filter(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("error")
                    || lower.contains("failed")
                    || lower.contains("instruction:")
                    || lower.contains("slippage")
                    || lower.contains("compute")
            })
            .take(40)
            .cloned()
            .collect();

        Ok(Self {
            signature: info.signature.clone(),
            slot: info.slot,
            success: info.success,
            experience: serde_json::to_value(&info.experience)
                .context("failed to serialize experience summary")?,
            error: info.error.clone(),
            failure_json: info
                .failure
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .context("failed to serialize standardized failure")?,
            failing_instruction: info
                .failing_instruction
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .context("failed to serialize failing instruction")?,
            raydium_product: info
                .raydium_product
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .context("failed to serialize Raydium product")?,
            raydium_context: info
                .raydium_context
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .context("failed to serialize Raydium context")?,
            registry_snapshot: registry_snapshot(),
            cpi_failures,
            selected_logs,
            recommended_actions: info.recommended_actions.clone(),
        })
    }
}

fn registry_snapshot() -> Option<serde_json::Value> {
    let raw = include_str!("failures/raydium_registry.generated.json");
    serde_json::from_str(raw).ok()
}

/// Creates the guarded prompt used by `ask_ai` without making a network call.
pub fn build_ai_prompt(context: &AiDebugContext, question: &str) -> anyhow::Result<ChatRequest> {
    let context_json =
        serde_json::to_string_pretty(context).context("failed to serialize AI debug context")?;
    Ok(ChatRequest::new(vec![
        ChatMessage::system(
            "You answer questions about a Solana/Raydium transaction debug result. \
             Treat the structured standardized failure as ground truth. \
             If decode_status says an artifact is missing, do not invent a name or meaning for the custom code. \
             Explain what IDL/source/SDK/docs artifact would be needed to decode it. \
             Use only the provided context; if something is not present, say what is missing. \
             Separate facts from guesses. Do not suggest signing or sending a transaction.",
        ),
        ChatMessage::user(format!(
            "Transaction debug context:\n```json\n{context_json}\n```\n\nUser question: {question}"
        )),
    ]))
}

/// Asks the configured `genai` model about an already-debugged transaction.
pub async fn ask_ai(
    info: &TransactionDebugInfo,
    question: &str,
    model_override: Option<&str>,
) -> anyhow::Result<AiResponse> {
    let model = model_override
        .map(str::to_string)
        .or_else(|| std::env::var("RAYDIUM_DEBUGGER_AI_MODEL").ok())
        .ok_or_else(|| {
            anyhow!(
                "AI question requested but no model was configured. Set --ai-model or RAYDIUM_DEBUGGER_AI_MODEL."
            )
        })?;
    let context = AiDebugContext::from_debug_info(info)?;
    let chat_req = build_ai_prompt(&context, question)?;
    let client = Client::new().context("failed to initialize genai client")?;
    let chat_res = client
        .exec_chat(&model, chat_req, None)
        .await
        .with_context(|| format!("AI request failed for model {model}"))?;
    Ok(AiResponse {
        model,
        answer: chat_res.first_text().unwrap_or("").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::failures::StandardizedFailure;

    #[test]
    fn prompt_contains_context_and_guardrails() {
        let info = TransactionDebugInfo {
            signature: "sig".to_string(),
            failure: Some(StandardizedFailure {
                source: "test".to_string(),
                program_id: Some("program".to_string()),
                program_label: Some("Program".to_string()),
                instruction_index: Some(1),
                code_decimal: Some(6001),
                code_hex: Some("0x1771".to_string()),
                name: Some("InvalidOwner".to_string()),
                title: "Program rejected the transaction".to_string(),
                user_message: "Program failed with InvalidOwner".to_string(),
                technical_message: "technical".to_string(),
                category: "authority_or_owner".to_string(),
                severity: "error".to_string(),
                confidence: "high".to_string(),
                evidence: vec!["evidence".to_string()],
                suggested_actions: vec!["action".to_string()],
                decode_status: "decoded_registry".to_string(),
                decode_attempts: vec!["Matched code 6001 against the test error list.".to_string()],
                missing_artifact: None,
                plain_title: Some("Plain title".to_string()),
                plain_explanation: Some("Plain explanation".to_string()),
                primary_action: Some("Primary action".to_string()),
                action_checklist: vec!["Checklist item".to_string()],
                evidence_summary: vec!["Evidence summary".to_string()],
                decode_explanation: Some("Decode explanation".to_string()),
            }),
            ..TransactionDebugInfo::default()
        };
        let context = AiDebugContext::from_debug_info(&info).unwrap();
        let prompt = build_ai_prompt(&context, "what happened?").unwrap();
        let serialized = serde_json::to_string(&prompt).unwrap();
        assert!(serialized.contains("standardized failure as ground truth"));
        assert!(serialized.contains("do not invent a name"));
        assert!(serialized.contains("0x1771"));
        assert!(serialized.contains("what happened?"));
    }
}
