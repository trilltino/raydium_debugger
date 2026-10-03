//! A bounded persistent writer and two read connections keep SQLite off executor threads.
//! All methods block; async callers must use `spawn_blocking`. Notifications follow commits.
use super::*;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    mpsc, Mutex,
};
use std::time::Duration;

type WriteJob = Box<dyn FnOnce(&mut Connection) + Send>;

/// Cloneable ledger handle. One writer accepts at most 128 queued operations.
#[derive(Clone)]
pub struct InvestigationStore {
    writer: mpsc::SyncSender<WriteJob>,
    readers: Arc<[Mutex<Connection>; 2]>,
    next_reader: Arc<AtomicUsize>,
    notifications: tokio::sync::broadcast::Sender<()>,
    contention: Arc<AtomicU64>,
}

impl std::fmt::Debug for InvestigationStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvestigationStore").finish_non_exhaustive()
    }
}

fn open(path: &Path) -> anyhow::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
    Ok(connection)
}

impl InvestigationStore {
    /// Opens the ledger, applies transactional migrations and interrupts unfinished runs.
    /// Must be initialized once per process, on a blocking thread, before accepting work.
    pub fn from_path(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut connection = open(&path)?;
        migrate(&mut connection)?;
        recover(&mut connection)?;
        prune(&mut connection)?;
        let readers = Arc::new([Mutex::new(open(&path)?), Mutex::new(open(&path)?)]);
        let (writer, receiver) = mpsc::sync_channel::<WriteJob>(128);
        let (notifications, _) = tokio::sync::broadcast::channel(128);
        let notify = notifications.clone();
        std::thread::Builder::new()
            .name("investigation-ledger".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    job(&mut connection);
                    // A lagged or disconnected UI never blocks the durable writer.
                    let _ = notify.send(());
                }
            })?;
        Ok(Self {
            writer,
            readers,
            next_reader: Arc::new(AtomicUsize::new(0)),
            notifications,
            contention: Arc::new(AtomicU64::new(0)),
        })
    }

    fn write<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> anyhow::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let contention = self.contention.clone();
        self.writer
            .try_send(Box::new(move |connection| {
                let result = operation(connection);
                if result.as_ref().err().and_then(|e| e.downcast_ref::<rusqlite::Error>()).is_some_and(|e| matches!(e, rusqlite::Error::SqliteFailure(code, _) if matches!(code.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))) {
                    contention.fetch_add(1, Ordering::Relaxed);
                }
                let _ = sender.send(result);
            }))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => {
                    anyhow::Error::new(crate::ServiceError::Capacity("ledger_queue"))
                }
                mpsc::TrySendError::Disconnected(_) => anyhow::anyhow!("ledger writer unavailable"),
            })?;
        receiver.recv().context("ledger writer stopped")?
    }

    fn read<T>(
        &self,
        operation: impl FnOnce(&Connection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let index = self.next_reader.fetch_add(1, Ordering::Relaxed) % 2;
        let connection = self.readers[index]
            .lock()
            .map_err(|_| anyhow::anyhow!("ledger read connection unavailable"))?;
        operation(&connection)
    }

    /// Number of exhausted SQLite busy/locked operations, with no private details.
    pub fn contention_count(&self) -> u64 {
        self.contention.load(Ordering::Relaxed)
    }

    /// Subscribes before reading a backlog; notifications are hints, never the event source.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.notifications.subscribe()
    }

    /// Reads a durable result. Missing IDs return `None`, never a new investigation.
    pub fn lookup(&self, id: &str) -> anyhow::Result<Option<InvestigationLookup>> {
        self.read(|connection| {
            connection
                .query_row(
                    "SELECT status,result_json FROM investigations WHERE investigation_id=?1",
                    [id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?
                .map(|(status, json)| {
                    Ok(InvestigationLookup {
                        investigation_id: id.into(),
                        status,
                        result: json.map(|value| serde_json::from_str(&value)).transpose()?,
                    })
                })
                .transpose()
        })
    }

    /// Reads the original input for an explicit retry; the retry gets a new ID and time.
    pub fn request(&self, id: &str) -> anyhow::Result<Option<InvestigationRequest>> {
        self.read(|connection| {
            connection.query_row("SELECT signature,symptom,cluster,recent_fingerprint FROM investigations WHERE investigation_id=?1",[id],|row| {
                let cluster: Option<String> = row.get(2)?;
                Ok(InvestigationRequest { signature:row.get(0)?,symptom:row.get(1)?,cluster:cluster.and_then(|c|match c.as_str(){"devnet"=>Some(RpcCluster::Devnet),"mainnet"=>Some(RpcCluster::Mainnet),_=>None}),recent_fingerprint:row.get(3)? })
            }).optional().map_err(Into::into)
        })
    }

    /// Returns at most 128 ordered events after a global monotonic ID. IDs serialize as strings.
    pub fn events_after(&self, id: &str, after: u64) -> anyhow::Result<Vec<InvestigationEvent>> {
        let after = i64::try_from(after).context("event cursor exceeds SQLite range")?;
        self.read(|connection| {
            let mut statement = connection.prepare_cached("SELECT event_id,event_type,stage,detail,event_json FROM investigation_events WHERE investigation_id=?1 AND event_id>?2 ORDER BY event_id LIMIT 128")?;
            let rows = statement.query_map(params![id,after], |row| {
                Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?,row.get::<_,Option<String>>(2)?,row.get::<_,Option<String>>(3)?,row.get::<_,Option<String>>(4)?))
            })?;
            rows.map(|row| {
                let (event_id,event_type,stage,message,json) = row?;
                let mut event = if let Some(json) = json { serde_json::from_str(&json)? } else {
                    InvestigationEvent { event_id:String::new(),event_type,investigation_id:id.into(),stage,message,result:None,error:None }
                };
                event.event_id = event_id.to_string();
                Ok(event)
            }).collect()
        })
    }

    /// Commits accepted status and the first progress event together.
    pub fn begin(&self, id: &str, request: &InvestigationRequest) -> anyhow::Result<()> {
        let id = id.to_owned();
        let request = request.clone();
        self.write(move |connection| {
            let transaction = connection.transaction()?;
            let now=now_seconds()?;
            transaction.execute("INSERT INTO investigations(investigation_id,signature,symptom,cluster,recent_fingerprint,status,created_at) VALUES (?1,?2,?3,?4,?5,'running',?6)", params![id,request.signature,request.symptom,request.cluster.map(RpcCluster::as_str),request.recent_fingerprint,now])?;
            append(&transaction, InvestigationEvent { event_id:String::new(),event_type:"progress".into(),investigation_id:id,stage:Some("accepted".into()),message:Some("Investigation accepted".into()),result:None,error:None },now)?;
            transaction.commit()?;Ok(())
        })
    }

    /// Persists progress before waking subscribers. Late progress after termination is ignored.
    pub fn record_event(&self, event: &InvestigationEvent) -> anyhow::Result<()> {
        let event = event.clone();
        anyhow::ensure!(
            event.event_type == "progress",
            "terminal events require a terminal transaction"
        );
        self.write(move |connection| {
            let transaction=connection.transaction()?;
            let running:bool=transaction.query_row("SELECT EXISTS(SELECT 1 FROM investigations WHERE investigation_id=?1 AND status='running')",[&event.investigation_id],|r|r.get(0))?;
            if running { append(&transaction,event,now_seconds()?)?; }
            transaction.commit()?;Ok(())
        })
    }

    /// Atomically commits evidence, result, status and exactly one terminal event.
    /// A result arriving after timeout or interruption cannot overwrite terminal state.
    pub fn complete(&self, result: &InvestigationResult) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(result.status.as_str(), "complete" | "partial"),
            "invalid completion status"
        );
        let result = result.clone();
        self.write(move |connection| {
            let transaction=connection.transaction()?;let now=now_seconds()?;
            let changed=transaction.execute("UPDATE investigations SET status=?1,completed_at=?2,result_json=?3 WHERE investigation_id=?4 AND status='running'",params![result.status,now,serde_json::to_string(&result)?,result.investigation_id])?;
            if changed>0 {
                {
                    let mut insert=transaction.prepare_cached("INSERT INTO investigation_evidence VALUES (?1,?2,?3,?4,?5,?6)")?;
                    for evidence in &result.evidence { insert.execute(params![evidence.evidence_id,result.investigation_id,evidence.evidence_type,evidence.source_reference,evidence.summary,evidence.observed_at])?; }
                }
                append(&transaction,InvestigationEvent { event_id:String::new(),event_type:"complete".into(),investigation_id:result.investigation_id.clone(),stage:Some("complete".into()),message:Some("Investigation complete".into()),result:Some(result),error:None },now)?;
            }
            transaction.commit()?;prune(connection)?;Ok(())
        })
    }

    /// Terminates failed work atomically; repeated or late transitions have no effect.
    pub fn fail(&self, id: &str) -> anyhow::Result<()> {
        self.terminate(id, InvestigationStatus::Failed)
    }

    /// Records timeout or shutdown interruption without repeating RPC work.
    pub fn terminate(&self, id: &str, status: InvestigationStatus) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(
                status,
                InvestigationStatus::Failed
                    | InvestigationStatus::TimedOut
                    | InvestigationStatus::Interrupted
            ),
            "invalid terminal transition"
        );
        let id = id.to_owned();
        self.write(move |connection| {
            let transaction = connection.transaction()?;
            terminate(&transaction, &id, status, now_seconds()?)?;
            transaction.commit()?;
            prune(connection)?;
            Ok(())
        })
    }
}

