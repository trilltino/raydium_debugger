//! SQLite persistence for integrator casebooks and saved transaction signatures.

use anyhow::{anyhow, Context};
use raydium_debugger::RpcCluster;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Signature;
use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const LEGACY_STORE_PATH: &str = ".raydium-debugger/integrators.json";
const DEFAULT_CASEBOOK_PATH: &str = ".raydium-debugger/casebooks.sqlite";
const GENERAL_CASEBOOK: &str = "General";
/// Maximum serialized size accepted when importing the legacy JSON store.
pub const MAX_STORE_BYTES: u64 = 2 * 1024 * 1024;
/// Maximum number of integrator records kept locally.
pub const MAX_INTEGRATORS: usize = 250;
/// Maximum saved transaction signatures per integrator.
pub const MAX_SIGNATURES_PER_INTEGRATOR: usize = 2_000;
/// Maximum classification tags stored with one saved signature.
pub const MAX_TAGS_PER_SIGNATURE: usize = 12;
/// Maximum display name length for an integrator.
pub const MAX_INTEGRATOR_NAME_CHARS: usize = 96;
/// Maximum casebook name length.
pub const MAX_CASEBOOK_NAME_CHARS: usize = 96;
/// Maximum contact field length for an integrator.
pub const MAX_CONTACT_CHARS: usize = 160;
/// Maximum free-form notes length for integrators/signatures/casebooks.
pub const MAX_NOTES_CHARS: usize = 2_000;
/// Maximum human label length for a saved signature.
pub const MAX_LABEL_CHARS: usize = 120;
/// Maximum explanation length for why a signature was saved.
pub const MAX_REASON_CHARS: usize = 500;
/// Maximum short outcome/status value length for a saved signature.
pub const MAX_OUTCOME_CHARS: usize = 32;
/// Maximum product/category/code string length on saved signatures.
pub const MAX_CLASSIFICATION_CHARS: usize = 80;
/// Maximum length for a single signature tag.
pub const MAX_TAG_CHARS: usize = 40;

/// Local SQLite-backed store for integrator casebooks and transaction signatures.
#[derive(Debug, Clone)]
pub struct SignatureStore {
    path: Arc<PathBuf>,
}

/// Legacy JSON document shape used only for one-time migration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntegratorDocument {
    pub integrators: Vec<IntegratorRecord>,
}

/// One integrator and its compatibility signature library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegratorRecord {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub contact: Option<String>,
    pub notes: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub signatures: Vec<SavedSignature>,
}

/// Casebook grouping saved transaction signatures for one integrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CasebookRecord {
    pub id: String,
    pub integrator_id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub owner_contact: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub signatures: Vec<SavedSignature>,
}

/// A saved transaction signature plus dev-rel context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSignature {
    pub id: String,
    pub signature: String,
    pub cluster: String,
    pub label: Option<String>,
    pub reason: Option<String>,
    pub outcome: Option<String>,
    pub product: Option<String>,
    pub failure_category: Option<String>,
    pub failure_code: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub pinned: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_debugged_at: Option<u64>,
}

/// Request body for creating an integrator record.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateIntegratorRequest {
    pub name: String,
    pub contact: Option<String>,
    pub notes: Option<String>,
}

/// Request body for creating a casebook.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCasebookRequest {
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub owner_contact: Option<String>,
}

/// Request body for saving a signature under an integrator or casebook.
#[derive(Debug, Clone, Deserialize)]
pub struct SaveSignatureRequest {
    pub signature: String,
    pub cluster: String,
    pub label: Option<String>,
    pub reason: Option<String>,
    pub outcome: Option<String>,
    pub product: Option<String>,
    pub failure_category: Option<String>,
    pub failure_code: Option<String>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
    pub pinned: Option<bool>,
}

/// Request body for updating saved signature context.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateSavedSignatureRequest {
    pub label: Option<String>,
    pub reason: Option<String>,
    pub outcome: Option<String>,
    pub product: Option<String>,
    pub failure_category: Option<String>,
    pub failure_code: Option<String>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
    pub pinned: Option<bool>,
}

