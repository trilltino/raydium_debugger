//! Private search operations; blocking operator tooling.
use super::*;
#[derive(Debug)]
pub(super) struct SearchOptions {
    pub(super) database_path: PathBuf,
    pub(super) index_path: PathBuf,
    pub(super) corpus_id: Option<String>,
    pub(super) limit: usize,
}

#[derive(Debug)]
pub(super) struct EntitySearchOptions {
    pub(super) database_path: PathBuf,
    pub(super) corpus_id: Option<String>,
    pub(super) limit: usize,
}

pub(super) fn parse_entity_search_options(args: &[String]) -> anyhow::Result<EntitySearchOptions> {
    let mut options = EntitySearchOptions {
        database_path: support_database_path(),
        corpus_id: None,
        limit: 30,
    };
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        index += 1;
        let value = args
            .get(index)
            .ok_or_else(|| anyhow!("missing value for {option}"))?;
        match option {
            "--database" => options.database_path = PathBuf::from(value),
            "--corpus" => {
                if !matches!(value.as_str(), "support" | "announcements") {
                    bail!("corpus must be 'support' or 'announcements'");
                }
                options.corpus_id = Some(value.clone());
            }
            "--limit" => {
                options.limit = value
                    .parse::<usize>()
                    .context("entity limit must be a positive integer")?;
                if options.limit == 0 || options.limit > MAX_SEARCH_RESULTS {
                    bail!("entity limit must be between 1 and {MAX_SEARCH_RESULTS}");
                }
            }
            other => bail!("unknown entity-search option {other:?}"),
        }
        index += 1;
    }
    Ok(options)
}

pub(super) fn search_entities_and_print(
    value: &str,
    options: &EntitySearchOptions,
) -> anyhow::Result<()> {
    let hits = search_entities(
        &options.database_path,
        value,
        options.corpus_id.as_deref(),
        options.limit,
    )?;
    if hits.is_empty() {
        println!("No matching entity candidates.");
        return Ok(());
    }
    for hit in hits {
        println!(
            "[{}] {}:{} {} = {} ({})",
            hit.corpus_id,
            hit.file_name,
            hit.source_message_id,
            hit.entity_type,
            hit.canonical_value,
            hit.surface_form
        );
    }
    Ok(())
}

