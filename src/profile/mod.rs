//! Thresholds and penalty weights — the entire scoring policy.
//!
//! | module | holds |
//! |---|---|
//! | [`rules`] | rule identifiers, `RuleConfig`, and the default tables |
//! | [`patterns`] | `PatternSet` and the default test/exclude patterns |
//! | [`config`] | discovering and applying an `.aqs.json` |

pub mod config;
pub mod patterns;
pub mod rules;

pub use config::{detect_shadcn_ui, find_config, load_profile, CONFIG_FILENAMES};
pub use patterns::PatternSet;
pub use rules::*;

use patterns::{EXCLUDE_PATTERNS, TEST_EXEMPT_RULES, TEST_PATTERNS};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub thresholds: BTreeMap<String, f64>,
    pub rules: BTreeMap<String, RuleConfig>,
    /// Patterns identifying test code, where the assertion rules apply and size limits
    /// are relaxed.
    pub test_patterns: PatternSet,
    /// Patterns excluded from scanning entirely.
    pub exclude_patterns: PatternSet,
    /// Rules that do not apply to test code. Test classes are *supposed* to hold
    /// independent methods that share no state, so design rules about cohesion fire
    /// constantly on a perfectly good test suite.
    pub test_exempt_rules: PatternSet,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "artifer".to_string(),
            thresholds: THRESHOLD_KEYS
                .iter()
                .map(|(key, value)| ((*key).to_string(), *value))
                .collect(),
            rules: RULE_DEFAULTS
                .iter()
                .map(|(key, value)| ((*key).to_string(), *value))
                .collect(),
            test_patterns: TEST_PATTERNS.iter().map(|s| s.to_string()).collect(),
            exclude_patterns: EXCLUDE_PATTERNS.iter().map(|s| s.to_string()).collect(),
            test_exempt_rules: TEST_EXEMPT_RULES.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Profile {
    pub fn threshold(&self, key: &str) -> f64 {
        self.thresholds[key]
    }

    pub fn rule(&self, category: &str) -> &RuleConfig {
        &self.rules[category]
    }

    pub fn is_test_exempt(&self, category: &str) -> bool {
        self.test_exempt_rules.contains(category)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use std::path::{Path, PathBuf};

    #[test]
    fn penalty_grows_past_the_threshold_then_caps() {
        let config = RuleConfig { base: 1.0, scale: 10.0, cap: 2.5, file_cap: 9.0, weight: 1.0 };
        assert_eq!(config.penalty(10.0, 10.0), 1.0);
        assert_eq!(config.penalty(20.0, 10.0), 2.0);
        assert_eq!(config.penalty(1000.0, 10.0), 2.5);
    }

    #[test]
    fn zero_weight_is_free() {
        let config = RuleConfig { base: 1.0, scale: 10.0, cap: 5.0, file_cap: 9.0, weight: 0.0 };
        assert_eq!(config.penalty(100.0, 10.0), 0.0);
    }

    #[test]
    fn recognises_test_and_excluded_paths() {
        let profile = Profile::default();
        assert!(profile.test_patterns.matches("apps/core/tests/test_models.py"));
        assert!(profile.test_patterns.matches("src/thing.spec.ts"));
        assert!(!profile.test_patterns.matches("apps/core/models.py"));

        assert!(profile.exclude_patterns.matches("node_modules/react/index.js"));
        assert!(profile.exclude_patterns.matches("src/components/ui/button.tsx"));
        assert!(profile.exclude_patterns.matches("src/types/api.d.ts"));
        assert!(profile.exclude_patterns.matches("reference/generate_deck.py"));
        assert!(profile.exclude_patterns.matches("tools/lib/reference/analysis_catalogue.py"));
        assert!(!profile.exclude_patterns.matches("src/components/TenderTable.tsx"));
        // Only the directory is excluded — a file merely named for it is still scored.
        assert!(!profile.exclude_patterns.matches("src/reference_data.py"));
        assert!(!profile.exclude_patterns.matches("src/lib/references/model.py"));
    }

    #[test]
    fn every_rule_has_documentation_and_defaults() {
        let profile = Profile::default();
        assert_eq!(profile.rules.len(), RULE_DEFAULTS.len());
        assert_eq!(profile.thresholds.len(), THRESHOLD_KEYS.len());
    }
}

#[cfg(test)]
mod shadcn_tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str, components_json: Option<&str>) -> PathBuf {
        let root = std::env::temp_dir().join(format!("aqs-shadcn-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("mkdir");
        if let Some(body) = components_json {
            std::fs::write(root.join("components.json"), body).expect("write");
        }
        root
    }

    #[test]
    fn reads_the_declared_ui_directory() {
        // Three of twelve Artifer frontends use @/shared/ui rather than
        // @/components/ui, so the location has to be read, not assumed.
        for (alias, expected) in [
            ("@/shared/ui", "shared/ui"),
            ("@/components/ui", "components/ui"),
            ("~/app/primitives", "app/primitives"),
            ("shared/ui", "shared/ui"),
        ] {
            let root = scratch("alias", Some(&format!(r#"{{"aliases":{{"ui":"{alias}"}}}}"#)));
            assert_eq!(detect_shadcn_ui(&root).as_deref(), Some(expected), "{alias}");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn a_declared_ui_directory_is_excluded() {
        let root = scratch("exclude", Some(r#"{"aliases":{"ui":"@/app/primitives"}}"#));
        let profile = load_profile(&root, None).expect("loads");
        assert!(profile.exclude_patterns.matches("app/primitives/button.tsx"));
        assert!(profile.exclude_patterns.matches("src/app/primitives/button.tsx"));
        // Unrelated code is untouched.
        assert!(!profile.exclude_patterns.matches("src/app/page.tsx"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn both_common_layouts_work_without_components_json() {
        // Scanning a subdirectory may not see components.json at all, so the two
        // observed layouts are also static defaults.
        let profile = Profile::default();
        assert!(profile.exclude_patterns.matches("src/shared/ui/avatar.tsx"));
        assert!(profile.exclude_patterns.matches("src/components/ui/button.tsx"));
        assert!(profile.exclude_patterns.matches("shared/ui/tabs.tsx"));
        // A `shared/` directory that is not the ui folder stays in scope.
        assert!(!profile.exclude_patterns.matches("src/shared/api/client.ts"));
        assert!(!profile.exclude_patterns.matches("src/shared/i18n/messages/en.ts"));
    }

    #[test]
    fn a_missing_or_broken_components_json_is_not_fatal() {
        let none = scratch("none", None);
        assert!(detect_shadcn_ui(&none).is_none());
        assert!(load_profile(&none, None).is_ok());
        let _ = std::fs::remove_dir_all(&none);

        let broken = scratch("broken", Some("{not json"));
        assert!(detect_shadcn_ui(&broken).is_none());
        assert!(load_profile(&broken, None).is_ok());
        let _ = std::fs::remove_dir_all(&broken);

        let empty = scratch("empty", Some(r#"{"aliases":{}}"#));
        assert!(detect_shadcn_ui(&empty).is_none());
        let _ = std::fs::remove_dir_all(&empty);
    }
}