impl SignatureStore {
    /// Opens the configured SQLite store and imports legacy JSON when present.
    pub fn from_env() -> anyhow::Result<Self> {
        let legacy_path = std::env::var("RAYDIUM_DEBUGGER_STORE_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);
        let path = std::env::var("RAYDIUM_DEBUGGER_CASEBOOK_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                legacy_path
                    .as_ref()
                    .filter(|path| {
                        path.extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| matches!(ext, "sqlite" | "db" | "sqlite3"))
                    })
                    .cloned()
            })
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CASEBOOK_PATH));
        let store = Self::from_path(path)?;
        if let Some(legacy_path) = legacy_path.as_deref() {
            if legacy_path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                store.import_legacy_if_empty(legacy_path)?;
            }
        }
        store.import_legacy_if_empty(Path::new(LEGACY_STORE_PATH))?;
        Ok(store)
    }

    /// Opens a SQLite store at a specific path and initializes the schema.
    pub fn from_path(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create store directory {}", parent.display())
            })?;
        }
        let store = Self {
            path: Arc::new(path),
        };
        store.with_conn(init_schema)?;
        Ok(store)
    }

    /// Returns all integrators with compatibility signatures flattened across casebooks.
    pub fn list(&self) -> anyhow::Result<Vec<IntegratorRecord>> {
        self.with_conn(load_integrators)
    }

    /// Creates a bounded integrator and a default `General` casebook.
    pub fn create_integrator(
        &self,
        request: CreateIntegratorRequest,
    ) -> anyhow::Result<IntegratorRecord> {
        let name =
            clean_required_bounded("integrator name", &request.name, MAX_INTEGRATOR_NAME_CHARS)?;
        let contact = clean_optional_bounded("contact", request.contact, MAX_CONTACT_CHARS)?;
        let notes = clean_optional_bounded("notes", request.notes, MAX_NOTES_CHARS)?;
        let now = now_secs();
        self.with_conn(|conn| {
            let count: i64 = conn.query_row("select count(*) from integrators", [], |row| row.get(0))?;
            if count as usize >= MAX_INTEGRATORS {
                return Err(anyhow!(
                    "integrator limit reached: maximum {MAX_INTEGRATORS} integrators"
                ));
            }
            let existing = load_integrators(conn)?;
            let id = uuid::Uuid::new_v4().to_string();
            let slug = unique_slug(&slugify(&name), &existing);
            conn.execute(
                "insert into integrators (id, name, slug, contact, notes, created_at, updated_at) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, name, slug, contact, notes, now, now],
            )?;
            ensure_general_casebook(conn, &id)?;
            get_integrator(conn, &id)
        })
    }

    /// Saves or updates a signature in the integrator's default casebook.
    pub fn save_signature(
        &self,
        integrator_id: &str,
        request: SaveSignatureRequest,
    ) -> anyhow::Result<IntegratorRecord> {
        let integrator_id = integrator_id.to_string();
        self.with_conn(|conn| {
            ensure_integrator_exists(conn, &integrator_id)?;
            let casebook_id = ensure_general_casebook(conn, &integrator_id)?;
            save_signature_in_casebook(conn, &casebook_id, request)?;
            get_integrator(conn, &integrator_id)
        })
    }

    /// Lists casebooks for one integrator.
    pub fn list_casebooks(&self, integrator_id: &str) -> anyhow::Result<Vec<CasebookRecord>> {
        let integrator_id = integrator_id.to_string();
        self.with_conn(|conn| {
            ensure_integrator_exists(conn, &integrator_id)?;
            load_casebooks(conn, Some(&integrator_id))
        })
    }

    /// Creates a casebook for one integrator.
    pub fn create_casebook(
        &self,
        integrator_id: &str,
        request: CreateCasebookRequest,
    ) -> anyhow::Result<CasebookRecord> {
        let integrator_id = integrator_id.to_string();
        let name = clean_required_bounded("casebook name", &request.name, MAX_CASEBOOK_NAME_CHARS)?;
        let description =
            clean_optional_bounded("description", request.description, MAX_NOTES_CHARS)?;
        let tags = clean_tags(request.tags)?;
        let owner_contact =
            clean_optional_bounded("owner contact", request.owner_contact, MAX_CONTACT_CHARS)?;
        let now = now_secs();
        self.with_conn(|conn| {
            ensure_integrator_exists(conn, &integrator_id)?;
            let record = CasebookRecord {
                id: uuid::Uuid::new_v4().to_string(),
                integrator_id,
                name,
                description,
                tags,
                owner_contact,
                created_at: now,
                updated_at: now,
                signatures: Vec::new(),
            };
            insert_casebook(conn, &record)?;
            Ok(record)
        })
    }

    /// Loads one casebook with signatures.
    pub fn get_casebook(&self, casebook_id: &str) -> anyhow::Result<CasebookRecord> {
        let casebook_id = casebook_id.to_string();
        self.with_conn(|conn| get_casebook(conn, &casebook_id))
    }

    /// Saves or updates a signature in a casebook.
    pub fn save_casebook_signature(
        &self,
        casebook_id: &str,
        request: SaveSignatureRequest,
    ) -> anyhow::Result<CasebookRecord> {
        let casebook_id = casebook_id.to_string();
        self.with_conn(|conn| {
            save_signature_in_casebook(conn, &casebook_id, request)?;
            get_casebook(conn, &casebook_id)
        })
    }

    /// Updates classification/context for an existing saved signature.
    pub fn update_casebook_signature(
        &self,
        casebook_id: &str,
        signature_id: &str,
        request: UpdateSavedSignatureRequest,
    ) -> anyhow::Result<CasebookRecord> {
        let casebook_id = casebook_id.to_string();
        let signature_id = signature_id.to_string();
        self.with_conn(|conn| {
            update_signature(conn, &casebook_id, &signature_id, request)?;
            get_casebook(conn, &casebook_id)
        })
    }

    fn import_legacy_if_empty(&self, legacy_path: &Path) -> anyhow::Result<()> {
        if !legacy_path.exists() {
            return Ok(());
        }
        if fs::metadata(legacy_path)?.len() > MAX_STORE_BYTES {
            return Err(anyhow!(
                "legacy integrator store is too large: exceeds {} byte limit",
                MAX_STORE_BYTES
            ));
        }
        let raw = fs::read_to_string(legacy_path)?;
        let document: IntegratorDocument = serde_json::from_str(&raw)?;
        validate_legacy_document(&document)?;
        self.with_conn(|conn| {
            let count: i64 = conn.query_row("select count(*) from integrators", [], |row| row.get(0))?;
            if count > 0 {
                return Ok(());
            }
            for integrator in document.integrators {
                conn.execute(
                    "insert into integrators (id, name, slug, contact, notes, created_at, updated_at) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        integrator.id,
                        integrator.name,
                        integrator.slug,
                        integrator.contact,
                        integrator.notes,
                        integrator.created_at,
                        integrator.updated_at
                    ],
                )?;
                let casebook_id = ensure_general_casebook(conn, &integrator.id)?;
                for saved in integrator.signatures {
                    insert_or_update_signature(conn, &casebook_id, saved)?;
                }
            }
            Ok(())
        })
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> anyhow::Result<T>) -> anyhow::Result<T> {
        let conn = Connection::open(self.path.as_ref())
            .with_context(|| format!("failed to open casebook store {}", self.path.display()))?;
        f(&conn)
    }
}

