//! Private parsing operations; blocking operator tooling.
use super::*;
pub(super) fn import_archive(export_dir: &Path, database_path: &Path) -> anyhow::Result<()> {
    if !export_dir.is_dir() {
        bail!("export directory does not exist: {}", export_dir.display());
    }
    if let Some(parent) = database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create database directory {}", parent.display()))?;
    }

    let mut sources = Vec::new();
    let mut missing_pages = Vec::new();
    for page in 1..=27 {
        let file_name = if page == 1 {
            "messages.html".to_string()
        } else {
            format!("messages{page}.html")
        };
        let path = export_dir.join(&file_name);
        if path.is_file() {
            sources.push(("support", path));
        } else {
            missing_pages.push(file_name);
        }
    }
    if sources.is_empty() {
        bail!(
            "no numbered Telegram message pages found in {}",
            export_dir.display()
        );
    }

    let updates_path = export_dir.join("messages_updates.html");
    if updates_path.is_file() {
        sources.push(("announcements", updates_path));
    }

    let mut connection = Connection::open(database_path)
        .with_context(|| format!("failed to open database {}", database_path.display()))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    initialize_schema(&connection)?;

    let mut imported_files = 0;
    let mut unchanged_files = 0;
    let mut stored_messages = 0;
    let mut skipped_records = 0;
    for (corpus_id, path) in sources {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("invalid source filename: {}", path.display()))?;
        anyhow::ensure!(
            fs::metadata(&path)?.len() <= 50 * 1024 * 1024,
            "archive page exceeds 50 MiB"
        );
        let html = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let result = import_source(&mut connection, corpus_id, file_name, &html)
            .with_context(|| format!("failed to import {file_name}"))?;
        if result.imported {
            imported_files += 1;
            stored_messages += result.stored_messages;
            skipped_records += result.skipped_records;
            println!(
                "{corpus_id}/{file_name}: imported {} messages; skipped {} service, empty, or malformed records",
                result.stored_messages, result.skipped_records
            );
        } else {
            unchanged_files += 1;
        }
    }

    let reply_stats = resolve_reply_edges(&mut connection)?;
    print_reply_stats(&reply_stats);
    let entity_count = ensure_entity_candidates(&mut connection)?;
    println!("Entity candidates refreshed: {entity_count}");
    let candidate_stats = rebuild_candidate_cases(&mut connection)?;
    print_candidate_build_stats(&candidate_stats);
    reconcile_resolutions(&mut connection)?;

    println!(
        "Import complete: {imported_files} source versions imported, {unchanged_files} unchanged, {stored_messages} message revisions stored, {skipped_records} records skipped."
    );
    if !missing_pages.is_empty() {
        println!("Missing numbered pages: {}", missing_pages.join(", "));
    }
    println!("Private database: {}", database_path.display());
    Ok(())
}
