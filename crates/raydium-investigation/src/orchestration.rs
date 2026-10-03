//! Blocking evidence assembly. Historical retrieval never reads private support archives.
use super::*;
/// Assembles evidence on a blocking thread using a shared normalized registry.
/// Provider failures become explicit unknown evidence; private source contents are
/// never read. A persistence failure from the progress callback aborts assembly.
pub fn execute_investigation(
    investigation_id: &str,
    request: &InvestigationRequest,
    registry: &raydium_knowledge::CompiledRegistry,
    observations_database_path: &Path,
    progress: &dyn Fn(InvestigationEvent) -> anyhow::Result<()>,
) -> anyhow::Result<InvestigationResult> {
    let signature = request
        .signature
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let symptom = request
        .symptom
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    validate_request(request)?;
    let (transaction_diagnosis, transaction_error) = if let Some(signature) = signature {
        send_progress(
            progress,
            investigation_id,
            "transaction",
            "Checking transaction evidence",
        )?;
        match run_diagnostic_request_blocking(DebugRequest {
            signature: signature.to_string(),
            cluster: request.cluster,
            data_mode: Some(DebugDataMode::RpcOnly),
            ..DebugRequest::default()
        }) {
            Ok(diagnosis) => (Some(diagnosis), None),
            Err(_) => (
                None,
                Some(
                    "Transaction evidence could not be fetched from the selected provider."
                        .to_string(),
                ),
            ),
        }
    } else {
        (None, None)
    };

    send_progress(
        progress,
        investigation_id,
        "knowledge",
        "Checking approved incidents and recent observations",
    )?;
    let selected_observations = if let Some(fingerprint) = &request.recent_fingerprint {
        let rows = recent_observation_groups(
            observations_database_path,
            request.cluster,
            Some(fingerprint),
        )?;
        if rows.is_empty() {
            anyhow::bail!("recent observation group was not found in the rolling window");
        }
        rows
    } else {
        Vec::new()
    };
    let mut search_text = symptom
        .map(str::to_string)
        .or_else(|| {
            transaction_diagnosis.as_ref().map(|diagnosis| {
                format!(
                    "{} {} {}",
                    diagnosis.diagnosis.title,
                    diagnosis.diagnosis.category,
                    diagnosis.diagnosis.explanation
                )
            })
        })
        .unwrap_or_default();
    if search_text.is_empty() {
        if let Some(observation) = selected_observations.first() {
            search_text = format!(
                "{} {}",
                observation.instruction.as_deref().unwrap_or_default(),
                observation.error_code.as_deref().unwrap_or_default()
            );
        }
    }
    let mut features = MatchFeatures::default();
    if let Some(cluster) = request.cluster {
        features.set("cluster", [cluster.as_str().to_string()]);
    }
    if let Some(transaction) = transaction_diagnosis
        .as_ref()
        .and_then(|diagnosis| diagnosis.transaction.as_ref())
    {
        features.set(
            "outcome",
            [if transaction.success {
                "success"
            } else {
                "failure"
            }
            .into()],
        );
        features.set("program", transaction.program_ids.clone());
        // An undecoded instruction is unknown, not proof an instruction is absent.
        if !transaction.decoded_instructions.is_empty()
            && transaction
                .decoded_instructions
                .iter()
                .all(|instruction| instruction.semantic_decode.is_some())
        {
            features.set(
                "instruction",
                transaction
                    .decoded_instructions
                    .iter()
                    .filter_map(|instruction| {
                        instruction
                            .semantic_decode
                            .as_ref()
                            .map(|decoded| decoded.instruction_name.clone())
                    }),
            );
        }
        if let Some(product) = &transaction.raydium_product {
            let product = serde_json::to_value(&product.product)?;
            if let Some(product) = product.as_str().filter(|product| *product != "unknown") {
                features.set("product", [product.to_string()]);
            }
        }
        features.set(
            "error",
            transaction
                .failure
                .as_ref()
                .and_then(|failure| failure.code_decimal)
                .map(|code| code.to_string()),
        );
        if let Some(program) = transaction
            .logs
            .iter()
            .find_map(|line| {
                let (program, error) = line.strip_prefix("Program ")?.split_once(" failed: ")?;
                Some((
                    program.to_string(),
                    raydium_debugger::parse_custom_error_code(error),
                ))
            })
            .or_else(|| {
                transaction.failure.as_ref().and_then(|failure| {
                    failure
                        .program_id
                        .clone()
                        .map(|program| (program, failure.code_decimal))
                })
            })
        {
            features.set("error_program", [program.0]);
            features.set("error", program.1.map(|code| code.to_string()));
        }
        features.observed_at = transaction.timestamp;
    } else if let Some(observation) = selected_observations.first() {
        if let Some(program) = &observation.program_id {
            features.set("program", [program.clone()]);
        }
        if let Some(instruction) = &observation.instruction {
            features.set("instruction", [instruction.clone()]);
        }
        if let Some(error) = &observation.error_code {
            features.set("error", [error.clone()]);
            if let Some(program) = &observation.program_id {
                features.set("error_program", [program.clone()]);
            }
        }
        features.observed_at = Some(observation.observed_at);
    }
    let (related_incidents, incident_matches) = registry.matches(&search_text, &features);
    let (recent_observations, observation_unavailable) = if selected_observations.is_empty() {
        match recent_observation_matches(observations_database_path, &search_text, request.cluster)
        {
            Ok(observations) => (observations, false),
            Err(_) => (Vec::new(), true),
        }
    } else {
        (selected_observations, false)
    };

    let mut evidence = Vec::new();
    if let Some(symptom) = symptom {
        evidence.push(InvestigationEvidence {
            evidence_id: format!("{investigation_id}:report"),
            evidence_type: "user_report".to_string(),
            source_reference: investigation_id.to_string(),
            summary: symptom.to_string(),
            observed_at: None,
        });
    }
    if let Some(diagnosis) = &transaction_diagnosis {
        evidence.push(InvestigationEvidence {
            evidence_id: format!("{investigation_id}:transaction"),
            evidence_type: "transaction_diagnosis".to_string(),
            source_reference: diagnosis
                .transaction
                .as_ref()
                .map(|transaction| transaction.signature.clone())
                .or_else(|| signature.map(str::to_string))
                .unwrap_or_default(),
            summary: diagnosis.diagnosis.title.clone(),
            observed_at: diagnosis
                .transaction
                .as_ref()
                .and_then(|transaction| transaction.timestamp),
        });
    }
    for incident in &related_incidents {
        evidence.push(InvestigationEvidence {
            evidence_id: format!("{investigation_id}:incident:{}", incident.id),
            evidence_type: "approved_historical_incident".to_string(),
            source_reference: incident.id.clone(),
            summary: incident.summary.clone(),
            observed_at: None,
        });
    }
    for observation in &recent_observations {
        evidence.push(InvestigationEvidence {
            evidence_id: format!(
                "{investigation_id}:recent:{}:{}:{}",
                observation.fingerprint, observation.source, observation.observed_at
            ),
            evidence_type: "recent_observation".to_string(),
            source_reference: observation.fingerprint.clone(),
            summary: format!(
                "Recent {} observation{}{}",
                observation.source,
                observation
                    .error_code
                    .as_deref()
                    .map(|code| format!(" with error {code}"))
                    .unwrap_or_default(),
                observation
                    .instruction
                    .as_deref()
                    .map(|instruction| format!(" for {instruction}"))
                    .unwrap_or_default()
            ),
            observed_at: Some(observation.observed_at),
        });
    }

    let mut unknowns = Vec::new();
    if observation_unavailable {
        unknowns.push("Operational observation evidence could not be read.".into());
    }
    if signature.is_none() {
        unknowns.push(
            "No transaction signature was supplied; no on-chain failure is asserted.".to_string(),
        );
    }
    if transaction_error.is_some() {
        unknowns.push(
            "Transaction evidence could not be fetched from the selected provider.".to_string(),
        );
    }
    if related_incidents.is_empty() {
        unknowns.push("No approved historical incident matched this input.".to_string());
    }
    if recent_observations.is_empty() {
        unknowns.push(
            "No matching recent operational observation was found in the rolling window."
                .to_string(),
        );
    }
    let result = InvestigationResult {
        investigation_id: investigation_id.to_string(),
        status: if transaction_diagnosis.is_some()
            || !related_incidents.is_empty()
            || !recent_observations.is_empty()
        {
            "complete".to_string()
        } else {
            "partial".to_string()
        },
        signature: signature.map(str::to_string),
        symptom: symptom.map(str::to_string),
        cluster: request.cluster.map(|cluster| cluster.as_str().to_string()),
        transaction_diagnosis,
        transaction_error,
        related_incidents,
        incident_matches,
        recent_observations,
        evidence,
        unknowns,
    };
    Ok(result)
}

fn send_progress(
    sender: &dyn Fn(InvestigationEvent) -> anyhow::Result<()>,
    investigation_id: &str,
    stage: &str,
    message: &str,
) -> anyhow::Result<()> {
    sender(InvestigationEvent {
        event_id: String::new(),
        event_type: "progress".to_string(),
        investigation_id: investigation_id.to_string(),
        stage: Some(stage.to_string()),
        message: Some(message.to_string()),
        result: None,
        error: None,
    })
}