fn init_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "
        pragma foreign_keys = on;
        create table if not exists integrators (
            id text primary key,
            name text not null,
            slug text not null unique,
            contact text,
            notes text,
            created_at integer not null,
            updated_at integer not null
        );
        create table if not exists casebooks (
            id text primary key,
            integrator_id text not null references integrators(id) on delete cascade,
            name text not null,
            description text,
            tags_json text not null,
            owner_contact text,
            created_at integer not null,
            updated_at integer not null
        );
        create table if not exists signatures (
            id text primary key,
            casebook_id text not null references casebooks(id) on delete cascade,
            signature text not null,
            cluster text not null,
            label text,
            reason text,
            outcome text,
            product text,
            failure_category text,
            failure_code text,
            tags_json text not null,
            notes text,
            pinned integer not null default 0,
            created_at integer not null,
            updated_at integer not null,
            last_debugged_at integer,
            unique(casebook_id, signature, cluster)
        );
        ",
    )?;
    Ok(())
}

fn load_integrators(conn: &Connection) -> anyhow::Result<Vec<IntegratorRecord>> {
    let mut stmt = conn.prepare(
        "select id, name, slug, contact, notes, created_at, updated_at from integrators order by created_at asc",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(IntegratorRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            slug: row.get(2)?,
            contact: row.get(3)?,
            notes: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            signatures: Vec::new(),
        })
    })?;
    let mut records = rows.collect::<Result<Vec<_>, _>>()?;
    for record in &mut records {
        record.signatures = load_signatures_for_integrator(conn, &record.id)?;
    }
    Ok(records)
}

