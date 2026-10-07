//! Private corpus review API shared by the local web and desktop shells.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone)]
struct PacketSummary {
    id: String,
    kind: String,
    hash: String,
    preview: String,
    date: String,
    message_count: usize,
    searchable: String,
}

type CatalogEntry = (SystemTime, u64, Arc<Vec<PacketSummary>>);
static PACKET_CATALOGS: OnceLock<Mutex<HashMap<PathBuf, CatalogEntry>>> = OnceLock::new();

fn packet_catalog(path: &Path) -> anyhow::Result<Arc<Vec<PacketSummary>>> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;
    let size = metadata.len();
    let cache = PACKET_CATALOGS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((stamp, length, entries)) = cache.lock().unwrap().get(path) {
        if *stamp == modified && *length == size {
            return Ok(entries.clone());
        }
    }
    let mut entries = Vec::new();
    for packet in jsonl_values(path)? {
        let packet = packet?;
        let messages = packet["messages"]
            .as_array()
            .ok_or_else(|| anyhow!("packet messages missing"))?;
        let first = messages.first();
        entries.push(PacketSummary {
            id: string_field(&packet, "case_id")?.into(),
            kind: string_field(&packet, "kind")?.into(),
            hash: string_field(&packet, "evidence_fingerprint")?.into(),
            preview: first
                .and_then(|m| m["body"].as_str())
                .unwrap_or("")
                .chars()
                .take(220)
                .collect(),
            date: first.and_then(|m| m["date"].as_str()).unwrap_or("").into(),
            message_count: messages.len(),
            searchable: messages
                .iter()
                .filter_map(|m| m["body"].as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase(),
        });
    }
    let entries = Arc::new(entries);
    cache
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), (modified, size, entries.clone()));
    Ok(entries)
}

/// Paths used by the local review interface. All source and draft data stays private.
#[derive(Debug, Clone)]
pub struct ReviewStore {
    /// Builder database containing original messages and reviewer decisions.
    pub database: PathBuf,
    /// Prepared evidence packets for grouped cases and ungrouped messages.
    pub packets: PathBuf,
    /// Unreviewed AI classification drafts.
    pub drafts: PathBuf,
    /// Sanitized incident registry read by the debugger.
    pub published: PathBuf,
}

/// Bounded listing controls for the archive review interface.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct ReviewQuery {
    /// Case-insensitive text search across message bodies, category, and packet ID.
    pub search: Option<String>,
    /// `all`, `case`, or `orphan`.
    pub kind: Option<String>,
    /// `all`, `unreviewed`, `reviewed`, `approved`, `rejected`, or `stale`.
    pub status: Option<String>,
    /// `all`, `valuable`, `uncertain`, `not_useful`, or `unset`.
    pub value: Option<String>,
    /// Zero-based result offset.
    pub offset: Option<usize>,
    /// Maximum page size, capped at 100.
    pub limit: Option<usize>,
}

/// One archive record in the paginated review queue.
#[derive(Debug, Serialize)]
pub struct ReviewRow {
    /// Stable packet identity.
    pub id: String,
    /// `case` or `orphan`.
    pub kind: String,
    /// Short source excerpt.
    pub preview: String,
    /// First source message date.
    pub date: String,
    /// Number of source messages in the packet.
    pub message_count: usize,
    /// Current AI suggestion, if present.
    pub ai_outcome: Option<String>,
    /// Current AI evidence tier, if present.
    pub ai_tier: Option<String>,
    /// Model validation status, including invalid outputs.
    pub ai_status: Option<String>,
    /// Human-assigned value, if present.
    pub value_status: Option<String>,
    /// Current review or publication status.
    pub review_status: String,
    /// Current draft category, if present.
    pub category: Option<String>,
}

/// One page and whole-corpus coverage counters.
#[derive(Debug, Serialize)]
pub struct ReviewList {
    /// Total prepared packets.
    pub packet_count: usize,
    /// Number of grouped cases.
    pub case_count: usize,
    /// Number of ungrouped messages.
    pub orphan_count: usize,
    /// Current validated AI drafts.
    pub classified_count: usize,
    /// Current model outputs that failed validation.
    pub invalid_count: usize,
    /// Packets with a current human decision.
    pub reviewed_count: usize,
    /// Approved grouped incidents in the builder database.
    pub approved_count: usize,
    /// Published current guidance entries, separate from historical incidents.
    pub guidance_count: usize,
    /// Number of rows matching the selected filters.
    pub filtered_count: usize,
    /// Requested page.
    pub items: Vec<ReviewRow>,
}

/// Full private evidence shown only to a local authenticated reviewer.
#[derive(Debug, Serialize)]
pub struct ReviewDetail {
    /// Entire source evidence packet.
    pub packet: Value,
    /// Current AI draft, or null when absent/stale.
    pub draft: Option<Value>,
    /// Saved human decision, or null.
    pub decision: Option<Value>,
    /// Builder publication status for a grouped case.
    pub review_status: String,
    /// Local rule suggestions with source message IDs.
    pub rule_signals: Vec<Value>,
}

