//! Private persistence operations; blocking operator tooling.
use super::*;
pub(super) fn print_stats(database_path: &Path) -> anyhow::Result<()> {
    let connection = open_existing_database(database_path)?;
    let mut statement = connection.prepare(
        "SELECT corpora.corpus_id, corpora.source_type,
                count(DISTINCT source_files.source_file_id), count(message_revisions.revision_id),
                sum(CASE WHEN message_revisions.date_title IS NULL THEN 1 ELSE 0 END),
                sum(CASE WHEN message_revisions.reply_to_message_id IS NOT NULL THEN 1 ELSE 0 END)
         FROM corpora
         LEFT JOIN source_files ON source_files.corpus_id = corpora.corpus_id
         LEFT JOIN message_revisions ON message_revisions.source_file_id = source_files.source_file_id
         GROUP BY corpora.corpus_id, corpora.source_type
         ORDER BY corpora.corpus_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })?;
    println!("corpus\ttype\tsources\tmessages\tmissing_date\treplies");
    for row in rows {
        let (corpus, source_type, sources, messages, missing_dates, replies) = row?;
        println!("{corpus}\t{source_type}\t{sources}\t{messages}\t{missing_dates}\t{replies}");
    }
    Ok(())
}

pub(super) fn validate_database(database_path: &Path) -> anyhow::Result<()> {
    let connection = open_existing_database(database_path)?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        bail!("SQLite integrity check failed: {integrity}");
    }
    let foreign_key_errors = connection
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |_| Ok(()))?
        .count();
    if foreign_key_errors != 0 {
        bail!("SQLite foreign key check found {foreign_key_errors} violation(s)");
    }
    let duplicate_source_messages: i64 = connection.query_row(
        "SELECT count(*) FROM (
            SELECT source_file_id, source_message_id
            FROM message_revisions
            GROUP BY source_file_id, source_message_id
            HAVING count(*) > 1
        )",
        [],
        |row| row.get(0),
    )?;
    if duplicate_source_messages != 0 {
        bail!("found {duplicate_source_messages} duplicate source message identity group(s)");
    }
    println!("Support database is valid: {}", database_path.display());
    Ok(())
}

pub(super) fn open_existing_database(database_path: &Path) -> anyhow::Result<Connection> {
    if !database_path.is_file() {
        bail!(
            "support database does not exist: {}",
            database_path.display()
        );
    }
    let connection = Connection::open(database_path)
        .with_context(|| format!("failed to open database {}", database_path.display()))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(connection)
}
