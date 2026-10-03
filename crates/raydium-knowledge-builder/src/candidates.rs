//! Private candidates operations; blocking operator tooling.
use super::*;
pub(super) fn load_case_members(
    connection: &Connection,
    case_id: &str,
) -> anyhow::Result<Vec<CandidateMessage>> {
    let mut statement = connection.prepare(
        "SELECT message_revisions.revision_id, message_revisions.corpus_id,
                source_files.file_name, message_revisions.source_message_id
         FROM candidate_case_messages
         JOIN message_revisions ON message_revisions.revision_id = candidate_case_messages.revision_id
         JOIN source_files ON source_files.source_file_id = message_revisions.source_file_id
         WHERE candidate_case_messages.case_id = ?1
         ORDER BY message_revisions.revision_id",
    )?;
    let rows = statement.query_map([case_id], |row| {
        Ok(CandidateMessage {
            revision_id: row.get(0)?,
            corpus_id: row.get(1)?,
            file_name: row.get(2)?,
            source_message_id: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn merge_candidate_cases(
    connection: &mut Connection,
    first_case_id: &str,
    second_case_id: &str,
    rationale: &str,
    reviewer: &str,
) -> anyhow::Result<()> {
    if first_case_id == second_case_id {
        bail!("choose two different candidate cases to merge");
    }
    let first_members = load_case_members(connection, first_case_id)?;
    let second_members = load_case_members(connection, second_case_id)?;
    if first_members.is_empty() || second_members.is_empty() {
        bail!("both cases must be current and contain messages");
    }
    let corpus_id = first_members[0].corpus_id.as_str();
    if first_members
        .iter()
        .chain(&second_members)
        .any(|message| message.corpus_id != corpus_id)
    {
        bail!("cases from different corpora cannot be merged");
    }
    let mut merged = first_members
        .into_iter()
        .chain(second_members)
        .map(|message| (candidate_message_identity(&message), message))
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_values()
        .collect::<Vec<_>>();
    merged.sort_by_key(candidate_message_identity);
    persist_manual_groups(
        connection,
        "merge",
        &[first_case_id.to_string(), second_case_id.to_string()],
        vec![merged],
        rationale,
        reviewer,
    )
}

pub(super) fn split_candidate_case(
    connection: &mut Connection,
    case_id: &str,
    moved_message_ids: &HashSet<String>,
    rationale: &str,
    reviewer: &str,
) -> anyhow::Result<()> {
    if moved_message_ids.is_empty() {
        bail!("provide at least one message ID to move into the new case");
    }
    let members = load_case_members(connection, case_id)?;
    if members.len() < 2 {
        bail!("a candidate needs at least two messages to split");
    }
    let matching_ids = members
        .iter()
        .filter(|message| moved_message_ids.contains(&message.source_message_id))
        .map(|message| message.source_message_id.clone())
        .collect::<HashSet<_>>();
    if matching_ids.len() != moved_message_ids.len() {
        bail!("all split message IDs must belong to the selected candidate");
    }
    if matching_ids.len() == members.len() {
        bail!("split must leave at least one message in each case");
    }
    let (mut retained, mut moved): (Vec<_>, Vec<_>) = members
        .into_iter()
        .partition(|message| !moved_message_ids.contains(&message.source_message_id));
    retained.sort_by_key(candidate_message_identity);
    moved.sort_by_key(candidate_message_identity);
    persist_manual_groups(
        connection,
        "split",
        &[case_id.to_string()],
        vec![retained, moved],
        rationale,
        reviewer,
    )
}

pub(super) fn persist_manual_groups(
    connection: &mut Connection,
    action: &str,
    source_case_ids: &[String],
    groups: Vec<Vec<CandidateMessage>>,
    rationale: &str,
    reviewer: &str,
) -> anyhow::Result<()> {
    if rationale.trim().is_empty() {
        bail!("override rationale must not be empty");
    }
    if !matches!(action, "merge" | "split") {
        bail!("unsupported candidate override action");
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs() as i64;
    let target_case_ids = groups
        .iter()
        .map(|group| manual_case_id(group))
        .collect::<Vec<_>>();
    let source_case_ids_json = serde_json::to_string(source_case_ids)?;
    let target_case_ids_json = serde_json::to_string(&target_case_ids)?;
    let transaction = connection.transaction()?;

    for source_case_id in source_case_ids {
        let changed = transaction.execute(
            "UPDATE candidate_cases SET is_current = 0, updated_at = ?1
             WHERE case_id = ?2 AND is_current = 1",
            params![timestamp, source_case_id],
        )?;
        if changed == 0 {
            bail!("current candidate case not found: {source_case_id}");
        }
    }

    for group in &groups {
        if group.is_empty() {
            bail!("manual candidate groups must not be empty");
        }
        let corpus_id = group[0].corpus_id.as_str();
        if group.iter().any(|message| message.corpus_id != corpus_id) {
            bail!("manual candidate cannot span corpora");
        }
        let case_id = manual_case_id(group);
        transaction.execute(
            "INSERT INTO candidate_cases (
                case_id, corpus_id, review_status, is_current, created_at, updated_at, case_origin
            ) VALUES (?1, ?2, 'candidate', 1, ?3, ?3, 'manual')
            ON CONFLICT(case_id) DO UPDATE SET
                is_current = 1,
                updated_at = excluded.updated_at",
            params![case_id, corpus_id, timestamp],
        )?;
        for message in group {
            transaction.execute(
                "DELETE FROM manual_case_memberships
                 WHERE corpus_id = ?1 AND file_name = ?2 AND source_message_id = ?3",
                params![
                    message.corpus_id,
                    message.file_name,
                    message.source_message_id
                ],
            )?;
            transaction.execute(
                "INSERT INTO manual_case_memberships (
                    corpus_id, file_name, source_message_id, case_id, assigned_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    message.corpus_id,
                    message.file_name,
                    message.source_message_id,
                    case_id,
                    timestamp
                ],
            )?;
        }
    }
    transaction.execute(
        "INSERT INTO case_override_events (
            action, source_case_ids_json, target_case_ids_json,
            reviewer, rationale, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            action,
            source_case_ids_json,
            target_case_ids_json,
            reviewer,
            rationale.trim(),
            timestamp
        ],
    )?;
    transaction.commit()?;
    rebuild_candidate_cases(connection)?;
    println!(
        "Candidate {action} recorded: {}",
        target_case_ids.join(", ")
    );
    Ok(())
}

pub(super) fn manual_case_id(messages: &[CandidateMessage]) -> String {
    let generated = candidate_case_id(messages);
    format!("manual-{}", generated.trim_start_matches("case-"))
}

pub(super) fn rebuild_candidate_cases(
    connection: &mut Connection,
) -> anyhow::Result<CandidateBuildStats> {
    let messages = load_candidate_messages(connection)?;
    let manual_groups = load_manual_case_members(connection)?;
    let manually_assigned = manual_groups
        .values()
        .flatten()
        .map(|message| message.revision_id)
        .collect::<HashSet<_>>();
    let automatic_messages = messages
        .iter()
        .filter(|message| !manually_assigned.contains(&message.revision_id))
        .cloned()
        .collect::<Vec<_>>();
    let mut graph = UnGraph::<usize, ()>::new_undirected();
    let mut node_by_revision = HashMap::new();
    for (index, message) in automatic_messages.iter().enumerate() {
        node_by_revision.insert(message.revision_id, graph.add_node(index));
    }

    let edges = {
        let mut statement = connection.prepare(
            "SELECT source_revision_id, target_revision_id
             FROM reply_edges
             WHERE status = 'resolved' AND target_revision_id IS NOT NULL",
        )?;
        let rows =
            statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (source_revision_id, target_revision_id) in &edges {
        if let (Some(source), Some(target)) = (
            node_by_revision.get(source_revision_id),
            node_by_revision.get(target_revision_id),
        ) {
            graph.add_edge(*source, *target, ());
        }
    }

    let mut visited = HashSet::new();
    let mut groups = Vec::new();
    for start in graph.node_indices() {
        if !visited.insert(start) {
            continue;
        }
        let mut pending = vec![start];
        let mut member_indices = Vec::new();
        while let Some(node) = pending.pop() {
            if let Some(index) = graph.node_weight(node) {
                member_indices.push(*index);
            }
            for neighbor in graph.neighbors(node) {
                if visited.insert(neighbor) {
                    pending.push(neighbor);
                }
            }
        }
        if member_indices.len() < 2 {
            continue;
        }
        let mut group = member_indices
            .into_iter()
            .map(|index| automatic_messages[index].clone())
            .collect::<Vec<_>>();
        group.sort_by_key(candidate_message_identity);
        groups.push(group);
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs() as i64;
    let transaction = connection.transaction()?;
    transaction.execute("UPDATE candidate_cases SET is_current = 0", [])?;
    transaction.execute("DELETE FROM candidate_case_messages", [])?;

    for (case_id, members) in &manual_groups {
        let corpus_id = members
            .first()
            .map(|message| message.corpus_id.as_str())
            .ok_or_else(|| anyhow!("manual candidate group must not be empty"))?;
        transaction.execute(
            "UPDATE candidate_cases
             SET is_current = 1, updated_at = ?1
             WHERE case_id = ?2 AND case_origin = 'manual'",
            params![timestamp, case_id],
        )?;
        for message in members {
            transaction.execute(
                "INSERT INTO candidate_case_messages (case_id, revision_id, link_reason)
                 VALUES (?1, ?2, 'manual_override')",
                params![case_id, message.revision_id],
            )?;
        }
        if members.iter().any(|message| message.corpus_id != corpus_id) {
            bail!("manual candidate graph connected messages across corpora");
        }
    }

    for group in &groups {
        let case_id = candidate_case_id(group);
        let corpus_id = group
            .first()
            .map(|message| message.corpus_id.as_str())
            .ok_or_else(|| anyhow!("candidate group must not be empty"))?;
        if group.iter().any(|message| message.corpus_id != corpus_id) {
            bail!("candidate graph connected messages across corpora");
        }
        transaction.execute(
            "INSERT INTO candidate_cases (
                case_id, corpus_id, review_status, is_current, created_at, updated_at, case_origin
            ) VALUES (?1, ?2, 'candidate', 1, ?3, ?3, 'automatic')
            ON CONFLICT(case_id) DO UPDATE SET
                is_current = 1,
                updated_at = excluded.updated_at",
            params![case_id, corpus_id, timestamp],
        )?;
        for message in group {
            transaction.execute(
                "INSERT INTO candidate_case_messages (case_id, revision_id, link_reason)
                 VALUES (?1, ?2, 'reply_component')",
                params![case_id, message.revision_id],
            )?;
        }
    }
    transaction.commit()?;

    Ok(CandidateBuildStats {
        current_messages: messages.len(),
        resolved_reply_edges: graph.edge_count(),
        candidate_cases: groups.len(),
    })
}

pub(super) fn load_manual_case_members(
    connection: &Connection,
) -> anyhow::Result<HashMap<String, Vec<CandidateMessage>>> {
    let mut statement = connection.prepare(
        "SELECT memberships.case_id, message_revisions.revision_id,
                message_revisions.corpus_id, source_files.file_name,
                message_revisions.source_message_id
         FROM manual_case_memberships AS memberships
         JOIN candidate_cases ON candidate_cases.case_id = memberships.case_id
         JOIN source_files ON source_files.corpus_id = memberships.corpus_id
                           AND source_files.file_name = memberships.file_name
         JOIN message_revisions ON message_revisions.source_file_id = source_files.source_file_id
                               AND message_revisions.source_message_id = memberships.source_message_id
         WHERE source_files.source_file_id = (
             SELECT MAX(latest.source_file_id)
             FROM source_files AS latest
             WHERE latest.corpus_id = source_files.corpus_id
               AND latest.file_name = source_files.file_name
         )
         ORDER BY memberships.case_id, message_revisions.revision_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            CandidateMessage {
                revision_id: row.get(1)?,
                corpus_id: row.get(2)?,
                file_name: row.get(3)?,
                source_message_id: row.get(4)?,
            },
        ))
    })?;
    let mut groups = HashMap::<String, Vec<CandidateMessage>>::new();
    for row in rows {
        let (case_id, message) = row?;
        groups.entry(case_id).or_default().push(message);
    }
    Ok(groups)
}

pub(super) fn load_candidate_messages(
    connection: &Connection,
) -> anyhow::Result<Vec<CandidateMessage>> {
    let mut statement = connection.prepare(
        "SELECT message_revisions.revision_id, message_revisions.corpus_id,
            source_files.file_name, message_revisions.source_message_id
         FROM message_revisions
         JOIN source_files ON source_files.source_file_id = message_revisions.source_file_id
                 WHERE message_revisions.corpus_id = 'support'
                     AND source_files.source_file_id = (
             SELECT MAX(latest.source_file_id)
             FROM source_files AS latest
             WHERE latest.corpus_id = source_files.corpus_id
               AND latest.file_name = source_files.file_name
         )
         ORDER BY message_revisions.revision_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(CandidateMessage {
            revision_id: row.get(0)?,
            corpus_id: row.get(1)?,
            file_name: row.get(2)?,
            source_message_id: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn candidate_message_identity(message: &CandidateMessage) -> String {
    format!(
        "{}/{}/{}",
        message.corpus_id, message.file_name, message.source_message_id
    )
}

pub(super) fn candidate_case_id(messages: &[CandidateMessage]) -> String {
    let mut identities = messages
        .iter()
        .map(candidate_message_identity)
        .collect::<Vec<_>>();
    identities.sort();
    let mut digest = Sha256::new();
    for identity in identities {
        digest.update((identity.len() as u64).to_le_bytes());
        digest.update(identity.as_bytes());
    }
    format!("case-{:x}", digest.finalize())
}

pub(super) fn print_candidate_build_stats(stats: &CandidateBuildStats) {
    println!(
        "Grouped {} current messages and {} resolved reply edges into {} candidate cases.",
        stats.current_messages, stats.resolved_reply_edges, stats.candidate_cases
    );
}

pub(super) fn list_candidate_cases(
    connection: &Connection,
    limit: usize,
) -> anyhow::Result<Vec<CandidateCase>> {
    let mut statement = connection.prepare(
        "SELECT candidate_cases.case_id, candidate_cases.corpus_id,
                candidate_cases.review_status, candidate_cases.is_current,
                count(candidate_case_messages.revision_id)
         FROM candidate_cases
         LEFT JOIN candidate_case_messages ON candidate_case_messages.case_id = candidate_cases.case_id
         WHERE candidate_cases.is_current = 1
         GROUP BY candidate_cases.case_id
         ORDER BY candidate_cases.corpus_id, candidate_cases.case_id
         LIMIT ?1",
    )?;
    let rows = statement.query_map([limit as i64], |row| {
        Ok(CandidateCase {
            case_id: row.get(0)?,
            corpus_id: row.get(1)?,
            status: row.get(2)?,
            is_current: row.get::<_, i64>(3)? != 0,
            message_count: row.get::<_, i64>(4)? as usize,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn list_candidates_with_signatures(
    connection: &Connection,
    limit: usize,
) -> anyhow::Result<Vec<(String, usize)>> {
    let mut statement = connection.prepare(
        "SELECT candidate_cases.case_id, count(DISTINCT entities.canonical_value)
         FROM candidate_cases
         JOIN candidate_case_messages ON candidate_case_messages.case_id = candidate_cases.case_id
         JOIN entities ON entities.revision_id = candidate_case_messages.revision_id
         WHERE candidate_cases.is_current = 1
           AND entities.entity_type = 'transaction_signature_candidate'
         GROUP BY candidate_cases.case_id
         ORDER BY candidate_cases.corpus_id, candidate_cases.case_id
         LIMIT ?1",
    )?;
    let rows = statement.query_map([limit as i64], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn show_candidate_case(
    connection: &mut Connection,
    case_id: &str,
) -> anyhow::Result<()> {
    let case: Option<(String, String, bool)> = connection
        .query_row(
            "SELECT corpus_id, review_status, is_current
             FROM candidate_cases WHERE case_id = ?1",
            [case_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? != 0)),
        )
        .optional()?;
    let Some((corpus_id, status, is_current)) = case else {
        bail!("candidate case not found: {case_id}");
    };
    println!("{case_id}\t{corpus_id}\t{status}\tcurrent={is_current}");
    let mut statement = connection.prepare(
        "SELECT message_revisions.source_message_id, message_revisions.sender,
                message_revisions.date_title, message_revisions.body,
                message_revisions.reply_to_message_id
         FROM candidate_case_messages
         JOIN message_revisions ON message_revisions.revision_id = candidate_case_messages.revision_id
         WHERE candidate_case_messages.case_id = ?1
         ORDER BY message_revisions.revision_id",
    )?;
    let rows = statement.query_map([case_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    for row in rows {
        let (message_id, sender, date_title, body, reply_to) = row?;
        println!("  message {message_id} {date_title} {sender}");
        if let Some(reply_to) = reply_to {
            println!("    replies to message {reply_to}");
        }
        println!("    {}", message_preview(&body));
    }
    let mut review_statement = connection.prepare(
        "SELECT action, reviewer, rationale, reviewed_at
         FROM review_events WHERE case_id = ?1 ORDER BY review_event_id",
    )?;
    let reviews = review_statement.query_map([case_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    for review in reviews {
        let (action, reviewer, rationale, reviewed_at) = review?;
        println!("  review {action} by {reviewer} at {reviewed_at}: {rationale}");
    }
    let mut snapshot_statement = connection.prepare(
        "SELECT DISTINCT transaction_snapshots.signature, transaction_snapshots.cluster,
                transaction_snapshots.observation_status, transaction_snapshots.captured_at,
                transaction_snapshots.error
         FROM candidate_case_messages
         JOIN entities ON entities.revision_id = candidate_case_messages.revision_id
                     AND entities.entity_type = 'transaction_signature_candidate'
         JOIN transaction_snapshots
              ON transaction_snapshots.signature = entities.canonical_value
         WHERE candidate_case_messages.case_id = ?1
         ORDER BY transaction_snapshots.signature, transaction_snapshots.cluster",
    )?;
    let snapshots = snapshot_statement.query_map([case_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    for snapshot in snapshots {
        let (signature, cluster, status, captured_at, error) = snapshot?;
        println!("  transaction {signature} [{cluster}] {status} at {captured_at}");
        if let Some(error) = error {
            println!("    {error}");
        }
    }
    Ok(())
}
