//! Non-sensitive operational measurements. Missing evidence is unknown, never zero lag.
use super::*;

/// Collector monitoring values contain elapsed seconds and no source contents.
#[derive(Debug, Default, Serialize)]
pub struct ObservationHealth {
    /// Whether the isolated observation database could be read.
    pub available: bool,
    /// Seconds since the latest durable checkpoint; None means no checkpoint exists.
    pub checkpoint_age_seconds: Option<i64>,
    /// Seconds since the newest observed execution; None means no observation exists.
    pub collector_lag_seconds: Option<i64>,
}

/// Reads aggregate timestamps on a blocking thread with a five-second busy timeout.
pub fn observation_health(path: &Path) -> anyhow::Result<ObservationHealth> {
    if !path.is_file() {
        return Ok(ObservationHealth::default());
    }
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    let checkpoint: Option<i64> = connection.query_row(
        "SELECT max(updated_at) FROM observation_collectors",
        [],
        |r| r.get(0),
    )?;
    let observed: Option<i64> = connection.query_row(
        "SELECT max(observed_at) FROM recent_observations",
        [],
        |r| r.get(0),
    )?;
    let now = now_seconds()?;
    Ok(ObservationHealth {
        available: true,
        checkpoint_age_seconds: checkpoint.map(|t| now.saturating_sub(t).max(0)),
        collector_lag_seconds: observed.map(|t| now.saturating_sub(t).max(0)),
    })
}
