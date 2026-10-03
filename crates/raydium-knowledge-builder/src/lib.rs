//! Private archive builder and operator commands. No runtime shell depends on this crate.
#![warn(missing_docs)]
mod enrichment;
mod evaluation;
mod review_pack;
use enrichment::*;
mod candidates;
use candidates::*;
mod review;
use review::*;
mod search;
use search::*;
mod persistence;
use persistence::*;
mod parsing;
use parsing::*;
mod migrations;
use migrations::*;
mod replies;
use replies::*;
mod entities;
use entities::*;
mod html;
use anyhow::{anyhow, bail, Context};
use html::*;
use petgraph::graph::UnGraph;
use raydium_debugger::{run_diagnostic_request_blocking, DebugDataMode, DebugRequest, RpcCluster};
use rusqlite::{params, Connection, OptionalExtension};
use scraper::{ElementRef, Html, Selector};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tantivy::{
    collector::TopDocs,
    doc,
    query::{BooleanQuery, Occur, Query, QueryParser, TermQuery},
    schema::{IndexRecordOption, Schema, Term, Value, STORED, STRING, TEXT},
    Index, TantivyDocument,
};

const DEFAULT_DATABASE_PATH: &str = ".raydium-debugger/support-knowledge.sqlite";
const DEFAULT_INDEX_PATH: &str = ".raydium-debugger/support-knowledge-index";
const DEFAULT_KNOWLEDGE_PATH: &str = "knowledge/incidents.generated.json";
const SCHEMA_VERSION: i64 = 10;

const ENTITY_EXTRACTION_VERSION: i64 = 1;
const SEARCH_INDEX_VERSION: u32 = 1;
const MAX_SEARCH_RESULTS: usize = 100;
const MAX_MESSAGE_PREVIEW_CHARS: usize = 500;
const MAX_ENRICHMENT_SIGNATURES: usize = 5;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct SearchIndexManifest {
    schema_version: u32,
    database_revision: i64,
}