/// Verified original attachment for local preview.
#[derive(Debug, Serialize)]
pub struct ReviewMedia {
    /// Browser media type from an allowlist.
    pub mime: String,
    /// Original bytes, limited to 32 MiB and checked against the packet hash.
    pub bytes: Vec<u8>,
}

/// Editable human decision for any packet. Only grouped, evidenced outcomes can publish.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDecision {
    /// Fingerprint supplied by the detail response; rejects edits to changed evidence.
    pub evidence_fingerprint: String,
    /// `valuable`, `uncertain`, or `not_useful`.
    pub value_status: String,
    /// `confirmed`, `team_fixed`, `proposed`, or `unknown`.
    pub outcome: String,
    /// Reviewer classification.
    pub category: String,
    /// Private explanation of the problem.
    pub diagnosis: String,
    /// Sanitized public problem summary, required for approval.
    pub summary: String,
    /// Sanitized historical resolution, required for approval.
    pub resolution: String,
    /// Current technical guidance, kept private unless separately curated.
    pub guidance: String,
    /// Product label for approved guidance.
    pub product: String,
    /// Failure domain for approved guidance.
    pub failure_domain: String,
    /// Message revisions checked by the reviewer.
    pub evidence_revision_ids: Vec<i64>,
    /// Pinned documentation or code excerpts checked by the reviewer.
    #[serde(default)]
    pub reference_ids: Vec<String>,
    /// Explanation of the accepted classification.
    pub rationale: String,
    /// Reviewer name or identifier.
    pub reviewer: String,
    /// `save`, `approve`, `publish_guidance`, or `reject`.
    pub action: String,
}

impl ReviewStore {
    /// Uses the same private paths as the classifier and runtime knowledge loader.
    pub fn from_env() -> Self {
        let private = PathBuf::from(".raydium-debugger/ai-case-reviews");
        Self {
            database: support_database_path(),
            packets: std::env::var_os("RAYDIUM_DEBUGGER_REVIEW_PACKETS_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| private.join("packets.jsonl")),
            drafts: std::env::var_os("RAYDIUM_DEBUGGER_REVIEW_DRAFTS_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| private.join("drafts.jsonl")),
            published: std::env::var_os("RAYDIUM_DEBUGGER_KNOWLEDGE_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(".raydium-debugger/knowledge/incidents.generated.json")
                }),
        }
    }

    fn connection(&self) -> anyhow::Result<Connection> {
        let connection = open_existing_database(&self.database)?;
        initialize_schema(&connection)?;
        Ok(connection)
    }

    /// Lists the full prepared corpus with bounded paging and coverage totals.
    pub fn list(&self, query: ReviewQuery) -> anyhow::Result<ReviewList> {
        let connection = self.connection()?;
        let drafts = jsonl_map(&self.drafts)?;
        let decisions = decision_map(&connection)?;
        let statuses = case_status_map(&connection)?;
        let limit = query.limit.unwrap_or(30).clamp(1, 100);
        let offset = query.offset.unwrap_or(0);
        let search = query.search.unwrap_or_default().trim().to_lowercase();
        let kind_filter = query.kind.unwrap_or_else(|| "all".into());
        let status_filter = query.status.unwrap_or_else(|| "all".into());
        let value_filter = query.value.unwrap_or_else(|| "all".into());
        anyhow::ensure!(
            matches!(kind_filter.as_str(), "all" | "case" | "orphan"),
            "invalid kind filter"
        );
        anyhow::ensure!(
            matches!(
                status_filter.as_str(),
                "all"
                    | "unreviewed"
                    | "reviewed"
                    | "approved"
                    | "published_guidance"
                    | "rejected"
                    | "stale"
            ),
            "invalid status filter"
        );
        anyhow::ensure!(
            matches!(
                value_filter.as_str(),
                "all" | "valuable" | "uncertain" | "not_useful" | "unset"
            ),
            "invalid value filter"
        );
        let mut result = ReviewList {
            packet_count: 0,
            case_count: 0,
            orphan_count: 0,
            classified_count: 0,
            invalid_count: 0,
            reviewed_count: 0,
            approved_count: 0,
            guidance_count: 0,
            filtered_count: 0,
            items: Vec::new(),
        };
        let mut matches = Vec::new();
        for packet in packet_catalog(&self.packets)?.iter() {
            let id = packet.id.as_str();
            let kind = packet.kind.as_str();
            let draft = drafts
                .get(id)
                .filter(|draft| draft["evidence_fingerprint"] == packet.hash);
            let decision = decisions.get(id);
            let current_decision =
                decision.filter(|decision| decision["evidence_fingerprint"] == packet.hash);
            let status = if decision.is_some() && current_decision.is_none() {
                "stale"
            } else if current_decision
                .is_some_and(|item| item["publication_status"] == "published_guidance")
            {
                if guidance_source_current(&connection, id, current_decision.unwrap())? {
                    "published_guidance"
                } else {
                    "stale"
                }
            } else if kind == "case"
                && statuses
                    .get(id)
                    .is_some_and(|s| s == "approved" || s == "rejected")
            {
                if statuses.get(id).is_some_and(|s| s == "approved")
                    && review_state(&connection, id)? == "stale_review"
                {
                    "stale"
                } else {
                    statuses.get(id).map(String::as_str).unwrap_or("unreviewed")
                }
            } else if current_decision.is_some() {
                "reviewed"
            } else {
                "unreviewed"
            };
            result.packet_count += 1;
            if kind == "case" {
                result.case_count += 1;
            } else {
                result.orphan_count += 1;
            }
            if draft.is_some_and(|draft| draft["status"] == "ai_draft_unreviewed") {
                result.classified_count += 1;
            } else if draft.is_some_and(|draft| draft["status"] == "invalid_output") {
                result.invalid_count += 1;
            }
            if current_decision.is_some() && status != "stale" {
                result.reviewed_count += 1;
            }
            if status == "published_guidance" {
                result.guidance_count += 1;
            }
            if status == "approved" {
                result.approved_count += 1;
            }
            if kind_filter != "all" && kind_filter != kind {
                continue;
            }
            if status_filter != "all" && status_filter != status {
                continue;
            }
            let value_status = current_decision.and_then(|v| v["value_status"].as_str());
            if value_filter != "all"
                && (if value_filter == "unset" {
                    value_status.is_some()
                } else {
                    value_status != Some(value_filter.as_str())
                })
            {
                continue;
            }
            if !search.is_empty()
                && !id.to_lowercase().contains(&search)
                && !draft
                    .and_then(|draft| draft["category"].as_str())
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&search)
                && !packet.searchable.contains(&search)
            {
                continue;
            }
            result.filtered_count += 1;
            matches.push(ReviewRow {
                id: id.to_owned(),
                kind: kind.to_owned(),
                preview: packet.preview.clone(),
                date: packet.date.clone(),
                message_count: packet.message_count,
                ai_outcome: draft.and_then(|v| v["outcome"].as_str()).map(str::to_owned),
                ai_tier: draft.and_then(|v| v["tier"].as_str()).map(str::to_owned),
                ai_status: draft.and_then(|v| v["status"].as_str()).map(str::to_owned),
                value_status: current_decision
                    .and_then(|v| v["value_status"].as_str())
                    .map(str::to_owned),
                review_status: status.to_owned(),
                category: current_decision
                    .and_then(|v| v["category"].as_str())
                    .or_else(|| draft.and_then(|v| v["category"].as_str()))
                    .map(str::to_owned),
            });
        }
        matches.sort_by(|a, b| review_priority(a).cmp(&review_priority(b)));
        result.items = matches.into_iter().skip(offset).take(limit).collect();
        Ok(result)
    }

