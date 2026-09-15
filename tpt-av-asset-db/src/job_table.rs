//! Job queue persistence: the `jobs` table records pipeline job state so
//! unfinished work survives a crash and can be recovered on restart.

use redb::ReadableTable;
use tpt_av_asset_utils::{AssetError, AssetId, Priority};

use crate::schema::{asset_key, Dec, Enc};
use crate::transaction;
use crate::AssetDb;

/// Lifecycle state of a pipeline job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// Queued, not yet started.
    Pending,
    /// Currently executing on a worker.
    Running,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
    /// Cancelled before or during execution.
    Cancelled,
}

impl JobState {
    /// Stable wire tag.
    pub fn as_u8(self) -> u8 {
        match self {
            JobState::Pending => 0,
            JobState::Running => 1,
            JobState::Completed => 2,
            JobState::Failed => 3,
            JobState::Cancelled => 4,
        }
    }

    /// Inverse of [`JobState::as_u8`]; returns `None` for unknown tags.
    pub fn from_u8(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(JobState::Pending),
            1 => Some(JobState::Running),
            2 => Some(JobState::Completed),
            3 => Some(JobState::Failed),
            4 => Some(JobState::Cancelled),
            _ => None,
        }
    }

    /// True once the job has reached a final state.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobState::Completed | JobState::Failed | JobState::Cancelled
        )
    }
}

/// A persisted pipeline job record.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    /// Unique job id.
    pub job_id: u64,
    /// The asset this job processes.
    pub asset_id: AssetId,
    /// Queue priority.
    pub priority: Priority,
    /// Lifecycle state.
    pub state: JobState,
    /// Job kind (e.g. `"waveform"`, `"thumbnails"`, `"video_proxy"`).
    pub kind: String,
    /// Last reported completion fraction (`0.0..=1.0`).
    pub progress: f64,
    /// Generator-specific resume cursor (e.g. chunks already written).
    pub resume_hint: u64,
    /// Parameters needed to rebuild the job (job-kind specific encoding).
    pub payload: String,
    /// Record creation time (Unix ms).
    pub created_ms: u64,
    /// Last update time (Unix ms).
    pub updated_ms: u64,
    /// Error message when `state == JobState::Failed`.
    pub error: Option<String>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl AssetDb {
    /// Inserts or updates a job record (`updated_ms` is refreshed).
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn upsert_job(&self, record: &JobRecord) -> Result<(), AssetError> {
        let mut row = Enc::new();
        row.bytes(&asset_key(&record.asset_id));
        row.u8(record.priority.as_u8());
        row.u8(record.state.as_u8());
        row.str(&record.kind);
        row.f64(record.progress);
        row.u64(record.resume_hint);
        row.str(&record.payload);
        row.u64(record.created_ms);
        row.u64(record.updated_ms.max(now_ms()));
        row.opt_str(record.error.as_deref());
        let row = row.finish();

        transaction::with_write_txn(&self.db, |txn| {
            let mut table = txn
                .open_table(crate::schema::JOBS)
                .map_err(transaction::db_err)?;
            table
                .insert(record.job_id, row.as_slice())
                .map_err(transaction::db_err)?;
            Ok(())
        })
    }

    /// Retrieves a job record by id.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn get_job(&self, job_id: u64) -> Result<Option<JobRecord>, AssetError> {
        let txn = self.db.begin_read().map_err(transaction::db_err)?;
        let table = txn
            .open_table(crate::schema::JOBS)
            .map_err(transaction::db_err)?;
        match table.get(job_id).map_err(transaction::db_err)? {
            Some(row) => Ok(Some(decode_job_record(job_id, row.value())?)),
            None => Ok(None),
        }
    }

    /// Deletes a job record. Returns whether a record was removed.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn delete_job(&self, job_id: u64) -> Result<bool, AssetError> {
        transaction::with_write_txn(&self.db, |txn| {
            let mut table = txn
                .open_table(crate::schema::JOBS)
                .map_err(transaction::db_err)?;
            let removed = table.remove(job_id).map_err(transaction::db_err)?;
            Ok(removed.is_some())
        })
    }

    /// Lists all job records.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn all_jobs(&self) -> Result<Vec<JobRecord>, AssetError> {
        let txn = self.db.begin_read().map_err(transaction::db_err)?;
        let table = txn
            .open_table(crate::schema::JOBS)
            .map_err(transaction::db_err)?;
        let mut jobs = Vec::new();
        for row in table.iter().map_err(transaction::db_err)? {
            let (key, value) = row.map_err(transaction::db_err)?;
            jobs.push(decode_job_record(key.value(), value.value())?);
        }
        Ok(jobs)
    }

    /// Lists job records in the given state.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn jobs_in_state(&self, state: JobState) -> Result<Vec<JobRecord>, AssetError> {
        Ok(self
            .all_jobs()?
            .into_iter()
            .filter(|job| job.state == state)
            .collect())
    }
}

