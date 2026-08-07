//! Cache keys.
//!
//! A file's report is a pure function of (file bytes, profile, tool version), so all
//! three go into the key. Bumping the version or changing a threshold therefore
//! invalidates everything, by construction, rather than by anyone remembering to.

use crate::profile::Profile;
use crate::VERSION;
use sha2::{Digest, Sha256};

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

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
    let patterns = profile.test_patterns.sorted();
    let excludes = profile.exclude_patterns.sorted();
    let exempt = profile.test_exempt_rules.sorted();
    for group in [&patterns, &excludes, &exempt] {
        for pattern in group.iter() {
            hasher.update(pattern.as_bytes());
            hasher.update([0]);
        }
        hasher.update([1]);
    }
    let digest = hasher.finalize();
    hex(&digest[..8])
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