    /// Returns a packet, its draft, any saved decision, and rule evidence.
    pub fn detail(&self, id: &str) -> anyhow::Result<ReviewDetail> {
        let connection = self.connection()?;
        let packet = find_packet(&self.packets, id)?;
        let hash = string_field(&packet, "evidence_fingerprint")?;
        let source_stale = ensure_packet_matches_database(&connection, &packet).is_err();
        let draft = jsonl_map(&self.drafts)?
            .remove(id)
            .filter(|draft| draft["evidence_fingerprint"] == hash);
        let decision = decision_map(&connection)?.remove(id);
        let status = if source_stale
            || decision
                .as_ref()
                .is_some_and(|item| item["evidence_fingerprint"] != hash)
        {
            "stale".to_owned()
        } else if decision
            .as_ref()
            .is_some_and(|item| item["publication_status"] == "published_guidance")
        {
            if guidance_source_current(&connection, id, decision.as_ref().unwrap())? {
                "published_guidance".into()
            } else {
                "stale".into()
            }
        } else if packet["kind"] == "case" {
            let status = case_status_map(&connection)?.remove(id).unwrap_or_default();
            if status == "approved" && review_state(&connection, id)? == "stale_review" {
                "stale".into()
            } else if status == "approved" || status == "rejected" {
                status
            } else if decision.is_some() {
                "reviewed".into()
            } else {
                "unreviewed".into()
            }
        } else if decision.is_some() {
            "reviewed".into()
        } else {
            "unreviewed".into()
        };
        let mut rule_signals = Vec::new();
        if packet["kind"] == "case" {
            let mut statement = connection.prepare("SELECT revision_id,signal FROM case_resolution_suggestions WHERE case_id=?1 ORDER BY revision_id")?;
            for row in statement.query_map([id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })? {
                let (revision_id, signal) = row?;
                rule_signals.push(json!({"revision_id":revision_id,"signal":signal}));
            }
        }
        Ok(ReviewDetail {
            packet,
            draft,
            decision,
            review_status: status,
            rule_signals,
        })
    }

