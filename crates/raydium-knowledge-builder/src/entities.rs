//! Private entities operations; blocking operator tooling.
use super::*;
pub(super) fn ensure_entity_candidates(connection: &mut Connection) -> anyhow::Result<usize> {
    let max_source_file_id: i64 = connection.query_row(
        "SELECT COALESCE(MAX(source_file_id), 0) FROM source_files",
        [],
        |row| row.get(0),
    )?;
    let expected_watermark = format!("{ENTITY_EXTRACTION_VERSION}:{max_source_file_id}");
    let current_watermark: Option<String> = connection
        .query_row(
            "SELECT value FROM knowledge_metadata WHERE key = 'entity_extraction'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if current_watermark.as_deref() == Some(expected_watermark.as_str()) {
        return Ok(0);
    }
    rebuild_entity_candidates(connection)
}

pub(super) fn rebuild_entity_candidates(connection: &mut Connection) -> anyhow::Result<usize> {
    let max_source_file_id: i64 = connection.query_row(
        "SELECT COALESCE(MAX(source_file_id), 0) FROM source_files",
        [],
        |row| row.get(0),
    )?;
    let messages = {
        let mut statement = connection.prepare(
            "SELECT revision_id, sender, body FROM message_revisions ORDER BY revision_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut extracted = Vec::new();
    for (revision_id, sender, body) in messages {
        let text = format!("{sender} {body}");
        extracted.extend(
            extract_entity_candidates(&text)
                .into_iter()
                .map(|candidate| (revision_id, candidate)),
        );
    }

    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM entities", [])?;
    for (revision_id, candidate) in &extracted {
        transaction.execute(
            "INSERT INTO entities (
                revision_id, entity_type, canonical_value, surface_form,
                decoded_length, extraction_version
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                revision_id,
                candidate.entity_type,
                candidate.canonical_value,
                candidate.surface_form,
                candidate.decoded_length,
                ENTITY_EXTRACTION_VERSION
            ],
        )?;
    }
    let watermark = format!("{ENTITY_EXTRACTION_VERSION}:{max_source_file_id}");
    transaction.execute(
        "INSERT INTO knowledge_metadata (key, value) VALUES ('entity_extraction', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [watermark],
    )?;
    transaction.commit()?;
    Ok(extracted.len())
}

pub(super) fn extract_entity_candidates(text: &str) -> Vec<EntityCandidate> {
    const BASE58_ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut candidates = HashSet::new();

    for token in text.split(|character: char| !BASE58_ALPHABET.contains(character)) {
        if !(32..=100).contains(&token.len()) {
            continue;
        }
        let Ok(decoded) = bs58::decode(token).into_vec() else {
            continue;
        };
        let entity_type = match decoded.len() {
            32 => "address_candidate",
            64 => "transaction_signature_candidate",
            _ => continue,
        };
        candidates.insert(EntityCandidate {
            entity_type,
            canonical_value: bs58::encode(&decoded).into_string(),
            surface_form: token.to_string(),
            decoded_length: Some(decoded.len() as i64),
        });
    }

    candidates.extend(extract_numeric_error_candidates(text));
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.entity_type
            .cmp(right.entity_type)
            .then_with(|| left.canonical_value.cmp(&right.canonical_value))
            .then_with(|| left.surface_form.cmp(&right.surface_form))
    });
    candidates
}

pub(super) fn extract_numeric_error_candidates(text: &str) -> Vec<EntityCandidate> {
    let lowercase = text.to_ascii_lowercase();
    let mut candidates = HashSet::new();
    for (index, _) in lowercase.match_indices("custom(") {
        let value_start = index + "custom(".len();
        let Some(relative_end) = lowercase[value_start..].find(')') else {
            continue;
        };
        let value_end = value_start + relative_end;
        let digits = &lowercase[value_start..value_end];
        let Ok(code) = digits.parse::<u32>() else {
            continue;
        };
        candidates.insert(EntityCandidate {
            entity_type: "numeric_error_candidate",
            canonical_value: code.to_string(),
            surface_form: text[index..=value_end].to_string(),
            decoded_length: None,
        });
    }

    for (index, _) in lowercase.match_indices("0x") {
        let digits_start = index + 2;
        let bytes = lowercase.as_bytes();
        let mut digits_end = digits_start;
        while digits_end < bytes.len() && bytes[digits_end].is_ascii_hexdigit() {
            digits_end += 1;
        }
        if digits_end == digits_start || digits_end - digits_start > 8 {
            continue;
        }
        let Ok(code) = u32::from_str_radix(&lowercase[digits_start..digits_end], 16) else {
            continue;
        };
        candidates.insert(EntityCandidate {
            entity_type: "numeric_error_candidate",
            canonical_value: code.to_string(),
            surface_form: text[index..digits_end].to_string(),
            decoded_length: None,
        });
    }
    candidates.into_iter().collect()
}
