//! Content-addressed report cache.
//!
//! A file's report is a pure function of (file bytes, profile, tool version). That
//! makes it trivially cacheable, and caching is where the real speed is: the baseline
//! side of a merge gate is an immutable commit, so after the first run every one of
//! its files is a cache hit.
//!
//! The key includes the tool version and a fingerprint of the profile, so bumping
//! either invalidates everything rather than silently mixing scores computed under
//! different rules.

use crate::models::FileReport;
use crate::profile::Profile;
use crate::VERSION;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::Path;

pub const CACHE_DIRNAME: &str = ".aqs";
pub const CACHE_FILENAME: &str = "cache.sqlite3";

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS reports (
    key TEXT PRIMARY KEY,
    report TEXT NOT NULL
) WITHOUT ROWID;";

/// A stable hash of everything in a profile that can change a score.
pub fn profile_fingerprint(profile: &Profile) -> String {
    // BTreeMap iteration is ordered, so this is stable without an explicit sort.
    let mut hasher = Sha256::new();
    hasher.update(profile.name.as_bytes());
    hasher.update([0]);
    for (key, value) in &profile.thresholds {
        hasher.update(key.as_bytes());
        hasher.update(format!("={value}").as_bytes());
        hasher.update([0]);
    }
    for (name, rule) in &profile.rules {
        hasher.update(name.as_bytes());
        hasher.update(
            format!(
                "={},{},{},{},{}",
                rule.base, rule.scale, rule.cap, rule.file_cap, rule.weight
            )
            .as_bytes(),
        );
        hasher.update([0]);
    }
    let mut patterns = profile.test_patterns.clone();
    patterns.sort();
    let mut excludes = profile.exclude_patterns.clone();
    excludes.sort();
    let mut exempt = profile.test_exempt_rules.clone();
    exempt.sort();
    for group in [&patterns, &excludes, &exempt] {
        for pattern in group {
            hasher.update(pattern.as_bytes());
            hasher.update([0]);
        }
        hasher.update([1]);
    }
    let digest = hasher.finalize();
    hex(&digest[..8])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Cache key for one file.
///
/// The path is part of the key because it decides test-vs-production treatment and is
/// echoed in the report, so the same bytes at two paths are two reports.
pub fn content_key(path: &str, source: &[u8], fingerprint: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(fingerprint.as_bytes());
    hasher.update([0]);
    hasher.update(path.as_bytes());
    hasher.update([0]);
    hasher.update(source);
    hex(&hasher.finalize())
}

pub trait Cache {
    fn enabled(&self) -> bool;
    fn fingerprint(&self) -> &str;
    fn get(&mut self, key: &str) -> Option<FileReport>;
    fn put(&mut self, key: &str, report: &FileReport);
    fn commit(&mut self);
    fn hits(&self) -> usize;
    fn misses(&self) -> usize;
}

/// Stand-in used when caching is switched off, so callers need no branches.
#[derive(Default)]
pub struct NullCache;

impl Cache for NullCache {
    fn enabled(&self) -> bool {
        false
    }
    fn fingerprint(&self) -> &str {
        ""
    }
    fn get(&mut self, _key: &str) -> Option<FileReport> {
        None
    }
    fn put(&mut self, _key: &str, _report: &FileReport) {}
    fn commit(&mut self) {}
    fn hits(&self) -> usize {
        0
    }
    fn misses(&self) -> usize {
        0
    }
}

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

pub fn open_cache(root: Option<&Path>, profile: &Profile, enabled: bool) -> Box<dyn Cache> {
    match (enabled, root) {
        (true, Some(root)) => Box::new(ReportCache::open(&root.join(CACHE_DIRNAME), profile)),
        _ => Box::new(NullCache),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_and_policy_sensitive() {
        let profile = Profile::default();
        assert_eq!(profile_fingerprint(&profile), profile_fingerprint(&profile));

        let mut stricter = Profile::default();
        stricter
            .thresholds
            .insert("function_cyclomatic_complexity_warning".into(), 3.0);
        assert_ne!(profile_fingerprint(&profile), profile_fingerprint(&stricter));
    }

    #[test]
    fn same_bytes_at_a_different_path_is_a_separate_entry() {
        let fingerprint = profile_fingerprint(&Profile::default());
        assert_ne!(
            content_key("app.py", b"x = 1", &fingerprint),
            content_key("test_app.py", b"x = 1", &fingerprint)
        );
    }

    #[test]
    fn unwritable_location_degrades_to_no_cache() {
        let directory = std::env::temp_dir().join("aqs-cache-test-file");
        std::fs::write(&directory, b"not a directory").expect("write blocker");
        let mut cache = ReportCache::open(&directory.join(CACHE_DIRNAME), &Profile::default());
        assert!(!cache.enabled());
        assert!(cache.get("anything").is_none());
        cache.commit();
        let _ = std::fs::remove_file(&directory);
    }
}
