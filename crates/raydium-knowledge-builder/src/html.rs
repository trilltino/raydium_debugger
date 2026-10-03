//! Private html operations; blocking operator tooling.
use super::*;
pub(super) fn import_source(
    connection: &mut Connection,
    corpus_id: &str,
    file_name: &str,
    html: &[u8],
) -> anyhow::Result<ImportResult> {
    let digest = format!("{:x}", Sha256::digest(html));
    let already_imported: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM source_files WHERE corpus_id = ?1 AND file_name = ?2 AND sha256 = ?3)",
        params![corpus_id, file_name, digest],
        |row| row.get(0),
    )?;
    if already_imported {
        return Ok(ImportResult::default());
    }

    let html = std::str::from_utf8(html).context("Telegram HTML is not valid UTF-8")?;
    let parsed = parse_html(html)?;
    let imported_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs() as i64;
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO source_files (corpus_id, file_name, sha256, imported_at) VALUES (?1, ?2, ?3, ?4)",
        params![corpus_id, file_name, digest, imported_at],
    )?;
    let source_file_id = transaction.last_insert_rowid();
    for message in &parsed.messages {
        transaction
            .prepare_cached(
                "INSERT INTO message_revisions (
                corpus_id, source_file_id, source_message_id, sender, date_title, body,
                reply_to_message_id, media_refs_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?
            .execute(params![
                corpus_id,
                source_file_id,
                message.source_message_id,
                message.sender,
                message.date_title,
                message.body,
                message.reply_to_message_id,
                serde_json::to_string(&message.media_refs)?
            ])?;
    }
    transaction.commit()?;

    Ok(ImportResult {
        imported: true,
        stored_messages: parsed.messages.len(),
        skipped_records: parsed.skipped_records,
    })
}

pub(super) fn parse_html(html: &str) -> anyhow::Result<ParsedHtml> {
    let message_selector = Selector::parse("div.message[id]")
        .map_err(|error| anyhow!("invalid message selector: {error}"))?;
    let sender_selector = Selector::parse(".from_name")
        .map_err(|error| anyhow!("invalid sender selector: {error}"))?;
    let date_selector = Selector::parse(".date.details")
        .map_err(|error| anyhow!("invalid date selector: {error}"))?;
    let body_selector =
        Selector::parse(".text").map_err(|error| anyhow!("invalid body selector: {error}"))?;
    let reply_selector = Selector::parse(".reply_to a[href]")
        .map_err(|error| anyhow!("invalid reply selector: {error}"))?;
    let media_selector = Selector::parse(".media_wrap a[href]")
        .map_err(|error| anyhow!("invalid media selector: {error}"))?;

    let document = Html::parse_document(html);
    let mut parsed = ParsedHtml::default();
    for element in document.select(&message_selector) {
        if has_class(element, "service") {
            parsed.skipped_records += 1;
            continue;
        }
        let Some(source_message_id) = element
            .value()
            .attr("id")
            .and_then(|id| id.strip_prefix("message"))
            .filter(|id| !id.is_empty() && id.chars().all(|ch| ch.is_ascii_digit()))
            .map(str::to_string)
        else {
            parsed.skipped_records += 1;
            continue;
        };

        let sender = selected_text(element, &sender_selector);
        let date_title = element
            .select(&date_selector)
            .next()
            .and_then(|date| date.value().attr("title"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let body = selected_text(element, &body_selector).unwrap_or_default();
        let reply_to_message_id = element
            .select(&reply_selector)
            .next()
            .and_then(|link| link.value().attr("href"))
            .and_then(|href| href.strip_prefix("#go_to_message"))
            .filter(|id| !id.is_empty() && id.chars().all(|ch| ch.is_ascii_digit()))
            .map(str::to_string);
        let media_refs = element
            .select(&media_selector)
            .filter_map(|link| link.value().attr("href"))
            .map(str::to_string)
            .collect::<Vec<_>>();

        if date_title.is_none()
            && body.is_empty()
            && reply_to_message_id.is_none()
            && media_refs.is_empty()
        {
            parsed.skipped_records += 1;
            continue;
        }

        parsed.messages.push(ParsedMessage {
            source_message_id,
            sender,
            date_title,
            body,
            reply_to_message_id,
            media_refs,
        });
    }
    Ok(parsed)
}

pub(super) fn selected_text(element: ElementRef<'_>, selector: &Selector) -> Option<String> {
    element
        .select(selector)
        .next()
        .map(|selected| normalize_text(&selected.text().collect::<String>()))
        .filter(|text| !text.is_empty())
}

pub(super) fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn has_class(element: ElementRef<'_>, class_name: &str) -> bool {
    element.value().attr("class").is_some_and(|classes| {
        classes
            .split_ascii_whitespace()
            .any(|class| class == class_name)
    })
}