fn append(connection: &Connection, mut event: InvestigationEvent, now: i64) -> anyhow::Result<()> {
    let next: i64 = connection.query_row(
        "UPDATE investigation_sequence SET value=value+1 RETURNING value",
        [],
        |r| r.get(0),
    )?;
    event.event_id = next.to_string();
    connection.execute("INSERT INTO investigation_events(event_id,investigation_id,event_type,stage,detail,created_at,event_json) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![next,event.investigation_id,event.event_type,event.stage,event.message,now,serde_json::to_string(&event)?])?;
    Ok(())
}

fn terminate(
    connection: &Connection,
    id: &str,
    status: InvestigationStatus,
    now: i64,
) -> anyhow::Result<()> {
    let changed=connection.execute("UPDATE investigations SET status=?1,completed_at=?2 WHERE investigation_id=?3 AND status='running'",params![status.as_str(),now,id])?;
    if changed > 0 {
        append(connection,InvestigationEvent { event_id:String::new(),event_type:"error".into(),investigation_id:id.into(),stage:Some(status.as_str().into()),message:None,result:None,error:Some(match status {InvestigationStatus::Interrupted=>"Investigation interrupted by process restart. Retry explicitly to start a new investigation.",InvestigationStatus::TimedOut=>"Investigation exceeded its deadline.",_=>"Investigation could not be completed."}.into()) },now)?;
    }
    Ok(())
}

fn migrate(connection: &mut Connection) -> anyhow::Result<()> {
    let transaction = connection.transaction()?;
    let version: i64 = transaction.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    anyhow::ensure!(
        version <= 1,
        "investigation ledger is newer than this application"
    );
    if version == 0 {
        transaction.execute_batch("CREATE TABLE IF NOT EXISTS investigations(investigation_id TEXT PRIMARY KEY,signature TEXT,symptom TEXT,cluster TEXT,status TEXT NOT NULL,created_at INTEGER NOT NULL,completed_at INTEGER,result_json TEXT);
            CREATE TABLE IF NOT EXISTS investigation_evidence(evidence_id TEXT PRIMARY KEY,investigation_id TEXT NOT NULL REFERENCES investigations(investigation_id) ON DELETE CASCADE,evidence_type TEXT NOT NULL,source_reference TEXT NOT NULL,summary TEXT NOT NULL,observed_at INTEGER);
            CREATE TABLE IF NOT EXISTS investigation_events(event_id INTEGER PRIMARY KEY,investigation_id TEXT NOT NULL REFERENCES investigations(investigation_id) ON DELETE CASCADE,event_type TEXT NOT NULL,stage TEXT,detail TEXT,created_at INTEGER NOT NULL);
            ALTER TABLE investigations ADD COLUMN recent_fingerprint TEXT;
            ALTER TABLE investigation_events ADD COLUMN event_json TEXT;
            CREATE INDEX investigation_events_by_run ON investigation_events(investigation_id,event_id);
            CREATE INDEX investigations_by_completion ON investigations(status,completed_at);
            CREATE TABLE investigation_sequence(value INTEGER NOT NULL);
            INSERT INTO investigation_sequence SELECT coalesce(max(event_id),0) FROM investigation_events;
            PRAGMA user_version=1;")?;
    }
    transaction.commit()?;
    Ok(())
}

fn recover(connection: &mut Connection) -> anyhow::Result<()> {
    let transaction = connection.transaction()?;
    let ids: Vec<String> = {
        let mut statement = transaction
            .prepare("SELECT investigation_id FROM investigations WHERE status='running'")?;
        let rows = statement.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let now = now_seconds()?;
    for id in ids {
        terminate(&transaction, &id, InvestigationStatus::Interrupted, now)?;
    }
    transaction.commit()?;
    Ok(())
}

fn prune(connection: &mut Connection) -> anyhow::Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "DELETE FROM investigations WHERE status!='running' AND completed_at<?1",
        [now_seconds()? - 30 * 24 * 60 * 60],
    )?;
    // Running work is never pruned; retain the newest terminal history within the cap.
    transaction.execute_batch("DELETE FROM investigations WHERE investigation_id IN (SELECT investigation_id FROM investigations WHERE status!='running' ORDER BY completed_at DESC,investigation_id DESC LIMIT -1 OFFSET 10000);")?;
    transaction.commit()?;
    Ok(())
}