    /// Reads a source attachment only after verifying packet membership, path, and hash.
    pub fn media(&self, id: &str, revision: i64, index: usize) -> anyhow::Result<ReviewMedia> {
        let packet = find_packet(&self.packets, id)?;
        let attachment = packet["messages"]
            .as_array()
            .and_then(|messages| {
                messages
                    .iter()
                    .find(|message| message["revision_id"] == revision)
            })
            .and_then(|message| message["attachments"].as_array())
            .and_then(|attachments| attachments.get(index))
            .ok_or_else(|| anyhow!("attachment not found in this packet"))?;
        let relative = attachment["relative_path"]
            .as_str()
            .ok_or_else(|| anyhow!("attachment path missing"))?;
        let expected_hash = attachment["sha256"]
            .as_str()
            .ok_or_else(|| anyhow!("attachment hash missing"))?;
        let mime = match Path::new(relative)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "pdf" => "application/pdf",
            "mp4" => "video/mp4",
            _ => bail!("attachment type is not previewable"),
        };
        let root = std::env::var_os("RAYDIUM_DEBUGGER_CHAT_EXPORT_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".raydium-debugger/ChatExport_2026-09-28"));
        let root = root.canonicalize()?;
        let path = root.join(relative).canonicalize()?;
        anyhow::ensure!(
            path.starts_with(&root) && path.is_file(),
            "attachment path leaves private archive"
        );
        anyhow::ensure!(
            path.metadata()?.len() <= 32 * 1024 * 1024,
            "attachment is too large for browser preview"
        );
        let bytes = fs::read(path)?;
        anyhow::ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == expected_hash,
            "attachment changed since packet preparation"
        );
        Ok(ReviewMedia {
            mime: mime.into(),
            bytes,
        })
    }

    /// Saves the review decision; approval also compiles the sanitized registry.
    pub fn submit(&self, id: &str, decision: ReviewDecision) -> anyhow::Result<ReviewDetail> {
        let packet = find_packet(&self.packets, id)?;
        let hash = string_field(&packet, "evidence_fingerprint")?;
        anyhow::ensure!(
            hash == decision.evidence_fingerprint,
            "source evidence changed; reload this packet"
        );
        anyhow::ensure!(
            matches!(
                decision.value_status.as_str(),
                "valuable" | "uncertain" | "not_useful"
            ),
            "invalid value status"
        );
        anyhow::ensure!(
            matches!(
                decision.outcome.as_str(),
                "confirmed" | "team_fixed" | "proposed" | "unknown"
            ),
            "invalid outcome"
        );
        anyhow::ensure!(
            matches!(
                decision.action.as_str(),
                "save" | "approve" | "publish_guidance" | "reject"
            ),
            "invalid review action"
        );
        anyhow::ensure!(
            !decision.reviewer.trim().is_empty() && !decision.rationale.trim().is_empty(),
            "reviewer and rationale are required"
        );
        anyhow::ensure!(
            decision.category.chars().count() <= 120
                && decision.diagnosis.chars().count() <= 2_000
                && decision.guidance.chars().count() <= 2_000,
            "review text is too long"
        );
        let member_ids = packet["messages"]
            .as_array()
            .ok_or_else(|| anyhow!("packet messages missing"))?
            .iter()
            .filter_map(|message| message["revision_id"].as_i64())
            .collect::<HashSet<_>>();
        anyhow::ensure!(
            decision
                .evidence_revision_ids
                .iter()
                .all(|id| member_ids.contains(id)),
            "evidence revision does not belong to this packet"
        );
        let reference_ids = packet["references"]
            .as_array()
            .ok_or_else(|| anyhow!("packet references missing"))?
            .iter()
            .filter_map(|reference| reference["id"].as_str())
            .collect::<HashSet<_>>();
        anyhow::ensure!(
            decision
                .reference_ids
                .iter()
                .all(|id| reference_ids.contains(id.as_str())),
            "reference ID does not belong to this packet"
        );
        let grouped = packet["kind"] == "case";
        if decision.action == "approve" {
            anyhow::ensure!(
                grouped,
                "ungrouped messages must be linked to a case before historical publication"
            );
            anyhow::ensure!(
                decision.value_status == "valuable",
                "mark an incident valuable before approval"
            );
            anyhow::ensure!(
                matches!(decision.outcome.as_str(), "confirmed" | "team_fixed"),
                "only confirmed or team-fixed outcomes can publish historical guidance"
            );
        }
        if decision.action == "publish_guidance" {
            anyhow::ensure!(
                decision.value_status == "valuable",
                "mark guidance valuable before publication"
            );
            anyhow::ensure!(
                !decision.evidence_revision_ids.is_empty(),
                "cite the source problem message before publishing guidance"
            );
            for (label, value, limit) in [
                ("summary", &decision.summary, 500),
                ("guidance", &decision.guidance, 1_000),
                ("product", &decision.product, 80),
                ("failure domain", &decision.failure_domain, 80),
                ("category", &decision.category, 120),
            ] {
                anyhow::ensure!(
                    !value.trim().is_empty() && value.chars().count() <= limit,
                    "{label} is required and must be at most {limit} characters"
                );
                anyhow::ensure!(
                    !value.to_ascii_lowercase().contains("http://")
                        && !value.to_ascii_lowercase().contains("https://"),
                    "published guidance cannot contain links"
                );
                anyhow::ensure!(
                    !extract_entity_candidates(value)
                        .iter()
                        .any(|entity| matches!(
                            entity.entity_type,
                            "address_candidate" | "transaction_signature_candidate"
                        )),
                    "published guidance cannot contain wallet addresses or signatures"
                );
            }
        }
        let mut connection = self.connection()?;
        ensure_packet_matches_database(&connection, &packet)?;
        let source_fingerprint = packet_source_fingerprint(&connection, &packet)?;
        if grouped {
            let current: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM candidate_cases WHERE case_id=?1 AND corpus_id='support' AND is_current=1)", [id], |row| row.get(0))?;
            anyhow::ensure!(
                current,
                "case membership changed; rebuild packets before reviewing"
            );
            if decision.action == "approve" {
                let evidence = decision
                    .evidence_revision_ids
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                verify_resolution(
                    &connection,
                    id,
                    &decision.outcome,
                    &evidence,
                    &decision.rationale,
                    &decision.reviewer,
                )?;
                annotate_candidate(
                    &connection,
                    id,
                    &decision.product,
                    &decision.failure_domain,
                    &decision.summary,
                    &decision.resolution,
                )?;
                review_candidate(
                    &mut connection,
                    id,
                    "approve",
                    &decision.rationale,
                    &decision.reviewer,
                )?;
            } else if decision.action == "reject" {
                review_candidate(
                    &mut connection,
                    id,
                    "reject",
                    &decision.rationale,
                    &decision.reviewer,
                )?;
            } else {
                connection.execute(
                    "UPDATE candidate_cases SET review_status='candidate' WHERE case_id=?1",
                    [id],
                )?;
            }
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let evidence_json = serde_json::to_string(&decision.evidence_revision_ids)?;
        let reference_json = serde_json::to_string(&decision.reference_ids)?;
        let publication_status = if decision.action == "publish_guidance" {
            "published_guidance"
        } else {
            "private"
        };
        connection.execute("INSERT INTO corpus_packet_reviews(packet_id,evidence_fingerprint,value_status,outcome,category,diagnosis,summary,resolution,guidance,product,failure_domain,evidence_json,rationale,reviewer,updated_at,reference_ids_json,publication_status,packet_kind,source_fingerprint)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
            ON CONFLICT(packet_id) DO UPDATE SET evidence_fingerprint=excluded.evidence_fingerprint,value_status=excluded.value_status,outcome=excluded.outcome,category=excluded.category,diagnosis=excluded.diagnosis,summary=excluded.summary,resolution=excluded.resolution,guidance=excluded.guidance,product=excluded.product,failure_domain=excluded.failure_domain,evidence_json=excluded.evidence_json,rationale=excluded.rationale,reviewer=excluded.reviewer,updated_at=excluded.updated_at,reference_ids_json=excluded.reference_ids_json,publication_status=excluded.publication_status,packet_kind=excluded.packet_kind,source_fingerprint=excluded.source_fingerprint",
            params![id, hash, decision.value_status, decision.outcome, decision.category,
                decision.diagnosis, decision.summary, decision.resolution, decision.guidance,
                decision.product, decision.failure_domain, evidence_json, decision.rationale,
                decision.reviewer, now, reference_json, publication_status,
                if grouped { "case" } else { "orphan" }, source_fingerprint])?;
        connection.execute("INSERT INTO corpus_packet_review_events(packet_id,action,reviewer,decision_json,recorded_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, decision.action, decision.reviewer, serde_json::to_string(&decision)?, now])?;
        drop(connection);
        compile_registry_to_file(&self.database, &self.published)?;
        self.detail(id)
    }
}

