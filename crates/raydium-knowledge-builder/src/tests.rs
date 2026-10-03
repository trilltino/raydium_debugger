//! Archive compatibility and review regressions.
use super::*;

const HTML: &str = r##"
        <div class="message service" id="message-1"><div class="body details">joined</div></div>
        <div class="message default clearfix" id="message12">
            <div class="date details" title="15.02.2021 16:27:30 UTC+00:00">16:27</div>
            <div class="from_name">Example User</div>
            <div class="reply_to details">In reply to <a href="#go_to_message7">this message</a></div>
            <div class="text">Hello <a href="https://example.com">there</a></div>
        </div>
        <div class="message default clearfix" id="message13"><div class="from_name">Deleted Account</div></div>
    "##;

#[test]
fn parses_message_text_dates_and_reply_targets() {
    let parsed = parse_html(HTML).unwrap();
    assert_eq!(parsed.messages.len(), 1);
    assert_eq!(parsed.skipped_records, 2);
    assert_eq!(parsed.messages[0].source_message_id, "12");
    assert_eq!(parsed.messages[0].sender.as_deref(), Some("Example User"));
    assert_eq!(
        parsed.messages[0].date_title.as_deref(),
        Some("15.02.2021 16:27:30 UTC+00:00")
    );
    assert_eq!(parsed.messages[0].reply_to_message_id.as_deref(), Some("7"));
    assert_eq!(parsed.messages[0].body, "Hello there");
}

