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
    use crate::models::FileReport;

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
        cache.put("anything", &report());
        cache.commit();
        assert!(cache.get("anything").is_none());
        let _ = std::fs::remove_file(&directory);
    }

    fn report() -> FileReport {
        FileReport::unsupported("a.py".into(), "python".into(), "unread".into())
    }

    #[test]
    fn a_report_survives_a_round_trip_once_committed() {
        let directory = std::env::temp_dir().join("aqs-cache-test-round-trip");
        let _ = std::fs::remove_dir_all(&directory);
        let mut cache = ReportCache::open(&directory, &Profile::default());
        assert!(cache.enabled());
        assert_eq!(
            cache.fingerprint(),
            keys::profile_fingerprint(&Profile::default())
        );

        cache.put("key", &report());
        assert!(
            cache.get("key").is_none(),
            "nothing is stored before commit"
        );
        cache.commit();
        assert_eq!(cache.get("key"), Some(report()));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_entry_from_another_schema_reads_as_a_miss() {
        let directory = std::env::temp_dir().join("aqs-cache-test-schema");
        let _ = std::fs::remove_dir_all(&directory);
        let mut cache = ReportCache::open(&directory, &Profile::default());
        rusqlite::Connection::open(directory.join(CACHE_FILENAME))
            .expect("open")
            .execute(
                "INSERT INTO reports (key, report) VALUES ('old', '{\"v\": 1}')",
                [],
            )
            .expect("insert");
        assert!(cache.get("old").is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_null_cache_stores_nothing() {
        let mut cache = open_cache(None, &Profile::default(), true);
        assert!(!cache.enabled());
        assert_eq!(cache.fingerprint(), "");
        cache.put("key", &report());
        cache.commit();
        assert!(cache.get("key").is_none());
    }
}
