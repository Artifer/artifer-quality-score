//! Content-addressed report cache.
//!
//! | module | holds |
//! |---|---|
//! | [`keys`] | the content hash and profile fingerprint |
//! | [`store`] | the `Cache` trait and the no-op implementation |
//! | [`sqlite`] | the real store |

pub mod keys;
pub mod sqlite;
pub mod store;

pub use keys::{content_key, profile_fingerprint};
pub use sqlite::ReportCache;
pub use store::{Cache, NullCache};

use crate::profile::Profile;
use std::path::Path;

pub const CACHE_DIRNAME: &str = ".aqs";
pub const CACHE_FILENAME: &str = "cache.sqlite3";

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS reports (
    key TEXT PRIMARY KEY,
    report TEXT NOT NULL
) WITHOUT ROWID;";

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
        assert_ne!(
            profile_fingerprint(&profile),
            profile_fingerprint(&stricter)
        );
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