#[derive(Debug, PartialEq)]
struct SearchHit {
    score: f32,
    corpus_id: String,
    file_name: String,
    source_message_id: String,
    sender: String,
    date_title: String,
    body: String,
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct EntityCandidate {
    entity_type: &'static str,
    canonical_value: String,
    surface_form: String,
    decoded_length: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
struct EntityHit {
    entity_type: String,
    canonical_value: String,
    surface_form: String,
    decoded_length: Option<i64>,
    corpus_id: String,
    file_name: String,
    source_message_id: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedHtml {
    messages: Vec<ParsedMessage>,
    skipped_records: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedMessage {
    source_message_id: String,
    sender: Option<String>,
    date_title: Option<String>,
    body: String,
    reply_to_message_id: Option<String>,
    media_refs: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ImportResult {
    imported: bool,
    stored_messages: usize,
    skipped_records: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ReplyResolutionStats {
    total: usize,
    resolved: usize,
    missing: usize,
    ambiguous: usize,
}

#[derive(Debug)]
struct ActiveMessage {
    revision_id: i64,
    corpus_id: String,
    source_message_id: String,
    reply_to_message_id: Option<String>,
}

#[derive(Debug, Clone)]
struct CandidateMessage {
    revision_id: i64,
    corpus_id: String,
    file_name: String,
    source_message_id: String,
}

#[derive(Debug, PartialEq, Eq)]
struct CandidateCase {
    case_id: String,
    corpus_id: String,
    status: String,
    is_current: bool,
    message_count: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CandidateBuildStats {
    current_messages: usize,
    resolved_reply_edges: usize,
    candidate_cases: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct EnrichmentStats {
    signatures_found: usize,
    fetched: usize,
    cached: usize,
    errors: usize,
}

struct TransactionSnapshotWrite<'a> {
    signature: &'a str,
    cluster: RpcCluster,
    source_revision_id: i64,
    captured_at: i64,
    observation_status: &'a str,
    diagnostic_json: Option<&'a str>,
    error: Option<&'a str>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
struct GeneratedRegistry {
    schema_version: u32,
    source_revision: i64,
    incidents: Vec<GeneratedIncident>,
}

type GeneratedIncident = raydium_knowledge::CuratedIncident;

/// Runs the existing support-knowledge operator command grammar; performs blocking I/O.
pub fn run(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        Some("review-pack") => {
            anyhow::ensure!((2..=3).contains(&args.len()), "usage: support-knowledge review-pack <private-directory> [database]");
            review_pack::prepare(&candidate_database_arg(args.get(2))?, Path::new(&args[1]))
        }
        Some("evaluate-review-pack") => {
            anyhow::ensure!(args.len() == 3, "usage: support-knowledge evaluate-review-pack <pack.json> <sanitized-artifact.json>");
            review_pack::evaluate(Path::new(&args[1]), Path::new(&args[2]))
        }

        Some("import-html") => {
            let export_dir = args
                .get(1)
                .ok_or_else(|| anyhow!("missing export directory"))?;
            if args.len() > 3 {
                bail!("usage: support-knowledge import-html <export-directory> [database-path]");
            }
            let database_path = args
                .get(2)
                .map(PathBuf::from)
                .unwrap_or_else(support_database_path);
            import_archive(Path::new(export_dir), &database_path)
        }
        Some("stats") => {
            if args.len() > 2 {
                bail!("usage: support-knowledge stats [database-path]");
            }
            let database_path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(support_database_path);
            print_stats(&database_path)
        }
        Some("validate") => {
            if args.len() > 2 {
                bail!("usage: support-knowledge validate [database-path]");
            }
            let database_path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(support_database_path);
            validate_database(&database_path)
        }
        Some("compile") => {
            if args.len() > 3 {
                bail!("usage: support-knowledge compile [database-path] [output-path]");
            }
            let database_path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(support_database_path);
            let output_path = args
                .get(2)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_KNOWLEDGE_PATH));
            compile_registry_to_file(&database_path, &output_path)
        }
        Some("evaluate-matches") => {
            let path = args.get(1).context("usage: support-knowledge evaluate-matches <benchmark.json> [--require-reviewed]")?;
            let report = evaluation::evaluate_benchmark(Path::new(path))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.quality_gate || (args.iter().any(|arg| arg == "--require-reviewed") && !report.reviewed) { bail!("match benchmark release gate failed"); }
            Ok(())
        }
        Some("observations") => raydium_observability::ingestion::run(&args[1..]),
        Some("resolve-replies") => {
            if args.len() > 2 {
                bail!("usage: support-knowledge resolve-replies [database-path]");
            }
            let database_path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(support_database_path);
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            let stats = resolve_reply_edges(&mut connection)?;
            print_reply_stats(&stats);
            Ok(())
        }
        Some("extract-entities") => {
            if args.len() > 2 {
                bail!("usage: support-knowledge extract-entities [database-path]");
            }
            let database_path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(support_database_path);
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            let count = rebuild_entity_candidates(&mut connection)?;
            println!("Rebuilt {count} entity candidates.");
            Ok(())
        }
        Some("entity-search") => {
            let value = args
                .get(1)
                .ok_or_else(|| anyhow!("missing entity value"))?;
            let options = parse_entity_search_options(&args[2..])?;
            search_entities_and_print(value, &options)
        }
        Some("candidates") => run_candidates(&args[1..]),
        Some("index") => {
            let (database_path, index_path) = parse_index_paths(&args[1..])?;
            rebuild_search_index(&database_path, &index_path).map(|count| {
                println!("Indexed {count} current messages into {}", index_path.display());
            })
        }
        Some("search") => {
            let query = args.get(1).ok_or_else(|| anyhow!("missing search query"))?;
            let options = parse_search_options(&args[2..])?;
            search_and_print(query, &options)
        }
        _ => bail!(
            "usage: cargo run -p xtask -- support-knowledge <import-html <export-directory> [database-path] | stats [database-path] | validate [database-path] | resolve-replies [database-path] | extract-entities [database-path] | entity-search <value> [--corpus ID] [--limit N] [--database PATH] | candidates <rebuild [database-path] | list [database-path] | show <case-id> [database-path] | review <case-id> <approve|reject> <reason> [database-path]> | index [--database PATH] [--index PATH] | search <query> [--corpus ID] [--limit N] [--database PATH] [--index PATH]>"
        ),
    }
}

fn run_candidates(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        Some("curate") => {
            anyhow::ensure!((4..=5).contains(&args.len()), "usage: candidates curate <case-id> <annotation.json> <reviewer> [database]");
            let database=candidate_database_arg(args.get(4))?;
            let connection=open_existing_database(&database)?; initialize_schema(&connection)?;
            review_pack::curate(&connection,&args[1],Path::new(&args[2]),&args[3])
        }

        Some("rebuild") => {
            if args.len() > 2 {
                bail!("usage: support-knowledge candidates rebuild [database-path]");
            }
            let database_path = candidate_database_arg(args.get(1))?;
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            resolve_reply_edges(&mut connection)?;
            let stats = rebuild_candidate_cases(&mut connection)?;
            print_candidate_build_stats(&stats);
            Ok(())
        }
        Some("list") => {
            if args.len() > 3 {
                bail!("usage: support-knowledge candidates list [limit] [database-path]");
            }
            let limit = args
                .get(1)
                .map(|value| value.parse::<usize>())
                .transpose()
                .context("candidate list limit must be a positive integer")?
                .unwrap_or(30);
            if limit == 0 || limit > MAX_SEARCH_RESULTS {
                bail!("candidate list limit must be between 1 and {MAX_SEARCH_RESULTS}");
            }
            let database_path = candidate_database_arg(args.get(2))?;
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            resolve_reply_edges(&mut connection)?;
            rebuild_candidate_cases(&mut connection)?;
            for case in list_candidate_cases(&connection, limit)? {
                println!(
                    "{}\t{}\t{} messages\t{}",
                    case.case_id, case.corpus_id, case.message_count, case.status
                );
            }
            Ok(())
        }
        Some("signatures") => {
            if args.len() > 3 {
                bail!("usage: support-knowledge candidates signatures [limit] [database-path]");
            }
            let limit = args
                .get(1)
                .map(|value| value.parse::<usize>())
                .transpose()
                .context("candidate signature limit must be a positive integer")?
                .unwrap_or(30);
            if limit == 0 || limit > MAX_SEARCH_RESULTS {
                bail!("candidate signature limit must be between 1 and {MAX_SEARCH_RESULTS}");
            }
            let database_path = candidate_database_arg(args.get(2))?;
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            resolve_reply_edges(&mut connection)?;
            rebuild_candidate_cases(&mut connection)?;
            for (case_id, signature_count) in
                list_candidates_with_signatures(&connection, limit)?
            {
                println!("{case_id}\t{signature_count} signatures");
            }
            Ok(())
        }
        Some("show") => {
            let case_id = args
                .get(1)
                .ok_or_else(|| anyhow!("missing candidate case ID"))?;
            if args.len() > 3 {
                bail!("usage: support-knowledge candidates show <case-id> [database-path]");
            }
            let database_path = candidate_database_arg(args.get(2))?;
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            resolve_reply_edges(&mut connection)?;
            rebuild_candidate_cases(&mut connection)?;
            show_candidate_case(&mut connection, case_id)
        }
        Some("review") => {
            let case_id = args
                .get(1)
                .ok_or_else(|| anyhow!("missing candidate case ID"))?;
            let action = args.get(2).ok_or_else(|| anyhow!("missing review action"))?;
            let rationale = args.get(3).ok_or_else(|| anyhow!("missing review rationale"))?;
            if args.len() > 6 || !matches!(action.as_str(), "approve" | "reject") {
                bail!("usage: support-knowledge candidates review <case-id> <approve|reject> <reason> [database-path]");
            }
            let database_path = candidate_database_arg(args.get(4))?;
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            resolve_reply_edges(&mut connection)?;
            rebuild_candidate_cases(&mut connection)?;
            review_candidate(&mut connection, case_id, action, rationale, args.get(5).map(String::as_str).unwrap_or("local"))
        }
        Some("annotate") => {
            if args.len() < 6 || args.len() > 7 {
                bail!("usage: support-knowledge candidates annotate <case-id> <product> <domain> <summary> <resolution> [database-path]");
            }
            let database_path = candidate_database_arg(args.get(6))?;
            let connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            annotate_candidate(
                &connection,
                &args[1],
                &args[2],
                &args[3],
                &args[4],
                &args[5],
            )
        }
        Some("enrich") => {
            let case_id = args
                .get(1)
                .ok_or_else(|| anyhow!("missing candidate case ID"))?;
            let EnrichmentOptions { cluster, limit, database_path } = parse_enrichment_options(&args[2..])?;
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            resolve_reply_edges(&mut connection)?;
            rebuild_candidate_cases(&mut connection)?;
            let stats = enrich_candidate(&mut connection, case_id, cluster, limit)?;
            println!(
                "Enrichment: {} signatures found, {} fetched, {} cached, {} errors.",
                stats.signatures_found, stats.fetched, stats.cached, stats.errors
            );
            Ok(())
        }
        Some("merge") => {
            if args.len() < 4 || args.len() > 5 {
                bail!("usage: support-knowledge candidates merge <case-id> <case-id> <reason> [database-path]");
            }
            let database_path = candidate_database_arg(args.get(4))?;
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            resolve_reply_edges(&mut connection)?;
            rebuild_candidate_cases(&mut connection)?;
            merge_candidate_cases(
                &mut connection,
                &args[1],
                &args[2],
                &args[3],
                "local",
            )
        }
        Some("split") => {
            if args.len() < 4 || args.len() > 5 {
                bail!("usage: support-knowledge candidates split <case-id> <message-id,...> <reason> [database-path]");
            }
            let database_path = candidate_database_arg(args.get(4))?;
            let mut connection = open_existing_database(&database_path)?;
            initialize_schema(&connection)?;
            resolve_reply_edges(&mut connection)?;
            rebuild_candidate_cases(&mut connection)?;
            let message_ids = args[2]
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect::<HashSet<_>>();
            split_candidate_case(
                &mut connection,
                &args[1],
                &message_ids,
                &args[3],
                "local",
            )
        }
        _ => bail!("usage: support-knowledge candidates <rebuild|list|signatures|show|annotate|review|enrich|merge|split> ..."),
    }
}
fn candidate_database_arg(value: Option<&String>) -> anyhow::Result<PathBuf> {
    Ok(value
        .map(PathBuf::from)
        .unwrap_or_else(support_database_path))
}

fn support_database_path() -> PathBuf {
    std::env::var_os("RAYDIUM_DEBUGGER_SUPPORT_DATABASE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATABASE_PATH))
}

#[cfg(test)]
mod tests;