fn review_priority(row: &ReviewRow) -> (u8, u8, u8, u8, &str) {
    let status = if row.review_status == "unreviewed" || row.review_status == "stale" {
        0
    } else {
        1
    };
    let kind = if row.kind == "case" { 0 } else { 1 };
    let outcome = match row.ai_outcome.as_deref() {
        Some("historical_resolution") => 0,
        Some("general_guidance") => 1,
        Some("open") => 2,
        _ if row.ai_status.as_deref() == Some("invalid_output") => 3,
        _ => 4,
    };
    let tier = match row.ai_tier.as_deref() {
        Some("reporter_confirmed") => 0,
        Some("team_fixed") => 1,
        Some("proposed_only") => 2,
        _ => 3,
    };
    (status, kind, outcome, tier, &row.id)
}

fn ensure_packet_matches_database(connection: &Connection, packet: &Value) -> anyhow::Result<()> {
    let packet_messages = packet["messages"]
        .as_array()
        .ok_or_else(|| anyhow!("packet messages missing"))?;
    let mut source = HashMap::new();
    if packet["kind"] == "case" {
        for message in messages(connection, string_field(packet, "case_id")?)? {
            source.insert(message.revision_id, (message.sender, message.body));
        }
    } else {
        let revision = packet_messages
            .first()
            .and_then(|message| message["revision_id"].as_i64())
            .ok_or_else(|| anyhow!("orphan revision missing"))?;
        let record: Option<(Option<String>, String)> = connection.query_row(
            "SELECT m.sender,m.body FROM message_revisions m JOIN source_files s ON s.source_file_id=m.source_file_id
             WHERE m.revision_id=?1 AND m.corpus_id='support' AND s.source_file_id=(SELECT MAX(latest.source_file_id) FROM source_files latest WHERE latest.corpus_id=s.corpus_id AND latest.file_name=s.file_name)",
            [revision], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        if let Some(record) = record {
            source.insert(revision, record);
        }
        let grouped: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM candidate_case_messages cm JOIN candidate_cases c ON c.case_id=cm.case_id WHERE cm.revision_id=?1 AND c.is_current=1)", [revision], |row| row.get(0))?;
        anyhow::ensure!(
            !grouped,
            "message membership changed; rebuild packets before reviewing"
        );
    }
    anyhow::ensure!(
        source.len() == packet_messages.len(),
        "source messages changed; rebuild packets before reviewing"
    );
    for message in packet_messages {
        let revision = message["revision_id"]
            .as_i64()
            .ok_or_else(|| anyhow!("packet revision missing"))?;
        let (sender, body) = source
            .get(&revision)
            .ok_or_else(|| anyhow!("case membership changed; rebuild packets before reviewing"))?;
        anyhow::ensure!(
            message["body"].as_str() == Some(body.as_str())
                && message["sender"].as_str() == sender.as_deref(),
            "source messages changed; rebuild packets before reviewing"
        );
    }
    Ok(())
}