fn get_integrator(conn: &Connection, id: &str) -> anyhow::Result<IntegratorRecord> {
    let mut record = conn
        .query_row(
            "select id, name, slug, contact, notes, created_at, updated_at from integrators where id = ?1",
            params![id],
            |row| {
                Ok(IntegratorRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    slug: row.get(2)?,
                    contact: row.get(3)?,
                    notes: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    signatures: Vec::new(),
                })
            },
        )
        .optional()?
        .ok_or_else(|| anyhow!("integrator not found"))?;
    record.signatures = load_signatures_for_integrator(conn, id)?;
    Ok(record)
}

fn ensure_integrator_exists(conn: &Connection, id: &str) -> anyhow::Result<()> {
    conn.query_row(
        "select id from integrators where id = ?1",
        params![id],
        |_| Ok(()),
    )
    .optional()?
    .ok_or_else(|| anyhow!("integrator not found"))
}

fn ensure_general_casebook(conn: &Connection, integrator_id: &str) -> anyhow::Result<String> {
    if let Some(id) = conn
        .query_row(
            "select id from casebooks where integrator_id = ?1 and name = ?2",
            params![integrator_id, GENERAL_CASEBOOK],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let now = now_secs();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "insert into casebooks (id, integrator_id, name, description, tags_json, owner_contact, created_at, updated_at) values (?1, ?2, ?3, null, '[]', null, ?4, ?5)",
        params![id, integrator_id, GENERAL_CASEBOOK, now, now],
    )?;
    Ok(id)
}

fn insert_casebook(conn: &Connection, record: &CasebookRecord) -> anyhow::Result<()> {
    conn.execute(
        "insert into casebooks (id, integrator_id, name, description, tags_json, owner_contact, created_at, updated_at) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            record.id,
            record.integrator_id,
            record.name,
            record.description,
            serde_json::to_string(&record.tags)?,
            record.owner_contact,
            record.created_at,
            record.updated_at
        ],
    )?;
    Ok(())
}

fn load_casebooks(
    conn: &Connection,
    integrator_id: Option<&str>,
) -> anyhow::Result<Vec<CasebookRecord>> {
    let sql = if integrator_id.is_some() {
        "select id, integrator_id, name, description, tags_json, owner_contact, created_at, updated_at from casebooks where integrator_id = ?1 order by created_at asc"
    } else {
        "select id, integrator_id, name, description, tags_json, owner_contact, created_at, updated_at from casebooks order by created_at asc"
    };
    let mut stmt = conn.prepare(sql)?;
    let mut records = if let Some(integrator_id) = integrator_id {
        stmt.query_map(params![integrator_id], casebook_from_row)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], casebook_from_row)?
            .collect::<Result<Vec<_>, _>>()?
    };
    for record in &mut records {
        record.signatures = load_signatures_for_casebook(conn, &record.id)?;
    }
    Ok(records)
}

fn get_casebook(conn: &Connection, id: &str) -> anyhow::Result<CasebookRecord> {
    let mut record = conn
        .query_row(
            "select id, integrator_id, name, description, tags_json, owner_contact, created_at, updated_at from casebooks where id = ?1",
            params![id],
            casebook_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("casebook not found"))?;
    record.signatures = load_signatures_for_casebook(conn, id)?;
    Ok(record)
}

fn casebook_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CasebookRecord> {
    let tags_json: String = row.get(4)?;
    Ok(CasebookRecord {
        id: row.get(0)?,
        integrator_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        owner_contact: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        signatures: Vec::new(),
    })
}