pub(super) fn search_entities(
    database_path: &Path,
    value: &str,
    corpus_id: Option<&str>,
    limit: usize,
) -> anyhow::Result<Vec<EntityHit>> {
    if value.trim().is_empty() {
        bail!("entity value must not be empty");
    }
    if limit == 0 || limit > MAX_SEARCH_RESULTS {
        bail!("entity limit must be between 1 and {MAX_SEARCH_RESULTS}");
    }
    if corpus_id.is_some_and(|corpus| !matches!(corpus, "support" | "announcements")) {
        bail!("corpus must be 'support' or 'announcements'");
    }

    let mut connection = open_existing_database(database_path)?;
    initialize_schema(&connection)?;
    ensure_entity_candidates(&mut connection)?;
    let canonical_value = canonical_entity_query(value);
    let mut statement = connection.prepare(
        "SELECT entities.entity_type, entities.canonical_value, entities.surface_form,
                entities.decoded_length, message_revisions.corpus_id, source_files.file_name,
                message_revisions.source_message_id
         FROM entities
         JOIN message_revisions ON message_revisions.revision_id = entities.revision_id
         JOIN source_files ON source_files.source_file_id = message_revisions.source_file_id
         WHERE (entities.canonical_value = ?1 OR entities.surface_form = ?2)
           AND (?3 IS NULL OR message_revisions.corpus_id = ?3)
           AND source_files.source_file_id = (
               SELECT MAX(latest.source_file_id)
               FROM source_files AS latest
               WHERE latest.corpus_id = source_files.corpus_id
                 AND latest.file_name = source_files.file_name
           )
         ORDER BY entities.entity_type, message_revisions.revision_id
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![canonical_value, value, corpus_id, limit as i64],
        |row| {
            Ok(EntityHit {
                entity_type: row.get(0)?,
                canonical_value: row.get(1)?,
                surface_form: row.get(2)?,
                decoded_length: row.get(3)?,
                corpus_id: row.get(4)?,
                file_name: row.get(5)?,
                source_message_id: row.get(6)?,
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn canonical_entity_query(value: &str) -> String {
    let trimmed = value.trim();
    let lowercase = trimmed.to_ascii_lowercase();
    if let Some(code) = lowercase
        .strip_prefix("custom(")
        .and_then(|code| code.strip_suffix(')'))
        .and_then(|code| code.parse::<u32>().ok())
    {
        return code.to_string();
    }
    if let Some(code) = lowercase
        .strip_prefix("0x")
        .and_then(|code| u32::from_str_radix(code, 16).ok())
    {
        return code.to_string();
    }
    if let Ok(decoded) = bs58::decode(trimmed).into_vec() {
        if matches!(decoded.len(), 32 | 64) {
            return bs58::encode(decoded).into_string();
        }
    }
    trimmed.to_string()
}

pub(super) fn parse_index_paths(args: &[String]) -> anyhow::Result<(PathBuf, PathBuf)> {
    let mut database_path = support_database_path();
    let mut index_path = PathBuf::from(DEFAULT_INDEX_PATH);
    let mut index = 0;
    while index < args.len() {
        let target = match args[index].as_str() {
            "--database" => &mut database_path,
            "--index" => &mut index_path,
            other => bail!("unknown index option {other:?}"),
        };
        index += 1;
        let value = args
            .get(index)
            .ok_or_else(|| anyhow!("missing option value"))?;
        *target = PathBuf::from(value);
        index += 1;
    }
    Ok((database_path, index_path))
}

pub(super) fn parse_search_options(args: &[String]) -> anyhow::Result<SearchOptions> {
    let mut options = SearchOptions {
        database_path: support_database_path(),
        index_path: PathBuf::from(DEFAULT_INDEX_PATH),
        corpus_id: None,
        limit: 10,
    };
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        index += 1;
        let value = args
            .get(index)
            .ok_or_else(|| anyhow!("missing value for {option}"))?;
        match option {
            "--database" => options.database_path = PathBuf::from(value),
            "--index" => options.index_path = PathBuf::from(value),
            "--corpus" => {
                if !matches!(value.as_str(), "support" | "announcements") {
                    bail!("corpus must be 'support' or 'announcements'");
                }
                options.corpus_id = Some(value.clone());
            }
            "--limit" => {
                options.limit = value
                    .parse::<usize>()
                    .context("search limit must be a positive integer")?;
                if options.limit == 0 || options.limit > MAX_SEARCH_RESULTS {
                    bail!("search limit must be between 1 and {MAX_SEARCH_RESULTS}");
                }
            }
            other => bail!("unknown search option {other:?}"),
        }
        index += 1;
    }
    Ok(options)
}

pub(super) fn search_and_print(query: &str, options: &SearchOptions) -> anyhow::Result<()> {
    let hits = search_messages(
        &options.database_path,
        &options.index_path,
        query,
        options.corpus_id.as_deref(),
        options.limit,
    )?;
    if hits.is_empty() {
        println!("No matches.");
        return Ok(());
    }
    for (rank, hit) in hits.iter().enumerate() {
        println!(
            "{}. [{}] {}:{} (score {:.3})",
            rank + 1,
            hit.corpus_id,
            hit.file_name,
            hit.source_message_id,
            hit.score
        );
        if !hit.date_title.is_empty() {
            println!("   {}", hit.date_title);
        }
        if !hit.sender.is_empty() {
            println!("   {}", hit.sender);
        }
        println!("   {}", message_preview(&hit.body));
    }
    Ok(())
}

pub(super) fn search_messages(
    database_path: &Path,
    index_path: &Path,
    query_text: &str,
    corpus_id: Option<&str>,
    limit: usize,
) -> anyhow::Result<Vec<SearchHit>> {
    let query_text = query_text.trim();
    if query_text.is_empty() {
        bail!("search query must not be empty");
    }
    if limit == 0 || limit > MAX_SEARCH_RESULTS {
        bail!("search limit must be between 1 and {MAX_SEARCH_RESULTS}");
    }
    if corpus_id.is_some_and(|corpus| !matches!(corpus, "support" | "announcements")) {
        bail!("corpus must be 'support' or 'announcements'");
    }

    ensure_search_index(database_path, index_path)?;
    let index = Index::open_in_dir(index_path)
        .with_context(|| format!("failed to open search index {}", index_path.display()))?;
    let fields = SearchFields::from_schema(&index.schema())?;
    let mut parser = QueryParser::for_index(&index, vec![fields.body, fields.sender]);
    parser.set_conjunction_by_default();
    let normalized_query = normalize_search_query(query_text);
    if normalized_query.is_empty() {
        bail!("search query must contain at least one letter or number");
    }
    let parsed_query = parser
        .parse_query(&normalized_query)
        .context("failed to parse search query")?;
    let query: Box<dyn Query> = if let Some(corpus_id) = corpus_id {
        let corpus_query = TermQuery::new(
            Term::from_field_text(fields.corpus_id, corpus_id),
            IndexRecordOption::Basic,
        );
        Box::new(BooleanQuery::new(vec![
            (Occur::Must, parsed_query),
            (Occur::Must, Box::new(corpus_query)),
        ]))
    } else {
        parsed_query
    };
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let matching_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;
    matching_docs
        .into_iter()
        .map(|(score, address)| {
            let document: TantivyDocument = searcher.doc(address)?;
            Ok(SearchHit {
                score,
                corpus_id: stored_text(&document, fields.corpus_id),
                file_name: stored_text(&document, fields.file_name),
                source_message_id: stored_text(&document, fields.message_id),
                sender: stored_text(&document, fields.sender),
                date_title: stored_text(&document, fields.date_title),
                body: stored_text(&document, fields.body),
            })
        })
        .collect()
}

pub(super) fn ensure_search_index(database_path: &Path, index_path: &Path) -> anyhow::Result<()> {
    let connection = open_existing_database(database_path)?;
    let current_revision = current_database_revision(&connection)?;
    let manifest_path = index_path.join("support-index-manifest.json");
    let index_is_current = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<SearchIndexManifest>(&raw).ok())
        .is_some_and(|manifest| {
            manifest.schema_version == SEARCH_INDEX_VERSION
                && manifest.database_revision == current_revision
                && index_path.join("meta.json").is_file()
        });
    if !index_is_current {
        rebuild_search_index(database_path, index_path)?;
    }
    Ok(())
}

pub(super) fn rebuild_search_index(
    database_path: &Path,
    index_path: &Path,
) -> anyhow::Result<usize> {
    let connection = open_existing_database(database_path)?;
    let database_revision = current_database_revision(&connection)?;
    fs::create_dir_all(index_path)
        .with_context(|| format!("failed to create index directory {}", index_path.display()))?;
    let schema = search_schema();
    let index = if index_path.join("meta.json").is_file() {
        Index::open_in_dir(index_path)
            .with_context(|| format!("failed to open search index {}", index_path.display()))?
    } else {
        Index::create_in_dir(index_path, schema.clone())
            .with_context(|| format!("failed to create search index {}", index_path.display()))?
    };
    let fields = SearchFields::from_schema(&index.schema())?;
    let mut writer = index
        .writer(50_000_000)
        .context("failed to create Tantivy index writer")?;
    writer.delete_all_documents()?;

    let mut statement = connection.prepare(
        "SELECT message_revisions.corpus_id, source_files.file_name,
                message_revisions.source_message_id, message_revisions.sender,
                message_revisions.date_title, message_revisions.body
         FROM message_revisions
         JOIN source_files ON source_files.source_file_id = message_revisions.source_file_id
         WHERE source_files.source_file_id = (
             SELECT MAX(latest.source_file_id)
             FROM source_files AS latest
             WHERE latest.corpus_id = source_files.corpus_id
               AND latest.file_name = source_files.file_name
         )
         ORDER BY message_revisions.revision_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            row.get::<_, String>(5)?,
        ))
    })?;
    let mut indexed_messages = 0;
    for row in rows {
        let (corpus_id, file_name, message_id, sender, date_title, body) = row?;
        writer.add_document(doc!(
            fields.corpus_id => corpus_id,
            fields.file_name => file_name,
            fields.message_id => message_id,
            fields.sender => sender,
            fields.date_title => date_title,
            fields.body => body,
        ))?;
        indexed_messages += 1;
    }
    writer.commit()?;
    let manifest = SearchIndexManifest {
        schema_version: SEARCH_INDEX_VERSION,
        database_revision,
    };
    fs::write(
        index_path.join("support-index-manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )
    .with_context(|| {
        format!(
            "failed to write search manifest in {}",
            index_path.display()
        )
    })?;
    Ok(indexed_messages)
}

pub(super) fn current_database_revision(connection: &Connection) -> anyhow::Result<i64> {
    Ok(connection.query_row(
        "SELECT COALESCE(MAX(source_file_id), 0) FROM source_files",
        [],
        |row| row.get(0),
    )?)
}

pub(super) fn search_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field("corpus_id", STRING | STORED);
    builder.add_text_field("file_name", STRING | STORED);
    builder.add_text_field("message_id", STRING | STORED);
    builder.add_text_field("sender", TEXT | STORED);
    builder.add_text_field("date_title", STORED);
    builder.add_text_field("body", TEXT | STORED);
    builder.build()
}

pub(super) struct SearchFields {
    pub(super) corpus_id: tantivy::schema::Field,
    pub(super) file_name: tantivy::schema::Field,
    pub(super) message_id: tantivy::schema::Field,
    pub(super) sender: tantivy::schema::Field,
    pub(super) date_title: tantivy::schema::Field,
    pub(super) body: tantivy::schema::Field,
}

impl SearchFields {
    fn from_schema(schema: &Schema) -> anyhow::Result<Self> {
        Ok(Self {
            corpus_id: schema.get_field("corpus_id")?,
            file_name: schema.get_field("file_name")?,
            message_id: schema.get_field("message_id")?,
            sender: schema.get_field("sender")?,
            date_title: schema.get_field("date_title")?,
            body: schema.get_field("body")?,
        })
    }
}

pub(super) fn stored_text(document: &TantivyDocument, field: tantivy::schema::Field) -> String {
    document
        .get_first(field)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string()
}

pub(super) fn normalize_search_query(query: &str) -> String {
    query
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
}

pub(super) fn message_preview(message: &str) -> String {
    let mut characters = message.chars();
    let preview = characters
        .by_ref()
        .take(MAX_MESSAGE_PREVIEW_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}