#[test]
fn repeated_import_is_idempotent_and_changed_source_is_a_new_revision() {
    let mut connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();

    let first =
        import_source(&mut connection, "support", "messages.html", HTML.as_bytes()).unwrap();
    let repeated =
        import_source(&mut connection, "support", "messages.html", HTML.as_bytes()).unwrap();
    let changed_html = HTML.replace("Hello", "Updated");
    let changed = import_source(
        &mut connection,
        "support",
        "messages.html",
        changed_html.as_bytes(),
    )
    .unwrap();

    assert!(first.imported);
    assert_eq!(first.stored_messages, 1);
    assert!(!repeated.imported);
    assert!(changed.imported);
    let revision_count: i64 = connection
        .query_row("SELECT count(*) FROM message_revisions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(revision_count, 2);
}

#[test]
fn search_filters_corpora_and_refreshes_after_source_revision() {
    let directory = std::env::temp_dir().join(format!(
        "support-search-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
    let database_path = directory.join("support.sqlite");
    let index_path = directory.join("index");
    let mut connection = Connection::open(&database_path).unwrap();
    initialize_schema(&connection).unwrap();

    let support_html = message_html("1", "pool visibility is broken: Custom(38)");
    let announcement_html = message_html("1", "pool custom release announcement");
    import_source(
        &mut connection,
        "support",
        "messages.html",
        support_html.as_bytes(),
    )
    .unwrap();
    import_source(
        &mut connection,
        "announcements",
        "messages_updates.html",
        announcement_html.as_bytes(),
    )
    .unwrap();
    drop(connection);

    let support_hits =
        search_messages(&database_path, &index_path, "pool", Some("support"), 10).unwrap();
    let all_hits = search_messages(&database_path, &index_path, "pool", None, 10).unwrap();
    assert_eq!(support_hits.len(), 1);
    assert_eq!(
        support_hits[0].body,
        "pool visibility is broken: Custom(38)"
    );
    assert_eq!(all_hits.len(), 2);
    let error_hits = search_messages(
        &database_path,
        &index_path,
        "Custom(38)",
        Some("support"),
        10,
    )
    .unwrap();
    assert_eq!(error_hits.len(), 1);
    let cross_corpus_error_hits =
        search_messages(&database_path, &index_path, "Custom(38)", None, 10).unwrap();
    assert_eq!(cross_corpus_error_hits.len(), 1);
    assert_eq!(cross_corpus_error_hits[0].corpus_id, "support");
    let custom_entity_hits =
        search_entities(&database_path, "Custom(38)", Some("support"), 10).unwrap();
    let hex_entity_hits = search_entities(&database_path, "0x26", Some("support"), 10).unwrap();
    assert_eq!(custom_entity_hits.len(), 1);
    assert_eq!(hex_entity_hits.len(), 1);
    assert_eq!(custom_entity_hits[0].canonical_value, "38");
    assert_eq!(hex_entity_hits[0].canonical_value, "38");

    let changed_html = message_html("1", "new pool indexing issue");
    let mut connection = Connection::open(&database_path).unwrap();
    import_source(
        &mut connection,
        "support",
        "messages.html",
        changed_html.as_bytes(),
    )
    .unwrap();
    drop(connection);

    let updated_hits = search_messages(
        &database_path,
        &index_path,
        "indexing issue",
        Some("support"),
        10,
    )
    .unwrap();
    let removed_hits = search_messages(
        &database_path,
        &index_path,
        "visibility broken",
        Some("support"),
        10,
    )
    .unwrap();
    assert_eq!(updated_hits.len(), 1);
    assert!(removed_hits.is_empty());
    let stale_entity_hits =
        search_entities(&database_path, "Custom(38)", Some("support"), 10).unwrap();
    assert!(stale_entity_hits.is_empty());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn search_preview_limits_long_pasted_logs() {
    let long_message = "a".repeat(MAX_MESSAGE_PREVIEW_CHARS + 20);
    let preview = message_preview(&long_message);
    assert_eq!(preview.chars().count(), MAX_MESSAGE_PREVIEW_CHARS + 3);
    assert!(preview.ends_with("..."));
}

#[test]
fn resolves_cross_page_replies_and_preserves_missing_or_ambiguous_targets() {
    let mut connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    import_source(
        &mut connection,
        "support",
        "messages.html",
        message_html("7", "original question").as_bytes(),
    )
    .unwrap();
    import_source(
        &mut connection,
        "support",
        "messages2.html",
        &[
            message_html_with_reply("8", "reply", Some("7")),
            message_html_with_reply("9", "missing target", Some("404")),
        ]
        .join("\n")
        .into_bytes(),
    )
    .unwrap();

    let first_pass = resolve_reply_edges(&mut connection).unwrap();
    assert_eq!(
        first_pass,
        ReplyResolutionStats {
            total: 2,
            resolved: 1,
            missing: 1,
            ambiguous: 0,
        }
    );

    import_source(
        &mut connection,
        "support",
        "messages3.html",
        message_html("7", "duplicate source message id").as_bytes(),
    )
    .unwrap();
    let second_pass = resolve_reply_edges(&mut connection).unwrap();
    assert_eq!(
        second_pass,
        ReplyResolutionStats {
            total: 2,
            resolved: 0,
            missing: 1,
            ambiguous: 1,
        }
    );
    let status: String = connection
            .query_row(
                "SELECT status FROM reply_edges
                 JOIN message_revisions ON message_revisions.revision_id = reply_edges.source_revision_id
                 WHERE message_revisions.source_message_id = '8'",
                [],
                |row| row.get(0),
            )
            .unwrap();
    assert_eq!(status, "ambiguous");
}

#[test]
fn candidate_groups_use_reply_edges_and_preserve_review_decisions() {
    let mut connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    let shared_address = bs58::encode([17u8; 32]).into_string();
    import_source(
        &mut connection,
        "support",
        "messages.html",
        message_html("7", &format!("question {shared_address}")).as_bytes(),
    )
    .unwrap();
    let second_page = [
        message_html_with_reply("8", "reply one", Some("7")),
        message_html_with_reply("9", "reply two", Some("8")),
        message_html("10", &format!("same address {shared_address}")),
        message_html("11", &format!("same address {shared_address}")),
    ]
    .join("\n");
    import_source(
        &mut connection,
        "support",
        "messages2.html",
        second_page.as_bytes(),
    )
    .unwrap();
    import_source(
        &mut connection,
        "announcements",
        "messages_updates.html",
        &[
            message_html("7", "release note"),
            message_html_with_reply("8", "announcement reply", Some("7")),
        ]
        .join("\n")
        .into_bytes(),
    )
    .unwrap();

    resolve_reply_edges(&mut connection).unwrap();
    let build = rebuild_candidate_cases(&mut connection).unwrap();
    assert_eq!(build.current_messages, 5);
    assert_eq!(build.resolved_reply_edges, 2);
    assert_eq!(build.candidate_cases, 1);

    let cases = list_candidate_cases(&connection, 50).unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].message_count, 3);
    assert_eq!(cases[0].status, "candidate");
    let case_id = cases[0].case_id.clone();
    let member_ids = {
        let mut statement = connection
                .prepare(
                    "SELECT message_revisions.source_message_id
                     FROM candidate_case_messages
                     JOIN message_revisions ON message_revisions.revision_id = candidate_case_messages.revision_id
                     WHERE candidate_case_messages.case_id = ?1
                     ORDER BY message_revisions.source_message_id",
                )
                .unwrap();
        statement
            .query_map([&case_id], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(member_ids, vec!["7", "8", "9"]);

    annotate_candidate(
        &connection,
        &case_id,
        "raydium",
        "support_process",
        "A user reported an issue in a reply thread.",
        "Ask for a transaction signature and confirm the selected cluster.",
    )
    .unwrap();
    review_candidate(
        &mut connection,
        &case_id,
        "approve",
        "reply chain describes one support case",
        "test-reviewer",
    )
    .unwrap();
    let rebuilt = rebuild_candidate_cases(&mut connection).unwrap();
    assert_eq!(rebuilt.candidate_cases, 1);
    let cases = list_candidate_cases(&connection, 50).unwrap();
    assert_eq!(cases[0].case_id, case_id);
    assert_eq!(cases[0].status, "approved");
    let registry = build_generated_registry(&connection).unwrap();
    assert_eq!(registry.incidents.len(), 1);
    assert_eq!(
        registry.incidents[0].summary,
        "A user reported an issue in a reply thread."
    );
    let generated_json = serde_json::to_string(&registry).unwrap();
    assert!(!generated_json.contains("original question"));
    assert!(!generated_json.contains("Example User"));
    let review_count: i64 = connection
        .query_row("SELECT count(*) FROM review_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(review_count, 1);
}

#[test]
fn manual_merge_and_split_survive_candidate_rebuilds() {
    let mut connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    import_source(
        &mut connection,
        "support",
        "messages.html",
        [
            message_html("1", "first thread root"),
            message_html("3", "second thread root"),
        ]
        .join("\n")
        .as_bytes(),
    )
    .unwrap();
    import_source(
        &mut connection,
        "support",
        "messages2.html",
        [
            message_html_with_reply("2", "first reply", Some("1")),
            message_html_with_reply("4", "second reply", Some("3")),
        ]
        .join("\n")
        .as_bytes(),
    )
    .unwrap();
    resolve_reply_edges(&mut connection).unwrap();
    rebuild_candidate_cases(&mut connection).unwrap();

    let initial_cases = list_candidate_cases(&connection, 10).unwrap();
    assert_eq!(initial_cases.len(), 2);
    let merged_case_id = {
        merge_candidate_cases(
            &mut connection,
            &initial_cases[0].case_id,
            &initial_cases[1].case_id,
            "same incident confirmed by reviewer",
            "reviewer",
        )
        .unwrap();
        let cases = list_candidate_cases(&connection, 10).unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].message_count, 4);
        assert!(cases[0].case_id.starts_with("manual-"));
        cases[0].case_id.clone()
    };

    split_candidate_case(
        &mut connection,
        &merged_case_id,
        &HashSet::from(["2".to_string(), "4".to_string()]),
        "separate unrelated reply threads",
        "reviewer",
    )
    .unwrap();
    rebuild_candidate_cases(&mut connection).unwrap();
    let split_cases = list_candidate_cases(&connection, 10).unwrap();
    assert_eq!(split_cases.len(), 2);
    assert!(split_cases.iter().all(|case| case.message_count == 2));
    assert!(split_cases
        .iter()
        .all(|case| case.case_id.starts_with("manual-")));
    let override_count: i64 = connection
        .query_row("SELECT count(*) FROM case_override_events", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(override_count, 2);
}

#[test]
fn transaction_snapshot_cache_is_scoped_by_signature_and_cluster() {
    let connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    let mut connection = connection;
    import_source(
        &mut connection,
        "support",
        "messages.html",
        message_html("1", "transaction example").as_bytes(),
    )
    .unwrap();
    let revision_id: i64 = connection
        .query_row(
            "SELECT revision_id FROM message_revisions LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    store_transaction_snapshot(
        &connection,
        TransactionSnapshotWrite {
            signature: "signature-example",
            cluster: RpcCluster::Mainnet,
            source_revision_id: revision_id,
            captured_at: 100,
            observation_status: "landed",
            diagnostic_json: Some("{\"ok\":true}"),
            error: None,
        },
    )
    .unwrap();
    store_transaction_snapshot(
        &connection,
        TransactionSnapshotWrite {
            signature: "signature-example",
            cluster: RpcCluster::Mainnet,
            source_revision_id: revision_id,
            captured_at: 200,
            observation_status: "not_observed_on_selected_provider",
            diagnostic_json: None,
            error: Some("not observed"),
        },
    )
    .unwrap();
    store_transaction_snapshot(
        &connection,
        TransactionSnapshotWrite {
            signature: "signature-example",
            cluster: RpcCluster::Devnet,
            source_revision_id: revision_id,
            captured_at: 300,
            observation_status: "landed",
            diagnostic_json: Some("{\"ok\":true}"),
            error: None,
        },
    )
    .unwrap();

    let count: i64 = connection
        .query_row("SELECT count(*) FROM transaction_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mainnet_status: String = connection
        .query_row(
            "SELECT observation_status FROM transaction_snapshots
                 WHERE signature = 'signature-example' AND cluster = 'mainnet'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(mainnet_status, "not_observed_on_selected_provider");
}

#[test]
fn extracts_only_expected_base58_lengths_and_normalizes_numeric_errors() {
    let signature = bs58::encode([42u8; 64]).into_string();
    let address = bs58::encode([7u8; 32]).into_string();
    let invalid_length = bs58::encode([9u8; 31]).into_string();
    let text = format!(
            "signature {signature}, address {address}, not-address {invalid_length}; Custom(38), custom program error: 0x26"
        );

    let entities = extract_entity_candidates(&text);
    assert!(entities.iter().any(|entity| {
        entity.entity_type == "transaction_signature_candidate"
            && entity.canonical_value == signature
            && entity.decoded_length == Some(64)
    }));
    assert!(entities.iter().any(|entity| {
        entity.entity_type == "address_candidate"
            && entity.canonical_value == address
            && entity.decoded_length == Some(32)
    }));
    assert!(!entities.iter().any(|entity| {
        entity.canonical_value == invalid_length
            && matches!(
                entity.entity_type,
                "transaction_signature_candidate" | "address_candidate"
            )
    }));
    let error_forms = entities
        .iter()
        .filter(|entity| entity.entity_type == "numeric_error_candidate")
        .collect::<Vec<_>>();
    assert_eq!(error_forms.len(), 2);
    assert!(error_forms
        .iter()
        .all(|entity| entity.canonical_value == "38"));
    assert_eq!(canonical_entity_query("Custom(38)"), "38");
    assert_eq!(canonical_entity_query("0x26"), "38");
}

fn message_html(message_id: &str, body: &str) -> String {
    format!(
        r#"<div class="message default clearfix" id="message{message_id}">
                <div class="date details" title="15.02.2021 16:27:30 UTC+00:00">16:27</div>
                <div class="from_name">Example User</div>
                <div class="text">{body}</div>
            </div>"#
    )
}

fn message_html_with_reply(message_id: &str, body: &str, reply_to: Option<&str>) -> String {
    let reply_markup = reply_to
            .map(|target| {
                format!(
                    "<div class=\"reply_to details\"><a href=\"#go_to_message{target}\">reply</a></div>"
                )
            })
            .unwrap_or_default();
    format!(
        r#"<div class="message default clearfix" id="message{message_id}">
                <div class="date details" title="15.02.2021 16:27:30 UTC+00:00">16:27</div>
                <div class="from_name">Example User</div>
                {reply_markup}
                <div class="text">{body}</div>
            </div>"#
    )
}