fn save_signature_in_casebook(
    conn: &Connection,
    casebook_id: &str,
    request: SaveSignatureRequest,
) -> anyhow::Result<SavedSignature> {
    let saved = validated_saved_signature(request)?;
    ensure_casebook_exists(conn, casebook_id)?;
    let count: i64 = conn.query_row(
        "select count(*) from signatures where casebook_id in (select id from casebooks where integrator_id = (select integrator_id from casebooks where id = ?1))",
        params![casebook_id],
        |row| row.get(0),
    )?;
    let existing: Option<String> = conn
        .query_row(
            "select id from signatures where casebook_id = ?1 and signature = ?2 and cluster = ?3",
            params![casebook_id, saved.signature, saved.cluster],
            |row| row.get(0),
        )
        .optional()?;
    if existing.is_none() && count as usize >= MAX_SIGNATURES_PER_INTEGRATOR {
        return Err(anyhow!(
            "signature limit reached: maximum {MAX_SIGNATURES_PER_INTEGRATOR} signatures per integrator"
        ));
    }
    insert_or_update_signature(conn, casebook_id, saved)
}

fn insert_or_update_signature(
    conn: &Connection,
    casebook_id: &str,
    mut saved: SavedSignature,
) -> anyhow::Result<SavedSignature> {
    let now = now_secs();
    if let Some(existing_id) = conn
        .query_row(
            "select id from signatures where casebook_id = ?1 and signature = ?2 and cluster = ?3",
            params![casebook_id, saved.signature, saved.cluster],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        saved.id = existing_id;
        saved.updated_at = now;
        saved.last_debugged_at = Some(now);
        conn.execute(
            "update signatures set label = ?1, reason = ?2, outcome = ?3, product = ?4, failure_category = ?5, failure_code = ?6, tags_json = ?7, notes = ?8, pinned = ?9, updated_at = ?10, last_debugged_at = ?11 where id = ?12",
            params![
                saved.label,
                saved.reason,
                saved.outcome,
                saved.product,
                saved.failure_category,
                saved.failure_code,
                serde_json::to_string(&saved.tags)?,
                saved.notes,
                saved.pinned as i64,
                saved.updated_at,
                saved.last_debugged_at,
                saved.id
            ],
        )?;
    } else {
        conn.execute(
            "insert into signatures (id, casebook_id, signature, cluster, label, reason, outcome, product, failure_category, failure_code, tags_json, notes, pinned, created_at, updated_at, last_debugged_at) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                saved.id,
                casebook_id,
                saved.signature,
                saved.cluster,
                saved.label,
                saved.reason,
                saved.outcome,
                saved.product,
                saved.failure_category,
                saved.failure_code,
                serde_json::to_string(&saved.tags)?,
                saved.notes,
                saved.pinned as i64,
                saved.created_at,
                saved.updated_at,
                saved.last_debugged_at,
            ],
        )?;
    }
    Ok(saved)
}

