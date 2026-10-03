//! Private review packages contain source context, never approvals inferred from heuristics.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct ReviewMessage {
    revision_id: i64,
    file_name: String,
    message_id: String,
    sender: Option<String>,
    date: Option<String>,
    body: String,
}
#[derive(Serialize, Deserialize)]
struct ReviewCase {
    case_id: String,
    conversation_family: String,
    split: String,
    sampling_tags: Vec<String>,
    approval: Option<bool>,
    reviewer: Option<String>,
    messages: Vec<ReviewMessage>,
}
#[derive(Serialize, Deserialize)]
struct ReviewQuery {
    id: String,
    conversation_family: String,
    split: String,
    query: String,
    source_revision_id: i64,
    relevant: Option<Vec<String>>,
    forbidden: Option<Vec<String>>,
    reviewer: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct ReviewPack {
    schema_version: u32,
    review_status: String,
    provenance: String,
    cases: Vec<ReviewCase>,
    queries: Vec<ReviewQuery>,
}

fn split(family: &str) -> &'static str {
    if Sha256::digest(family.as_bytes())[0].is_multiple_of(3) {
        "held_out"
    } else {
        "development"
    }
}
fn question(body: &str) -> bool {
    if body.chars().count() > 1000 {
        return false;
    }
    let text = body.to_lowercase();
    text.contains('?')
        || [
            "error",
            "failed",
            "not showing",
            "not visible",
            "issue",
            "problem",
            "unable",
            "cannot",
            "can't",
            "help",
        ]
        .iter()
        .any(|term| text.contains(term))
}

