//! Private review operations; blocking operator tooling.
use super::*;
pub(super) fn review_candidate(
    connection: &mut Connection,
    case_id: &str,
    action: &str,
    rationale: &str,
    reviewer: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !reviewer.trim().is_empty(),
        "reviewer identity must not be empty"
    );
    if rationale.trim().is_empty() {
        bail!("review rationale must not be empty");
    }
    if !matches!(action, "approve" | "reject") {
        bail!("review action must be approve or reject");
    }
    if action == "approve" {
        ensure_publishable_resolution(connection, case_id)?;
        let annotated: bool = connection.query_row(
            "SELECT EXISTS (
                SELECT 1 FROM case_annotations WHERE case_id = ?1
                  AND length(trim(summary)) > 0 AND length(trim(resolution)) > 0
             )",
            [case_id],
            |row| row.get(0),
        )?;
        if !annotated {
            bail!("add a curated summary and resolution with candidates annotate before approval");
        }
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs() as i64;
    let status = match action {
        "approve" => "approved",
        "reject" => "rejected",
        _ => unreachable!("action was checked above"),
    };
    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE candidate_cases SET review_status = ?1, updated_at = ?2
         WHERE case_id = ?3 AND is_current = 1",
        params![status, timestamp, case_id],
    )?;
    if changed == 0 {
        bail!("current candidate case not found: {case_id}");
    }
    transaction.execute(
        "INSERT INTO review_events (case_id, action, reviewer, rationale, reviewed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![case_id, action, reviewer, rationale.trim(), timestamp],
    )?;
    transaction.commit()?;
    println!("Candidate {case_id} {status}.");
    Ok(())
}

pub(super) fn annotate_candidate(
    connection: &Connection,
    case_id: &str,
    product: &str,
    failure_domain: &str,
    summary: &str,
    resolution: &str,
) -> anyhow::Result<()> {
    for (label, value, max_chars) in [
        ("product", product, 80),
        ("failure domain", failure_domain, 80),
        ("summary", summary, 500),
        ("resolution", resolution, 1_000),
    ] {
        if value.trim().is_empty() {
            bail!("{label} must not be empty");
        }
        if value.chars().count() > max_chars {
            bail!("{label} exceeds {max_chars} characters");
        }
    }
    for text in [product, failure_domain, summary, resolution] {
        let lowercase = text.to_ascii_lowercase();
        if lowercase.contains("http://") || lowercase.contains("https://") {
            bail!("curated incident text must not contain links");
        }
        if extract_entity_candidates(text).iter().any(|entity| {
            matches!(
                entity.entity_type,
                "address_candidate" | "transaction_signature_candidate"
            )
        }) {
            bail!("curated incident text must not contain wallet addresses or signatures");
        }
    }
    let current: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM candidate_cases WHERE case_id = ?1 AND is_current = 1)",
        [case_id],
        |row| row.get(0),
    )?;
    if !current {
        bail!("current candidate case not found: {case_id}");
    }
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs() as i64;
    connection.execute(
        "INSERT INTO case_annotations (
            case_id, product, failure_domain, summary, resolution,
            symptom_tags_json, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, '[]', ?6)
         ON CONFLICT(case_id) DO UPDATE SET
            product = excluded.product,
            failure_domain = excluded.failure_domain,
            summary = excluded.summary,
            resolution = excluded.resolution,
            updated_at = excluded.updated_at,
            curated_json = NULL, annotation_reviewer = NULL",
        params![
            case_id,
            product.trim(),
            failure_domain.trim(),
            summary.trim(),
            resolution.trim(),
            updated_at
        ],
    )?;
    connection.execute(
        "UPDATE candidate_cases SET review_status='candidate' WHERE case_id=?1",
        [case_id],
    )?;
    println!("Saved curated annotation for {case_id}; approval is required after edits.");
    Ok(())
}