fn update_signature(
    conn: &Connection,
    casebook_id: &str,
    signature_id: &str,
    request: UpdateSavedSignatureRequest,
) -> anyhow::Result<()> {
    ensure_casebook_exists(conn, casebook_id)?;
    let existing = conn
        .query_row(
            "select signature, cluster from signatures where id = ?1 and casebook_id = ?2",
            params![signature_id, casebook_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("saved signature not found"))?;
    let saved = validated_saved_signature(SaveSignatureRequest {
        signature: existing.0,
        cluster: existing.1,
        label: request.label,
        reason: request.reason,
        outcome: request.outcome,
        product: request.product,
        failure_category: request.failure_category,
        failure_code: request.failure_code,
        tags: request.tags,
        notes: request.notes,
        pinned: request.pinned,
    })?;
    conn.execute(
        "update signatures set label = ?1, reason = ?2, outcome = ?3, product = ?4, failure_category = ?5, failure_code = ?6, tags_json = ?7, notes = ?8, pinned = ?9, updated_at = ?10 where id = ?11 and casebook_id = ?12",
        params![
            saved.label,
            saved.reason,
            saved.outcome,
            saved.product,
            saved.failure_category,
            saved.failure_code,
            serde_json::to_string(&saved.tags)?,
            saved.notes,
            saved.pinned as i64,
            now_secs(),
            signature_id,
            casebook_id
        ],
    )?;
    Ok(())
}

fn ensure_casebook_exists(conn: &Connection, id: &str) -> anyhow::Result<()> {
    conn.query_row(
        "select id from casebooks where id = ?1",
        params![id],
        |_| Ok(()),
    )
    .optional()?
    .ok_or_else(|| anyhow!("casebook not found"))
}

fn load_signatures_for_integrator(
    conn: &Connection,
    integrator_id: &str,
) -> anyhow::Result<Vec<SavedSignature>> {
    let mut stmt = conn.prepare(
        "select s.id, s.signature, s.cluster, s.label, s.reason, s.outcome, s.product, s.failure_category, s.failure_code, s.tags_json, s.notes, s.pinned, s.created_at, s.updated_at, s.last_debugged_at
         from signatures s join casebooks c on c.id = s.casebook_id
         where c.integrator_id = ?1 order by s.pinned desc, s.updated_at desc",
    )?;
    let rows = stmt.query_map(params![integrator_id], saved_signature_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn load_signatures_for_casebook(
    conn: &Connection,
    casebook_id: &str,
) -> anyhow::Result<Vec<SavedSignature>> {
    let mut stmt = conn.prepare(
        "select id, signature, cluster, label, reason, outcome, product, failure_category, failure_code, tags_json, notes, pinned, created_at, updated_at, last_debugged_at
         from signatures where casebook_id = ?1 order by pinned desc, updated_at desc",
    )?;
    let rows = stmt.query_map(params![casebook_id], saved_signature_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn saved_signature_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedSignature> {
    let tags_json: String = row.get(9)?;
    Ok(SavedSignature {
        id: row.get(0)?,
        signature: row.get(1)?,
        cluster: row.get(2)?,
        label: row.get(3)?,
        reason: row.get(4)?,
        outcome: row.get(5)?,
        product: row.get(6)?,
        failure_category: row.get(7)?,
        failure_code: row.get(8)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        notes: row.get(10)?,
        pinned: row.get::<_, i64>(11)? != 0,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        last_debugged_at: row.get(14)?,
    })
}

fn validated_saved_signature(request: SaveSignatureRequest) -> anyhow::Result<SavedSignature> {
    let now = now_secs();
    Ok(SavedSignature {
        id: uuid::Uuid::new_v4().to_string(),
        signature: parse_signature(&request.signature)?,
        cluster: parse_cluster(&request.cluster)?.as_str().to_string(),
        label: clean_optional_bounded("label", request.label, MAX_LABEL_CHARS)?,
        reason: clean_optional_bounded("reason", request.reason, MAX_REASON_CHARS)?,
        outcome: clean_optional_bounded("outcome", request.outcome, MAX_OUTCOME_CHARS)?,
        product: clean_optional_bounded("product", request.product, MAX_CLASSIFICATION_CHARS)?,
        failure_category: clean_optional_bounded(
            "failure category",
            request.failure_category,
            MAX_CLASSIFICATION_CHARS,
        )?,
        failure_code: clean_optional_bounded(
            "failure code",
            request.failure_code,
            MAX_CLASSIFICATION_CHARS,
        )?,
        tags: clean_tags(request.tags)?,
        notes: clean_optional_bounded("notes", request.notes, MAX_NOTES_CHARS)?,
        pinned: request.pinned.unwrap_or(false),
        created_at: now,
        updated_at: now,
        last_debugged_at: Some(now),
    })
}

fn clean_required(field: &str, value: &str) -> anyhow::Result<String> {
    let cleaned = value.trim().to_string();
    if cleaned.is_empty() {
        return Err(anyhow!("{field} is required"));
    }
    Ok(cleaned)
}

fn clean_required_bounded(field: &str, value: &str, max_chars: usize) -> anyhow::Result<String> {
    let cleaned = clean_required(field, value)?;
    ensure_max_chars(field, &cleaned, max_chars)?;
    Ok(cleaned)
}

fn clean_optional_bounded(
    field: &str,
    value: Option<String>,
    max_chars: usize,
) -> anyhow::Result<Option<String>> {
    let Some(cleaned) = value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    ensure_max_chars(field, &cleaned, max_chars)?;
    Ok(Some(cleaned))
}

fn ensure_max_chars(field: &str, value: &str, max_chars: usize) -> anyhow::Result<()> {
    let count = value.chars().count();
    if count > max_chars {
        return Err(anyhow!(
            "{field} is too long: {count} characters exceeds {max_chars} character limit"
        ));
    }
    Ok(())
}

fn parse_signature(value: &str) -> anyhow::Result<String> {
    let signature = clean_required("transaction signature", value)?;
    Signature::from_str(&signature).map_err(|_| anyhow!("invalid Solana transaction signature"))?;
    Ok(signature)
}

fn parse_cluster(value: &str) -> anyhow::Result<RpcCluster> {
    match clean_required("cluster", value)?.as_str() {
        "devnet" => Ok(RpcCluster::Devnet),
        "mainnet" => Ok(RpcCluster::Mainnet),
        _ => Err(anyhow!("cluster must be devnet or mainnet")),
    }
}

fn clean_tags(tags: Option<Vec<String>>) -> anyhow::Result<Vec<String>> {
    let tags = tags.unwrap_or_default();
    if tags.len() > MAX_TAGS_PER_SIGNATURE {
        return Err(anyhow!(
            "too many tags: {} exceeds {MAX_TAGS_PER_SIGNATURE} tag limit",
            tags.len()
        ));
    }
    tags.into_iter()
        .filter_map(|tag| {
            let cleaned = tag.trim().to_string();
            (!cleaned.is_empty()).then_some(cleaned)
        })
        .map(|tag| clean_required_bounded("tag", &tag, MAX_TAG_CHARS))
        .collect()
}

fn validate_legacy_document(document: &IntegratorDocument) -> anyhow::Result<()> {
    if document.integrators.len() > MAX_INTEGRATORS {
        return Err(anyhow!(
            "integrator store has {} integrators, exceeding limit {MAX_INTEGRATORS}",
            document.integrators.len()
        ));
    }
    for integrator in &document.integrators {
        ensure_max_chars(
            "integrator name",
            &integrator.name,
            MAX_INTEGRATOR_NAME_CHARS,
        )?;
        if integrator.signatures.len() > MAX_SIGNATURES_PER_INTEGRATOR {
            return Err(anyhow!(
                "integrator {} has {} signatures, exceeding limit {MAX_SIGNATURES_PER_INTEGRATOR}",
                integrator.id,
                integrator.signatures.len()
            ));
        }
        for saved in &integrator.signatures {
            parse_signature(&saved.signature)?;
            parse_cluster(&saved.cluster)?;
        }
    }
    Ok(())
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn unique_slug(base: &str, integrators: &[IntegratorRecord]) -> String {
    let base = if base.is_empty() { "integrator" } else { base };
    let mut candidate = base.to_string();
    let mut index = 2;
    while integrators.iter().any(|record| record.slug == candidate) {
        candidate = format!("{base}-{index}");
        index += 1;
    }
    candidate
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "raydium-debugger-{name}-{}.sqlite",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn persists_integrators_and_default_casebook() {
        let path = test_path("integrators");
        let store = SignatureStore::from_path(&path).unwrap();
        let created = store
            .create_integrator(CreateIntegratorRequest {
                name: "Jupiter".to_string(),
                contact: Some("devrel@example.com".to_string()),
                notes: None,
            })
            .unwrap();

        let reloaded = SignatureStore::from_path(&path).unwrap();
        let integrators = reloaded.list().unwrap();
        assert_eq!(integrators.len(), 1);
        assert_eq!(integrators[0].id, created.id);
        assert_eq!(integrators[0].slug, "jupiter");
        assert_eq!(
            reloaded.list_casebooks(&created.id).unwrap()[0].name,
            GENERAL_CASEBOOK
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn upserts_signatures_by_cluster() {
        let path = test_path("signatures");
        let store = SignatureStore::from_path(&path).unwrap();
        let integrator = store
            .create_integrator(CreateIntegratorRequest {
                name: "Integrator".to_string(),
                contact: None,
                notes: None,
            })
            .unwrap();

        let signature = Signature::new_unique().to_string();
        store
            .save_signature(
                &integrator.id,
                SaveSignatureRequest {
                    signature: signature.clone(),
                    cluster: "mainnet".to_string(),
                    label: Some("first".to_string()),
                    reason: None,
                    outcome: Some("failed".to_string()),
                    product: Some("cpmm".to_string()),
                    failure_category: None,
                    failure_code: None,
                    tags: Some(vec!["swap".to_string()]),
                    notes: None,
                    pinned: Some(true),
                },
            )
            .unwrap();
        let updated = store
            .save_signature(
                &integrator.id,
                SaveSignatureRequest {
                    signature,
                    cluster: "mainnet".to_string(),
                    label: Some("renamed".to_string()),
                    reason: Some("slippage".to_string()),
                    outcome: Some("failed".to_string()),
                    product: Some("cpmm".to_string()),
                    failure_category: Some("price_or_slippage".to_string()),
                    failure_code: Some("0x1780".to_string()),
                    tags: None,
                    notes: None,
                    pinned: Some(false),
                },
            )
            .unwrap();

        assert_eq!(updated.signatures.len(), 1);
        assert_eq!(updated.signatures[0].label.as_deref(), Some("renamed"));
        assert_eq!(
            updated.signatures[0].failure_code.as_deref(),
            Some("0x1780")
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn casebook_create_save_and_update_work() {
        let path = test_path("casebook");
        let store = SignatureStore::from_path(&path).unwrap();
        let integrator = store
            .create_integrator(CreateIntegratorRequest {
                name: "Integrator".to_string(),
                contact: None,
                notes: None,
            })
            .unwrap();
        let casebook = store
            .create_casebook(
                &integrator.id,
                CreateCasebookRequest {
                    name: "Regressions".to_string(),
                    description: Some("known issues".to_string()),
                    tags: Some(vec!["failed".to_string()]),
                    owner_contact: None,
                },
            )
            .unwrap();
        let with_signature = store
            .save_casebook_signature(
                &casebook.id,
                SaveSignatureRequest {
                    signature: Signature::new_unique().to_string(),
                    cluster: "devnet".to_string(),
                    label: Some("case".to_string()),
                    reason: None,
                    outcome: Some("failed".to_string()),
                    product: Some("launch_lab".to_string()),
                    failure_category: Some("unknown".to_string()),
                    failure_code: None,
                    tags: Some(vec!["needs-idl".to_string()]),
                    notes: None,
                    pinned: Some(false),
                },
            )
            .unwrap();
        let signature_id = with_signature.signatures[0].id.clone();
        let updated = store
            .update_casebook_signature(
                &casebook.id,
                &signature_id,
                UpdateSavedSignatureRequest {
                    label: Some("updated".to_string()),
                    reason: None,
                    outcome: Some("regression".to_string()),
                    product: None,
                    failure_category: None,
                    failure_code: None,
                    tags: Some(vec!["regression".to_string()]),
                    notes: None,
                    pinned: Some(true),
                },
            )
            .unwrap();
        assert_eq!(updated.signatures[0].label.as_deref(), Some("updated"));
        assert!(updated.signatures[0].pinned);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_invalid_signature() {
        let path = test_path("invalid-signature");
        let store = SignatureStore::from_path(&path).unwrap();
        let integrator = store
            .create_integrator(CreateIntegratorRequest {
                name: "Integrator".to_string(),
                contact: None,
                notes: None,
            })
            .unwrap();

        let err = store
            .save_signature(
                &integrator.id,
                SaveSignatureRequest {
                    signature: "not-a-signature".to_string(),
                    cluster: "mainnet".to_string(),
                    label: None,
                    reason: None,
                    outcome: None,
                    product: None,
                    failure_category: None,
                    failure_code: None,
                    tags: None,
                    notes: None,
                    pinned: None,
                },
            )
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("invalid Solana transaction signature"));
        let _ = fs::remove_file(path);
    }
}
