//! What a cache must be able to do, and the no-op that satisfies it.

use crate::models::FileReport;

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