pub(super) fn build_generated_registry(
    connection: &Connection,
) -> anyhow::Result<GeneratedRegistry> {
    let source_revision = connection.query_row(
        "SELECT COALESCE(MAX(revision_id), 0) FROM message_revisions",
        [],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT candidate_cases.case_id, case_annotations.product,
                case_annotations.failure_domain, case_annotations.summary,
                case_annotations.resolution, case_annotations.symptom_tags_json,
                count(candidate_case_messages.revision_id)
         FROM candidate_cases
         JOIN case_annotations ON case_annotations.case_id = candidate_cases.case_id
         LEFT JOIN candidate_case_messages ON candidate_case_messages.case_id = candidate_cases.case_id
         WHERE candidate_cases.is_current = 1
           AND candidate_cases.review_status = 'approved'
         GROUP BY candidate_cases.case_id
         ORDER BY candidate_cases.case_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)? as usize,
        ))
    })?;
    let mut incidents = Vec::new();
    for row in rows {
        let (id, product, failure_domain, summary, resolution, tags_json, evidence_count) = row?;
        ensure_publishable_resolution(connection, &id)?;
        let curated: Option<String> = connection.query_row(
            "SELECT curated_json FROM case_annotations WHERE case_id=?1",
            [&id],
            |r| r.get(0),
        )?;
        if let Some(curated) = curated {
            let mut incident: GeneratedIncident = serde_json::from_str(&curated)?;
            anyhow::ensure!(incident.id == id, "curated identity mismatch");
            incident.evidence_message_count = evidence_count;
            raydium_knowledge::CompiledRegistry::new(vec![incident.clone()])?;
            incidents.push(incident);
            continue;
        }
        incidents.push(GeneratedIncident {
            id,
            product: optional_annotation_value(&product),
            failure_domain: optional_annotation_value(&failure_domain),
            summary,
            resolution,
            symptom_tags: serde_json::from_str(&tags_json)?,
            evidence_message_count: evidence_count,
            must: Vec::new(),
            should: Vec::new(),
            must_not: Vec::new(),
            retired: false,
            valid_from: None,
            valid_until: None,
        });
    }
    let mut guidance = Vec::new();
    let mut statement = connection.prepare("SELECT packet_id,packet_kind,source_fingerprint,product,failure_domain,category,summary,guidance,evidence_json,reference_ids_json
        FROM corpus_packet_reviews WHERE publication_status='published_guidance' AND value_status='valuable' ORDER BY packet_id")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
        ))
    })?;
    for row in rows {
        let (
            id,
            kind,
            source_hash,
            product,
            domain,
            category,
            summary,
            advice,
            evidence_json,
            reference_json,
        ) = row?;
        let evidence: Vec<i64> = serde_json::from_str(&evidence_json)?;
        let references: Vec<String> = serde_json::from_str(&reference_json)?;
        if evidence.is_empty() {
            continue;
        }
        let source = if kind == "case" {
            let current: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM candidate_cases WHERE case_id=?1 AND is_current=1)",
                [&id],
                |r| r.get(0),
            )?;
            if !current {
                continue;
            }
            messages(connection, &id)?
        } else {
            let revision = evidence[0];
            let grouped: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM candidate_case_messages cm JOIN candidate_cases c ON c.case_id=cm.case_id WHERE cm.revision_id=?1 AND c.is_current=1)", [revision], |r| r.get(0))?;
            if grouped {
                continue;
            }
            connection.query_row("SELECT m.revision_id,m.sender,m.body FROM message_revisions m JOIN source_files s ON s.source_file_id=m.source_file_id WHERE m.revision_id=?1 AND m.corpus_id='support' AND s.source_file_id=(SELECT MAX(latest.source_file_id) FROM source_files latest WHERE latest.corpus_id=s.corpus_id AND latest.file_name=s.file_name)", [revision], |r| Ok(ResolutionMessage { revision_id:r.get(0)?, sender:r.get(1)?, body:r.get(2)? })).optional()?.into_iter().collect()
        };
        if source_hash != fingerprint(&source)
            || !evidence.iter().all(|revision| {
                source
                    .iter()
                    .any(|message| message.revision_id == *revision)
            })
        {
            continue;
        }
        guidance.push(GeneratedGuidance {
            id,
            product: optional_annotation_value(&product),
            failure_domain: optional_annotation_value(&domain),
            category,
            summary,
            guidance: advice,
            evidence_message_count: evidence.len(),
            reference_count: references.len(),
        });
    }
    Ok(GeneratedRegistry {
        schema_version: 1,
        source_revision,
        incidents,
        guidance,
    })
}

pub(super) fn optional_annotation_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !value.eq_ignore_ascii_case("unknown")).then(|| value.to_string())
}

pub(super) fn compile_registry_to_file(
    database_path: &Path,
    output_path: &Path,
) -> anyhow::Result<()> {
    let connection = open_existing_database(database_path)?;
    initialize_schema(&connection)?;
    let registry = build_generated_registry(&connection)?;
    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }
    let temporary = output_path.with_extension("json.pending");
    {
        use std::io::Write;
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(&registry)?)?;
        file.sync_all()?;
    }
    fs::rename(&temporary, output_path)
        .with_context(|| format!("failed to publish {}", output_path.display()))?;
    println!(
        "Compiled {} approved sanitized incidents to {}.",
        registry.incidents.len(),
        output_path.display()
    );
    Ok(())
}
