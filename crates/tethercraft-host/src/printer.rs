//! Remote printer queue (Tier 4, Windows to Windows first).
//!
//! The spool queue, job states, and size caps are real. Talking to the Win32
//! print spooler is OS work and returns `NotImplemented`; nothing here claims
//! a page actually printed.

use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

/// One job may not exceed 256 MiB of spool data.
pub const MAX_JOB_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Printing,
    Done,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJob {
    pub id: u64,
    pub printer: String,
    pub doc: String,
    pub bytes_len: usize,
    pub state: JobState,
}

pub struct SpoolQueue {
    jobs: std::collections::HashMap<u64, PrintJob>,
    next_id: u64,
}

impl SpoolQueue {
    pub fn new() -> Self {
        Self {
            jobs: std::collections::HashMap::new(),
            next_id: 1,
        }
    }

    /// Accept a job into the queue. The caller must hold `Permission::Printer`.
    pub fn submit(
        &mut self,
        printer: &str,
        doc: &str,
        bytes_len: usize,
    ) -> Result<PrintJob, TetherError> {
        if printer.trim().is_empty() || doc.trim().is_empty() {
            return Err(TetherError::Invalid(
                "printer and document names required".to_string(),
            ));
        }
        if bytes_len == 0 || bytes_len > MAX_JOB_BYTES {
            return Err(TetherError::Invalid(
                "job must be 1 byte..=256 MiB".to_string(),
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let job = PrintJob {
            id,
            printer: printer.to_string(),
            doc: doc.to_string(),
            bytes_len,
            state: JobState::Queued,
        };
        self.jobs.insert(id, job.clone());
        Ok(job)
    }

    pub fn start(&mut self, id: u64) -> Result<(), TetherError> {
        match self.jobs.get_mut(&id) {
            Some(j) if j.state == JobState::Queued => {
                j.state = JobState::Printing;
                Ok(())
            }
            Some(_) => Err(TetherError::Invalid("job not queued".to_string())),
            None => Err(TetherError::Invalid("unknown job".to_string())),
        }
    }

    pub fn finish(&mut self, id: u64) -> Result<(), TetherError> {
        match self.jobs.get_mut(&id) {
            Some(j) if j.state == JobState::Printing => {
                j.state = JobState::Done;
                Ok(())
            }
            Some(_) => Err(TetherError::Invalid("job not printing".to_string())),
            None => Err(TetherError::Invalid("unknown job".to_string())),
        }
    }

    pub fn fail(&mut self, id: u64, reason: &str) -> Result<(), TetherError> {
        match self.jobs.get_mut(&id) {
            Some(j) => {
                j.state = JobState::Failed(reason.to_string());
                Ok(())
            }
            None => Err(TetherError::Invalid("unknown job".to_string())),
        }
    }

    /// Hand the spooled bytes to the OS spooler. Pending Win32 wiring.
    pub fn spool_to_os(&self, _id: u64) -> Result<(), TetherError> {
        Err(TetherError::NotImplemented(
            "Win32 print spooler wiring ships with the Tier 4 host service".to_string(),
        ))
    }

    pub fn queued(&self) -> usize {
        self.jobs
            .values()
            .filter(|j| j.state == JobState::Queued)
            .count()
    }
}

impl Default for SpoolQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_lifecycle() {
        let mut q = SpoolQueue::new();
        assert!(q.submit("", "d", 10).is_err());
        assert!(q.submit("p", "d", 0).is_err());
        assert!(q.submit("p", "d", MAX_JOB_BYTES + 1).is_err());
        let job = q
            .submit("OfficeJet", "report.pdf", 1024)
            .unwrap_or(PrintJob {
                id: 0,
                printer: String::new(),
                doc: String::new(),
                bytes_len: 0,
                state: JobState::Failed(String::new()),
            });
        assert!(q.queued() == 1);
        assert!(q.finish(job.id).is_err());
        assert!(q.start(job.id).is_ok());
        assert!(q.finish(job.id).is_ok());
        assert!(q.queued() == 0);
        assert!(q.spool_to_os(job.id).is_err());
    }

    #[test]
    fn fail_path() {
        let mut q = SpoolQueue::new();
        let job = q.submit("p", "d", 8).unwrap_or(PrintJob {
            id: 0,
            printer: String::new(),
            doc: String::new(),
            bytes_len: 0,
            state: JobState::Failed(String::new()),
        });
        assert!(q.fail(job.id, "paper jam").is_ok());
        assert!(q.start(999).is_err());
    }
}