impl Enc {
    fn opt_str(&mut self, v: Option<&str>) {
        match v {
            Some(s) => {
                self.u8(1);
                self.str(s);
            }
            None => self.u8(0),
        }
    }
}

fn decode_opt_str(d: &mut Dec<'_>) -> Result<Option<String>, AssetError> {
    Ok(if d.u8()? == 1 { Some(d.str()?) } else { None })
}

/// Deserializes a [`JobRecord`] (job id comes from the table key).
///
/// # Errors
/// Returns [`AssetError::Db`] if the row is corrupt.
pub fn decode_job_record(job_id: u64, data: &[u8]) -> Result<JobRecord, AssetError> {
    let mut d = Dec::new(data);
    let asset_id = AssetId::from_key_bytes(d.bytes()?)?;
    let priority =
        Priority::from_u8(d.u8()?).ok_or_else(|| AssetError::db("corrupt row: bad priority"))?;
    let state =
        JobState::from_u8(d.u8()?).ok_or_else(|| AssetError::db("corrupt row: bad job state"))?;
    let kind = d.str()?;
    let progress = d.f64()?;
    let resume_hint = d.u64()?;
    let payload = d.str()?;
    let created_ms = d.u64()?;
    let updated_ms = d.u64()?;
    let error = decode_opt_str(&mut d)?;

    Ok(JobRecord {
        job_id,
        asset_id,
        priority,
        state,
        kind,
        progress,
        resume_hint,
        payload,
        created_ms,
        updated_ms,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_state_tags_and_terminality() {
        for tag in 0..5u8 {
            assert_eq!(JobState::from_u8(tag).unwrap().as_u8(), tag);
        }
        assert!(JobState::from_u8(5).is_none());
        assert!(!JobState::Running.is_terminal());
        assert!(JobState::Completed.is_terminal());
        assert!(JobState::Failed.is_terminal());
        assert!(JobState::Cancelled.is_terminal());
    }

    #[test]
    fn job_record_row_roundtrip() {
        let record = JobRecord {
            job_id: 42,
            asset_id: AssetId::from_parts(9, 8, 7),
            priority: Priority::High,
            state: JobState::Failed,
            kind: "waveform".into(),
            progress: 0.42,
            resume_hint: 12,
            payload: "/media/a.wav".into(),
            created_ms: 1_000,
            updated_ms: 2_000,
            error: Some("decode blew up".into()),
        };
        // Round-trip through the codec directly.
        let mut row = Enc::new();
        row.bytes(&asset_key(&record.asset_id));
        row.u8(record.priority.as_u8());
        row.u8(record.state.as_u8());
        row.str(&record.kind);
        row.f64(record.progress);
        row.u64(record.resume_hint);
        row.str(&record.payload);
        row.u64(record.created_ms);
        row.u64(record.updated_ms);
        row.opt_str(record.error.as_deref());
        let decoded = decode_job_record(record.job_id, &row.finish()).unwrap();
        assert_eq!(decoded, record);
    }
}