fn packet_source_fingerprint(connection: &Connection, packet: &Value) -> anyhow::Result<String> {
    if packet["kind"] == "case" {
        return Ok(fingerprint(&messages(
            connection,
            string_field(packet, "case_id")?,
        )?));
    }
    let revision = packet["messages"]
        .as_array()
        .and_then(|messages| messages.first())
        .and_then(|message| message["revision_id"].as_i64())
        .ok_or_else(|| anyhow!("orphan revision missing"))?;
    let message = connection.query_row(
        "SELECT m.revision_id,m.sender,m.body FROM message_revisions m JOIN source_files s ON s.source_file_id=m.source_file_id
         WHERE m.revision_id=?1 AND m.corpus_id='support' AND s.source_file_id=(SELECT MAX(latest.source_file_id) FROM source_files latest WHERE latest.corpus_id=s.corpus_id AND latest.file_name=s.file_name)",
        [revision],
        |row| {
            Ok(ResolutionMessage {
                revision_id: row.get(0)?,
                sender: row.get(1)?,
                body: row.get(2)?,
            })
        },
    )?;
    Ok(fingerprint(&[message]))
}

fn guidance_source_current(
    connection: &Connection,
    id: &str,
    decision: &Value,
) -> anyhow::Result<bool> {
    let expected = decision["source_fingerprint"].as_str().unwrap_or("");
    let kind = decision["packet_kind"].as_str().unwrap_or("");
    let source = if kind == "case" {
        let current: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM candidate_cases WHERE case_id=?1 AND is_current=1)",
            [id],
            |r| r.get(0),
        )?;
        if !current {
            return Ok(false);
        }
        messages(connection, id)?
    } else if kind == "orphan" {
        let revision = match decision["evidence_revision_ids"]
            .as_array()
            .and_then(|ids| ids.first())
            .and_then(Value::as_i64)
        {
            Some(id) => id,
            None => return Ok(false),
        };
        let grouped: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM candidate_case_messages cm JOIN candidate_cases c ON c.case_id=cm.case_id WHERE cm.revision_id=?1 AND c.is_current=1)", [revision], |r| r.get(0))?;
        if grouped {
            return Ok(false);
        }
        connection.query_row("SELECT m.revision_id,m.sender,m.body FROM message_revisions m JOIN source_files s ON s.source_file_id=m.source_file_id WHERE m.revision_id=?1 AND m.corpus_id='support' AND s.source_file_id=(SELECT MAX(latest.source_file_id) FROM source_files latest WHERE latest.corpus_id=s.corpus_id AND latest.file_name=s.file_name)", [revision], |r| Ok(ResolutionMessage { revision_id:r.get(0)?, sender:r.get(1)?, body:r.get(2)? })).optional()?.into_iter().collect()
    } else {
        return Ok(false);
    };
    Ok(!source.is_empty() && fingerprint(&source) == expected)
}

fn string_field<'a>(value: &'a Value, field: &str) -> anyhow::Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| anyhow!("packet missing {field}"))
}

