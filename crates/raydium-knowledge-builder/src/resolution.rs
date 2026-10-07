//! Private, repeatable resolution triage and evidence review.
use super::*;

const RESOLUTION_RULE_VERSION: i64 = 3;

#[derive(Clone)]
pub(super) struct ResolutionMessage {
    pub(super) revision_id: i64,
    pub(super) sender: Option<String>,
    pub(super) body: String,
}

pub(super) fn messages(
    connection: &Connection,
    case_id: &str,
) -> anyhow::Result<Vec<ResolutionMessage>> {
    let mut statement = connection.prepare(
        "SELECT m.revision_id, m.sender, m.body FROM candidate_case_messages cm
         JOIN message_revisions m ON m.revision_id=cm.revision_id
         WHERE cm.case_id=?1 ORDER BY m.revision_id",
    )?;
    let result = statement
        .query_map([case_id], |r| {
            Ok(ResolutionMessage {
                revision_id: r.get(0)?,
                sender: r.get(1)?,
                body: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(result)
}

pub(super) fn fingerprint(messages: &[ResolutionMessage]) -> String {
    let mut digest = Sha256::new();
    for message in messages {
        digest.update(message.revision_id.to_le_bytes());
        digest.update((message.body.len() as u64).to_le_bytes());
        digest.update(message.body.as_bytes());
        digest.update(message.sender.as_deref().unwrap_or("").as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn normalized(body: &str) -> String {
    let mut text = String::with_capacity(body.len() + 2);
    text.push(' ');
    for c in body.to_lowercase().chars() {
        text.push(if c.is_alphanumeric() { c } else { ' ' });
    }
    text.push(' ');
    text
}

fn contains_any(text: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| text.contains(phrase))
}

pub(super) fn signals(messages: &[ResolutionMessage]) -> Vec<(i64, &'static str)> {
    let reporter = messages.first().and_then(|m| m.sender.as_deref());
    let mut results = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let text = normalized(&message.body);
        let negated = contains_any(
            &text,
            &[
                " not fixed ",
                " not resolved ",
                " not working ",
                " still not ",
                " unable to ",
                " didn t work ",
                " doesn t work ",
                " still fails ",
                " not yet ",
                " will be fixing ",
            ],
        );
        let is_reporter = reporter.is_some_and(|sender| message.sender.as_deref() == Some(sender));
        let confirmation = index > 0
            && is_reporter
            && !negated
            && !message.body.contains('?')
            && contains_any(
                &text,
                &[
                    " resolved ",
                    " solved ",
                    " it worked ",
                    " working now ",
                    " working fine now ",
                    " fixed it ",
                    " succeeded ",
                    " successful ",
                ],
            );
        if confirmation {
            results.push((message.revision_id, "reporter_confirmation"));
        }
        let team_fix = index > 0
            && !is_reporter
            && !negated
            && !message.body.contains('?')
            && contains_any(
                &text,
                &[
                    " fixed ",
                    " is fixed ",
                    " issue should be fixed ",
                    " we fixed ",
                    " update is done ",
                    " we are done with the update ",
                ],
            );
        if team_fix {
            results.push((message.revision_id, "team_fix"));
        }
        let proposal = index > 0
            && !negated
            && contains_any(
                &text,
                &[
                    " try ",
                    " change ",
                    " switch ",
                    " refresh ",
                    " use ",
                    " need to ",
                    " have to ",
                    " turn off ",
                    " reinstall ",
                    " modify ",
                ],
            );
        if proposal {
            results.push((message.revision_id, "proposal"));
        }
    }
    results
}

pub(super) fn reconcile_resolutions(connection: &mut Connection) -> anyhow::Result<()> {
    let ids: Vec<String> = connection
        .prepare("SELECT case_id FROM candidate_cases WHERE is_current=1 AND corpus_id='support' ORDER BY case_id")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut scans = Vec::with_capacity(ids.len());
    for id in ids {
        let case_messages = messages(connection, &id)?;
        scans.push((id, fingerprint(&case_messages), signals(&case_messages)));
    }
    let tx = connection.transaction()?;
    tx.execute("DELETE FROM case_resolution_suggestions", [])?;
    tx.execute("DELETE FROM case_resolution_scans", [])?;
    for (id, hash, suggestions) in &scans {
        tx.execute(
            "INSERT INTO case_resolution_scans(case_id,fingerprint,rule_version) VALUES (?1,?2,?3)",
            params![id, hash, RESOLUTION_RULE_VERSION],
        )?;
        for (revision_id, signal) in suggestions {
            tx.execute(
                "INSERT INTO case_resolution_suggestions(case_id,revision_id,signal) VALUES (?1,?2,?3)",
                params![id, revision_id, signal],
            )?;
        }
    }
    tx.commit()?;
    println!("Reconciled {} current support candidates.", scans.len());
    resolution_report(connection, false)
}

pub(super) fn suggested_tier(connection: &Connection, id: &str) -> anyhow::Result<&'static str> {
    let scanned: Option<String> = connection
        .query_row(
            "SELECT fingerprint FROM case_resolution_scans WHERE case_id=?1 AND rule_version=?2",
            params![id, RESOLUTION_RULE_VERSION],
            |r| r.get(0),
        )
        .optional()?;
    if scanned.as_deref() != Some(fingerprint(&messages(connection, id)?).as_str()) {
        return Ok("not_scanned");
    }
    let mut statement = connection
        .prepare("SELECT DISTINCT signal FROM case_resolution_suggestions WHERE case_id=?1")?;
    let signals: Vec<String> = statement
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(if signals.iter().any(|s| s == "reporter_confirmation") {
        "confirmed"
    } else if signals.iter().any(|s| s == "team_fix") {
        "team_fixed"
    } else if signals.iter().any(|s| s == "proposal") {
        "proposed"
    } else {
        "unknown"
    })
}

pub(super) fn review_state(connection: &Connection, id: &str) -> anyhow::Result<&'static str> {
    let review: Option<String> = connection
        .query_row(
            "SELECT fingerprint FROM case_resolution_reviews WHERE case_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(review) = review else {
        return Ok("unreviewed");
    };
    let current = fingerprint(&messages(connection, id)?);
    Ok(if review == current {
        "reviewed"
    } else {
        "stale_review"
    })
}

pub(super) fn resolution_report(connection: &Connection, detailed: bool) -> anyhow::Result<()> {
    let ids: Vec<String> = connection.prepare(
        "SELECT case_id FROM candidate_cases WHERE is_current=1 AND corpus_id='support' ORDER BY case_id"
    )?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let mut counts = [0usize; 5];
    let mut unreviewed = 0;
    let mut stale = 0;
    if detailed {
        println!("case_id\tsuggested_tier\treview_state");
    }
    for id in &ids {
        let tier = suggested_tier(connection, id)?;
        let index = match tier {
            "confirmed" => 0,
            "team_fixed" => 1,
            "proposed" => 2,
            "unknown" => 3,
            _ => 4,
        };
        counts[index] += 1;
        let state = review_state(connection, id)?;
        if state == "unreviewed" {
            unreviewed += 1;
        }
        if state == "stale_review" {
            stale += 1;
        }
        if detailed {
            println!("{id}\t{tier}\t{state}");
        }
    }
    let summary = format!("resolution suggestions: confirmed={} team_fixed={} proposed={} unknown={} not_scanned={} unreviewed={} stale_reviews={}", counts[0], counts[1], counts[2], counts[3], counts[4], unreviewed, stale);
    if detailed {
        eprintln!("{summary}");
    } else {
        println!("{summary}");
    }
    Ok(())
}

pub(super) fn show_resolution(connection: &Connection, id: &str) -> anyhow::Result<()> {
    let current: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM candidate_cases WHERE case_id=?1 AND is_current=1 AND corpus_id='support')",
        [id], |r| r.get(0)
    )?;
    anyhow::ensure!(current, "current support candidate not found: {id}");
    println!(
        "{id}\tsuggested={}\t{}",
        suggested_tier(connection, id)?,
        review_state(connection, id)?
    );
    if suggested_tier(connection, id)? == "not_scanned" {
        println!("  suggestions are stale; run candidates reconcile");
    } else {
        let mut statement = connection.prepare(
            "SELECT s.revision_id,s.signal,m.body FROM case_resolution_suggestions s JOIN message_revisions m ON m.revision_id=s.revision_id WHERE s.case_id=?1 ORDER BY s.revision_id,s.signal"
        )?;
        for row in statement.query_map([id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (revision, signal, body) = row?;
            println!("  {revision}\t{signal}\t{}", message_preview(&body));
        }
    }
    if let Some((outcome,evidence,reviewer,rationale)) = connection.query_row(
        "SELECT outcome,evidence_json,reviewer,rationale FROM case_resolution_reviews WHERE case_id=?1",
        [id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))
    ).optional()? {
        println!("  review: {outcome} evidence={evidence} by {reviewer}: {rationale}");
    }
    Ok(())
}

pub(super) fn verify_resolution(
    connection: &Connection,
    id: &str,
    outcome: &str,
    evidence: &str,
    rationale: &str,
    reviewer: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(outcome, "confirmed" | "team_fixed" | "proposed" | "unknown"),
        "invalid resolution outcome"
    );
    anyhow::ensure!(
        !reviewer.trim().is_empty() && !rationale.trim().is_empty(),
        "reviewer and rationale are required"
    );
    let case_messages = messages(connection, id)?;
    anyhow::ensure!(!case_messages.is_empty() && connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM candidate_cases WHERE case_id=?1 AND is_current=1 AND corpus_id='support')",[id],|r|r.get::<_,bool>(0))?, "current support candidate not found: {id}");
    let mut ids: Vec<i64> = if evidence.trim().is_empty() {
        Vec::new()
    } else {
        evidence
            .split(',')
            .map(|part| part.trim().parse::<i64>())
            .collect::<Result<_, _>>()?
    };
    ids.sort_unstable();
    ids.dedup();
    anyhow::ensure!(
        outcome == "unknown" || !ids.is_empty(),
        "cite at least one evidence revision"
    );
    anyhow::ensure!(
        outcome != "confirmed" || ids.len() >= 2,
        "confirmed outcome needs an action and later confirmation"
    );
    anyhow::ensure!(
        ids.iter()
            .all(|id| case_messages.iter().any(|m| m.revision_id == *id)),
        "evidence revision does not belong to current case"
    );
    let reporter = case_messages.first().and_then(|m| m.sender.as_deref());
    if outcome == "confirmed" {
        let final_message = case_messages
            .iter()
            .find(|m| m.revision_id == *ids.last().unwrap());
        anyhow::ensure!(
            reporter.is_some()
                && final_message
                    .is_some_and(|m| m.sender.as_deref() == reporter && !m.body.contains('?')),
            "confirmed outcome needs a later reporter confirmation message"
        );
    }
    if outcome == "team_fixed" {
        anyhow::ensure!(
            ids.iter().any(
                |revision| case_messages.iter().any(|m| m.revision_id == *revision
                    && m.sender.as_deref() != reporter
                    && !m.body.contains('?'))
            ),
            "team-fixed outcome needs a team statement in the cited evidence"
        );
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    connection.execute(
        "INSERT INTO case_resolution_reviews(case_id,outcome,evidence_json,fingerprint,reviewer,rationale,reviewed_at) VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(case_id) DO UPDATE SET outcome=excluded.outcome,evidence_json=excluded.evidence_json,fingerprint=excluded.fingerprint,reviewer=excluded.reviewer,rationale=excluded.rationale,reviewed_at=excluded.reviewed_at",
        params![id,outcome,serde_json::to_string(&ids)?,fingerprint(&case_messages),reviewer.trim(),rationale.trim(),now]
    )?;
    connection.execute(
        "UPDATE candidate_cases SET review_status='candidate' WHERE case_id=?1",
        [id],
    )?;
    println!("Recorded {outcome} resolution review for {id}.");
    Ok(())
}

pub(super) fn ensure_publishable_resolution(
    connection: &Connection,
    id: &str,
) -> anyhow::Result<()> {
    let review: Option<(String,String,String)> = connection.query_row(
        "SELECT outcome,evidence_json,fingerprint FROM case_resolution_reviews WHERE case_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))
    ).optional()?;
    let Some((outcome, evidence, hash)) = review else {
        bail!("verify resolution evidence before approval: {id}");
    };
    anyhow::ensure!(
        matches!(outcome.as_str(), "confirmed" | "team_fixed"),
        "only confirmed or team-fixed outcomes may be approved: {id}"
    );
    let current = messages(connection, id)?;
    anyhow::ensure!(
        hash == fingerprint(&current),
        "resolution review is stale; recheck messages: {id}"
    );
    let ids: Vec<i64> = serde_json::from_str(&evidence)?;
    anyhow::ensure!(
        !ids.is_empty()
            && ids
                .iter()
                .all(|revision| current.iter().any(|m| m.revision_id == *revision)),
        "resolution evidence is no longer in case: {id}"
    );
    Ok(())
}