/// Samples thirty distinct conversation families and sixty verbatim problem queries.
/// Only the initiating reporter's problem messages become queries; resolution text
/// remains local context. This does not approve cases or infer relevance labels.
pub(super) fn prepare(database: &Path, directory: &Path) -> anyhow::Result<()> {
    let mut connection = open_existing_database(database)?;
    initialize_schema(&connection)?;
    resolve_reply_edges(&mut connection)?;
    rebuild_candidate_cases(&mut connection)?;
    let mut groups: std::collections::BTreeMap<String, Vec<ReviewCase>> =
        std::collections::BTreeMap::new();
    let mut offset = 0;
    loop {
        let ids: Vec<String> = {
            let mut statement=connection.prepare("SELECT case_id FROM candidate_cases WHERE is_current=1 AND corpus_id='support' ORDER BY case_id LIMIT 128 OFFSET ?1")?;
            let rows = statement.query_map([offset], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        if ids.is_empty() {
            break;
        }
        offset += ids.len() as i64;
        for id in ids {
            let mut statement=connection.prepare("SELECT m.revision_id,f.file_name,m.source_message_id,m.sender,m.date_title,m.body FROM candidate_case_messages c JOIN message_revisions m ON m.revision_id=c.revision_id JOIN source_files f ON f.source_file_id=m.source_file_id WHERE c.case_id=?1 ORDER BY m.revision_id LIMIT 128")?;
            let rows = statement.query_map([&id], |r| {
                Ok(ReviewMessage {
                    revision_id: r.get(0)?,
                    file_name: r.get(1)?,
                    message_id: r.get(2)?,
                    sender: r.get(3)?,
                    date: r.get(4)?,
                    body: r.get(5)?,
                })
            })?;
            let messages: Vec<_> = rows.collect::<rusqlite::Result<_>>()?;
            let Some(first) = messages.first() else {
                continue;
            };
            let questions = messages
                .iter()
                .filter(|m| m.sender.is_some() && m.sender == first.sender && question(&m.body))
                .count();
            if questions < 2 {
                continue;
            }
            let year = first
                .date
                .as_deref()
                .and_then(|d| d.get(6..10))
                .unwrap_or("unknown");
            let mut tags = vec![format!("period:{year}")];
            let text = messages
                .iter()
                .map(|m| m.body.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            for product in ["clmm", "cpmm", "amm", "swap", "pool", "token", "liquidity"] {
                if text.contains(product) {
                    tags.push(format!("suggested:{product}"));
                }
            }
            if text.contains("custom(") || text.contains("0x") {
                tags.push("suggested:custom_error".into());
            }
            let bucket = format!("{year}:{}", tags.get(1).cloned().unwrap_or_default());
            groups.entry(bucket).or_default().push(ReviewCase {
                conversation_family: id.clone(),
                split: split(&id).into(),
                case_id: id,
                sampling_tags: tags,
                approval: None,
                reviewer: None,
                messages,
            });
        }
    }
    let mut selected = Vec::new();
    while selected.len() < 30 {
        let before = selected.len();
        for group in groups.values_mut() {
            if selected.len() == 30 {
                break;
            }
            if let Some(case) = group.pop() {
                selected.push(case);
            }
        }
        anyhow::ensure!(before!=selected.len(),"archive has fewer than 30 families with two reporter problem messages; no synthetic examples were substituted");
    }
    let mut queries = Vec::new();
    for case in &selected {
        let reporter = &case.messages[0].sender;
        for message in case
            .messages
            .iter()
            .filter(|m| m.sender.is_some() && &m.sender == reporter && question(&m.body))
            .take(2)
        {
            queries.push(ReviewQuery {
                id: format!("query:{}:{}", case.case_id, message.revision_id),
                conversation_family: case.conversation_family.clone(),
                split: case.split.clone(),
                query: message.body.clone(),
                source_revision_id: message.revision_id,
                relevant: None,
                forbidden: None,
                reviewer: None,
            });
        }
    }
    let pack=ReviewPack {schema_version:1,review_status:"pending_human_review".into(),provenance:"Private archive conversations. Sampling tags are heuristic suggestions. Approval and relevance labels are intentionally unset. Query features exclude responder resolution text.".into(),cases:selected,queries};
    fs::create_dir_all(directory)?;
    let output = directory.join("review-pack.json");
    anyhow::ensure!(
        !output.exists(),
        "review package already exists; refusing to overwrite human review work"
    );
    fs::write(&output, serde_json::to_vec_pretty(&pack)?)?;
    println!("Prepared 30 private conversations and 60 unlabeled queries. Human approval remains pending.");
    Ok(())
}

pub(super) fn evaluate(pack_path: &Path, artifact: &Path) -> anyhow::Result<()> {
    let pack: ReviewPack = serde_json::from_slice(&fs::read(pack_path)?)?;
    anyhow::ensure!(
        pack.cases.len() == 30 && pack.queries.len() == 60,
        "review pack requires 30 cases and 60 queries"
    );
    for case in &pack.cases {
        anyhow::ensure!(
            case.approval.is_some() && case.reviewer.as_ref().is_some_and(|s| !s.trim().is_empty()),
            "case approvals and explicit reviewer identities remain incomplete"
        );
    }
    for query in &pack.queries {
        anyhow::ensure!(
            query.relevant.is_some()
                && query.forbidden.is_some()
                && query
                    .reviewer
                    .as_ref()
                    .is_some_and(|s| !s.trim().is_empty()),
            "human relevance labels remain incomplete"
        );
        let case = pack
            .cases
            .iter()
            .find(|case| case.conversation_family == query.conversation_family)
            .context("query references unknown family")?;
        anyhow::ensure!(
            case.split == query.split,
            "conversation family crosses evaluation splits"
        );
    }
    let registry = raydium_knowledge::CompiledRegistry::load(artifact)?;
    for partition in ["development", "held_out"] {
        let queries:Vec<_>=pack.queries.iter().filter(|q|q.split==partition).map(|q|serde_json::json!({"id":q.id,"query":q.query,"relevant":q.relevant,"forbidden":q.forbidden,"reviewer":q.reviewer})).collect();
        anyhow::ensure!(!queries.is_empty(), "evaluation split has no queries");
        let benchmark = serde_json::json!({"review_status":"reviewed","reviewer":"review-pack-per-query-identities","incidents":registry.incidents(),"queries":queries});
        let temporary = pack_path.with_extension(format!("{partition}.benchmark.json"));
        fs::write(&temporary, serde_json::to_vec_pretty(&benchmark)?)?;
        let report = evaluation::evaluate_benchmark(&temporary)?;
        println!("{partition}: {}", serde_json::to_string_pretty(&report)?);
        anyhow::ensure!(
            report.reviewed && report.quality_gate,
            "reviewed retrieval release gate failed"
        );
    }
    Ok(())
}

pub(super) fn curate(
    connection: &Connection,
    id: &str,
    path: &Path,
    reviewer: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !reviewer.trim().is_empty(),
        "explicit reviewer identity is required"
    );
    let incident: raydium_knowledge::CuratedIncident = serde_json::from_slice(&fs::read(path)?)?;
    anyhow::ensure!(
        incident.id == id,
        "annotation ID must preserve candidate identity"
    );
    raydium_knowledge::CompiledRegistry::new(vec![incident.clone()])?;
    for tag in &incident.symptom_tags {
        anyhow::ensure!(
            !tag.contains("http")
                && !tag.contains('@')
                && extract_entity_candidates(tag).is_empty(),
            "symptom tags must be sanitized"
        );
    }
    let transaction = connection.unchecked_transaction()?;
    annotate_candidate(
        &transaction,
        id,
        incident.product.as_deref().unwrap_or("unknown"),
        incident.failure_domain.as_deref().unwrap_or("unknown"),
        &incident.summary,
        &incident.resolution,
    )?;
    transaction.execute("UPDATE case_annotations SET curated_json=?1,annotation_reviewer=?2,symptom_tags_json=?3 WHERE case_id=?4",params![serde_json::to_string(&incident)?,reviewer,serde_json::to_string(&incident.symptom_tags)?,id])?;
    transaction.commit()?;
    Ok(())
}