fn jsonl_values(path: &Path) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Value>>> {
    let file = fs::File::open(path)
        .with_context(|| format!("cannot open private review file {}", path.display()))?;
    Ok(BufReader::new(file)
        .lines()
        .filter(|line| line.as_ref().map_or(true, |s| !s.is_empty()))
        .map(|line| Ok(serde_json::from_str(&line?)?)))
}

fn jsonl_map(path: &Path) -> anyhow::Result<HashMap<String, Value>> {
    if !path.is_file() {
        return Ok(HashMap::new());
    }
    let mut result = HashMap::new();
    for item in jsonl_values(path)? {
        let item = item?;
        if let Some(id) = item["case_id"].as_str() {
            result.insert(id.to_owned(), item);
        }
    }
    Ok(result)
}

fn find_packet(path: &Path, id: &str) -> anyhow::Result<Value> {
    jsonl_values(path)?
        .find_map(|item| match item {
            Ok(packet) if packet["case_id"] == id => Some(Ok(packet)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .ok_or_else(|| anyhow!("prepared packet not found: {id}"))?
}

fn case_status_map(connection: &Connection) -> anyhow::Result<HashMap<String, String>> {
    let mut statement = connection.prepare("SELECT case_id,review_status FROM candidate_cases WHERE corpus_id='support' AND is_current=1")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
}

fn decision_map(connection: &Connection) -> anyhow::Result<HashMap<String, Value>> {
    let mut statement = connection.prepare("SELECT packet_id,evidence_fingerprint,value_status,outcome,category,diagnosis,summary,resolution,guidance,product,failure_domain,evidence_json,rationale,reviewer,updated_at,reference_ids_json,publication_status,packet_kind,source_fingerprint FROM corpus_packet_reviews")?;
    let rows = statement.query_map([], |row| {
        let ids: String = row.get(11)?;
        let references: String = row.get(15)?;
        Ok((row.get::<_, String>(0)?, json!({
            "evidence_fingerprint":row.get::<_, String>(1)?, "value_status":row.get::<_, String>(2)?,
            "outcome":row.get::<_, String>(3)?, "category":row.get::<_, String>(4)?,
            "diagnosis":row.get::<_, String>(5)?, "summary":row.get::<_, String>(6)?,
            "resolution":row.get::<_, String>(7)?, "guidance":row.get::<_, String>(8)?,
            "product":row.get::<_, String>(9)?, "failure_domain":row.get::<_, String>(10)?,
            "evidence_revision_ids":serde_json::from_str::<Value>(&ids).unwrap_or(json!([])),
            "reference_ids":serde_json::from_str::<Value>(&references).unwrap_or(json!([])),
            "publication_status":row.get::<_, String>(16)?,
            "packet_kind":row.get::<_, String>(17)?,
            "source_fingerprint":row.get::<_, String>(18)?,
            "rationale":row.get::<_, String>(12)?, "reviewer":row.get::<_, String>(13)?,
            "updated_at":row.get::<_, i64>(14)?,
        })))
    })?;
    Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_corpus_review_and_publication_preserve_the_boundary() {
        let root = std::env::temp_dir().join(format!(
            "raydium-review-ui-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let store = ReviewStore {
            database: root.join("source.sqlite"),
            packets: root.join("packets.jsonl"),
            drafts: root.join("drafts.jsonl"),
            published: root.join("incidents.json"),
        };
        let connection = Connection::open(&store.database).unwrap();
        initialize_schema(&connection).unwrap();
        connection.execute("INSERT INTO source_files(corpus_id,file_name,sha256,imported_at) VALUES ('support','messages.html','test',1)", []).unwrap();
        for (revision, sender, body) in [
            (1, "reporter", "Swap failed"),
            (2, "team", "We fixed the swap issue"),
            (3, "reporter", "How do I find my pool?"),
        ] {
            connection.execute("INSERT INTO message_revisions(revision_id,corpus_id,source_file_id,source_message_id,sender,date_title,body,media_refs_json) VALUES (?1,'support',1,?2,?3,'2024-01-01',?4,'[]')",
                params![revision, revision.to_string(), sender, body]).unwrap();
        }
        connection.execute("INSERT INTO candidate_cases(case_id,corpus_id,review_status,is_current,created_at,updated_at) VALUES ('case-test','support','candidate',1,1,1)", []).unwrap();
        for revision in [1, 2] {
            connection.execute("INSERT INTO candidate_case_messages(case_id,revision_id,link_reason) VALUES ('case-test',?1,'reply')", [revision]).unwrap();
        }
        drop(connection);
        let packets = [
            json!({"case_id":"case-test","kind":"case","evidence_fingerprint":"case-hash","messages":[
                {"revision_id":1,"sender":"reporter","body":"Swap failed","date":"2024-01-01","source_message_id":"1","attachments":[]},
                {"revision_id":2,"sender":"team","body":"We fixed the swap issue","date":"2024-01-01","source_message_id":"2","attachments":[]}
            ],"references":[],"upgrade_context":[]}),
            json!({"case_id":"orphan-test","kind":"orphan","evidence_fingerprint":"orphan-hash","messages":[
                {"revision_id":3,"sender":"reporter","body":"How do I find my pool?","date":"2024-01-01","source_message_id":"3","attachments":[]}
            ],"references":[],"upgrade_context":[]}),
        ];
        fs::write(
            &store.packets,
            packets
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let list = store.list(ReviewQuery::default()).unwrap();
        assert_eq!(
            (list.packet_count, list.case_count, list.orphan_count),
            (2, 1, 1)
        );
        let mut decision = ReviewDecision {
            evidence_fingerprint: "case-hash".into(),
            value_status: "valuable".into(),
            outcome: "team_fixed".into(),
            category: "swap".into(),
            diagnosis: "Swap failed".into(),
            summary: "Swap failed in the interface".into(),
            resolution: "The team stated that the swap issue was fixed.".into(),
            guidance: String::new(),
            product: "Swap".into(),
            failure_domain: "interface".into(),
            evidence_revision_ids: vec![2],
            reference_ids: vec![],
            rationale: "Team explicitly stated the fix; reporter did not confirm.".into(),
            reviewer: "test reviewer".into(),
            action: "approve".into(),
        };
        let detail = store.submit("case-test", decision.clone()).unwrap();
        assert_eq!(detail.review_status, "approved");
        let published: Value =
            serde_json::from_slice(&fs::read(&store.published).unwrap()).unwrap();
        assert_eq!(published["incidents"].as_array().unwrap().len(), 1);
        assert!(!published.to_string().contains("reporter"));
        decision.evidence_fingerprint = "orphan-hash".into();
        decision.evidence_revision_ids = vec![3];
        assert!(store.submit("orphan-test", decision.clone()).is_err());
        decision.action = "save".into();
        decision.outcome = "unknown".into();
        decision.value_status = "uncertain".into();
        let orphan = store.submit("orphan-test", decision.clone()).unwrap();
        assert_eq!(orphan.review_status, "reviewed");
        let listing = store
            .list(ReviewQuery {
                status: Some("reviewed".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(listing.filtered_count, 1);
        decision.action = "publish_guidance".into();
        decision.value_status = "valuable".into();
        decision.summary = "Finding a pool".into();
        decision.guidance = "Search the pool address in the Raydium interface.".into();
        let guidance = store.submit("orphan-test", decision).unwrap();
        assert_eq!(guidance.review_status, "published_guidance");
        let published: Value =
            serde_json::from_slice(&fs::read(&store.published).unwrap()).unwrap();
        assert_eq!(published["guidance"].as_array().unwrap().len(), 1);
        assert!(!published.to_string().contains("How do I find my pool?"));
        assert!(!published.to_string().contains("reporter"));
        let changed = ReviewDecision {
            evidence_fingerprint: "case-hash".into(),
            value_status: "uncertain".into(),
            outcome: "unknown".into(),
            category: String::new(),
            diagnosis: String::new(),
            summary: String::new(),
            resolution: String::new(),
            guidance: String::new(),
            product: String::new(),
            failure_domain: String::new(),
            evidence_revision_ids: vec![],
            reference_ids: vec![],
            rationale: "needs another look".into(),
            reviewer: "test reviewer".into(),
            action: "save".into(),
        };
        store.submit("case-test", changed).unwrap();
        let published: Value =
            serde_json::from_slice(&fs::read(&store.published).unwrap()).unwrap();
        assert!(published["incidents"].as_array().unwrap().is_empty());
        let connection = Connection::open(&store.database).unwrap();
        connection
            .execute(
                "UPDATE message_revisions SET body='Changed orphan' WHERE revision_id=3",
                [],
            )
            .unwrap();
        compile_registry_to_file(&store.database, &store.published).unwrap();
        let published: Value =
            serde_json::from_slice(&fs::read(&store.published).unwrap()).unwrap();
        assert!(published["guidance"].as_array().unwrap().is_empty());
        connection
            .execute(
                "UPDATE message_revisions SET body='Changed' WHERE revision_id=2",
                [],
            )
            .unwrap();
        let stale = store.detail("case-test").unwrap();
        assert_eq!(stale.review_status, "stale");
        let retry = ReviewDecision {
            evidence_fingerprint: "case-hash".into(),
            value_status: "valuable".into(),
            outcome: "team_fixed".into(),
            category: String::new(),
            diagnosis: String::new(),
            summary: "Swap failed".into(),
            resolution: "Team said it was fixed.".into(),
            guidance: String::new(),
            product: "Swap".into(),
            failure_domain: "interface".into(),
            evidence_revision_ids: vec![2],
            reference_ids: vec![],
            rationale: "checked".into(),
            reviewer: "test reviewer".into(),
            action: "approve".into(),
        };
        assert!(store
            .submit("case-test", retry)
            .unwrap_err()
            .to_string()
            .contains("source messages changed"));
        drop(connection);
        assert!(root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(root).unwrap();
    }
}
