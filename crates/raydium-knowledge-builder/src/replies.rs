//! Private replies operations; blocking operator tooling.
use super::*;
pub(super) fn resolve_reply_edges(
    connection: &mut Connection,
) -> anyhow::Result<ReplyResolutionStats> {
    let messages = {
        let mut statement = connection.prepare(
            "SELECT message_revisions.revision_id, message_revisions.corpus_id,
                    message_revisions.source_message_id, message_revisions.reply_to_message_id
             FROM message_revisions
             JOIN source_files ON source_files.source_file_id = message_revisions.source_file_id
             WHERE source_files.source_file_id = (
                 SELECT MAX(latest.source_file_id)
                 FROM source_files AS latest
                 WHERE latest.corpus_id = source_files.corpus_id
                   AND latest.file_name = source_files.file_name
             )",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ActiveMessage {
                revision_id: row.get(0)?,
                corpus_id: row.get(1)?,
                source_message_id: row.get(2)?,
                reply_to_message_id: row.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut targets: HashMap<(String, String), Vec<i64>> = HashMap::new();
    for message in &messages {
        targets
            .entry((message.corpus_id.clone(), message.source_message_id.clone()))
            .or_default()
            .push(message.revision_id);
    }

    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM reply_edges", [])?;
    let mut stats = ReplyResolutionStats::default();
    for message in messages {
        let Some(target_source_message_id) = message.reply_to_message_id else {
            continue;
        };
        stats.total += 1;
        let matching_targets =
            targets.get(&(message.corpus_id.clone(), target_source_message_id.clone()));
        let (status, target_revision_id) = match matching_targets.map(Vec::as_slice) {
            Some([target_revision_id]) => {
                stats.resolved += 1;
                ("resolved", Some(*target_revision_id))
            }
            Some([]) | None => {
                stats.missing += 1;
                ("unresolved", None)
            }
            Some(_) => {
                stats.ambiguous += 1;
                ("ambiguous", None)
            }
        };
        transaction.execute(
            "INSERT INTO reply_edges (
                source_revision_id, corpus_id, target_source_message_id,
                target_revision_id, status
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                message.revision_id,
                message.corpus_id,
                target_source_message_id,
                target_revision_id,
                status
            ],
        )?;
    }
    transaction.commit()?;
    Ok(stats)
}

pub(super) fn print_reply_stats(stats: &ReplyResolutionStats) {
    println!(
        "Reply links: {} resolved, {} unresolved, {} ambiguous ({} total)",
        stats.resolved, stats.missing, stats.ambiguous, stats.total
    );
}
