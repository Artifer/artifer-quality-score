//! The sqlite-backed store.
//!
//! Failures are never fatal: a cache that cannot be opened or written degrades to no
//! caching at all. Being unable to write a cache file is not a reason to refuse to score
//! someone's code.

use super::keys::profile_fingerprint;
use super::store::Cache;
use super::{CACHE_FILENAME, SCHEMA};
use crate::models::FileReport;
use crate::profile::Profile;
use rusqlite::Connection;
use std::path::Path;

/// A sqlite-backed store of scored file reports.
///
/// Failures are never fatal: a cache that cannot be opened or written degrades to no
/// caching at all. Being unable to write a cache file is not a reason to refuse to
/// score someone's code.
pub struct ReportCache {
    connection: Option<Connection>,
    fingerprint: String,
    pending: Vec<(String, String)>,
}

impl ReportCache {
    pub fn open(directory: &Path, profile: &Profile) -> Self {
        let fingerprint = profile_fingerprint(profile);
        let connection = std::fs::create_dir_all(directory)
            .ok()
            .and_then(|_| Connection::open(directory.join(CACHE_FILENAME)).ok())
            .and_then(|connection| connection.execute_batch(SCHEMA).ok().map(|()| connection));
        Self {
            connection,
            fingerprint,
            pending: Vec::new(),
        }
    }
}

impl Cache for ReportCache {
    fn enabled(&self) -> bool {
        self.connection.is_some()
    }

    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn get(&mut self, key: &str) -> Option<FileReport> {
        let connection = self.connection.as_ref()?;
        let stored: String = connection
            .query_row("SELECT report FROM reports WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .ok()?;
        // An entry written by an incompatible schema reads as a miss.
        serde_json::from_str::<FileReport>(&stored).ok()
    }

    fn put(&mut self, key: &str, report: &FileReport) {
        if self.connection.is_none() {
            return;
        }
        if let Ok(encoded) = serde_json::to_string(report) {
            self.pending.push((key.to_string(), encoded));
        }
    }

    /// Flush pending writes in one transaction.
    fn commit(&mut self) {
        // Nothing is queued without a connection, so an empty batch covers both cases.
        let pending = std::mem::take(&mut self.pending);
        let connection = self.connection.as_mut().filter(|_| !pending.is_empty());
        if let Some(transaction) = connection.and_then(|c| c.transaction().ok()) {
            for (key, report) in &pending {
                let _ = transaction.execute(
                    "INSERT OR REPLACE INTO reports (key, report) VALUES (?1, ?2)",
                    [key, report],
                );
            }
            let _ = transaction.commit();
        }
    }
}
