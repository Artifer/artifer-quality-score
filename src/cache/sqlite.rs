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
    hits: usize,
    misses: usize,
}

impl ReportCache {
    pub fn open(directory: &Path, profile: &Profile) -> Self {
        let fingerprint = profile_fingerprint(profile);
        let connection = std::fs::create_dir_all(directory)
            .ok()
            .and_then(|_| Connection::open(directory.join(CACHE_FILENAME)).ok())
            .and_then(|connection| match connection.execute_batch(SCHEMA) {
                Ok(()) => Some(connection),
                Err(_) => None,
            });
        Self { connection, fingerprint, pending: Vec::new(), hits: 0, misses: 0 }
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
        let stored: Option<String> = connection
            .query_row("SELECT report FROM reports WHERE key = ?1", [key], |row| row.get(0))
            .ok();
        let Some(stored) = stored else {
            self.misses += 1;
            return None;
        };
        match serde_json::from_str::<FileReport>(&stored) {
            Ok(report) => {
                self.hits += 1;
                Some(report)
            }
            Err(_) => {
                // An entry written by an incompatible schema; treat as a miss.
                self.misses += 1;
                None
            }
        }
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
        if self.pending.is_empty() {
            return;
        }
        let Some(connection) = self.connection.as_mut() else {
            self.pending.clear();
            return;
        };
        if let Ok(transaction) = connection.transaction() {
            for (key, report) in &self.pending {
                let _ = transaction.execute(
                    "INSERT OR REPLACE INTO reports (key, report) VALUES (?1, ?2)",
                    [key, report],
                );
            }
            let _ = transaction.commit();
        }
        self.pending.clear();
    }

    fn hits(&self) -> usize {
        self.hits
    }

    fn misses(&self) -> usize {
        self.misses
    }
}
