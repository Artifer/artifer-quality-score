//! Thresholds and penalty weights — the entire scoring policy, in one place.
//!
//! The rule taxonomy follows CodeScene's published code-health factors. The weights
//! are ours: theirs are calibrated against proprietary baseline data, so there is no
//! public number to copy.
//!
//! Threshold keys reuse CodeScene's public naming where a published equivalent
//! exists, so anyone who has written a `code-health-rules.json` finds them familiar.

use crate::glob::{fnmatch, strip_glob_chars};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// Rule identifiers. These strings are the public contract: they appear in JSON
// output, in `aqs explain`, and as keys in user config.
pub const COMPLEX_METHOD: &str = "Complex Method";
pub const LARGE_METHOD: &str = "Large Method";
pub const DEEP_NESTED_COMPLEXITY: &str = "Deep, Nested Complexity";
pub const BUMPY_ROAD: &str = "Bumpy Road Ahead";
pub const COMPLEX_CONDITIONAL: &str = "Complex Conditional";
pub const EXCESS_ARGUMENTS: &str = "Excess Number of Function Arguments";
pub const BRAIN_METHOD: &str = "Brain Method";
pub const CONSTRUCTOR_OVER_INJECTION: &str = "Constructor Over-Injection";
pub const STRING_HEAVY_ARGUMENTS: &str = "String Heavy Function Arguments";
pub const PRIMITIVE_OBSESSION: &str = "Primitive Obsession";
pub const CODE_DUPLICATION: &str = "Code Duplication";
pub const LARGE_ASSERTION_BLOCKS: &str = "Large Assertion Blocks";
pub const DUPLICATED_ASSERTION_BLOCKS: &str = "Duplicated Assertion Blocks";
pub const FILE_SIZE: &str = "Lines of Code in a Single File";
pub const DECLARATION_SIZE: &str = "Lines of Declarations in a Single File";
pub const FUNCTION_COUNT: &str = "Number of Functions in a Single Module";
pub const OVERALL_COMPLEXITY: &str = "Overall Code Complexity";
pub const BRAIN_CLASS: &str = "Brain Class";
pub const LOW_COHESION: &str = "Low Cohesion";

/// How much one rule can cost a file.
///
/// A finding costs `base` points when it fires exactly at the threshold, then grows
/// linearly — one further `base` per `scale` units past the threshold — until it hits
/// `cap`. Growth matters: a metric an LLM optimises against needs to keep rewarding
/// progress on a bad function, not go flat the moment it stops being the worst
/// offender.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RuleConfig {
    /// Points deducted for a finding sitting exactly at the threshold.
    pub base: f64,
    /// Units past the threshold that add one more `base` worth of penalty.
    pub scale: f64,
    /// Ceiling for a single finding.
    pub cap: f64,
    /// Ceiling for all findings of this rule in one file. Stops a file with 40
    /// mediocre functions from being indistinguishable from one with 400.
    pub file_cap: f64,
    /// 0.0 disables the rule, 1.0 is full strength.
    pub weight: f64,
}

impl RuleConfig {
    pub fn penalty(&self, value: f64, threshold: f64) -> f64 {
        if self.weight <= 0.0 {
            return 0.0;
        }
        let excess = (value - threshold).max(0.0);
        let growth = if self.scale > 0.0 { excess / self.scale } else { 0.0 };
        (self.base * (1.0 + growth)).min(self.cap) * self.weight
    }
}

const fn rule(base: f64, scale: f64, cap: f64, file_cap: f64) -> RuleConfig {
    RuleConfig { base, scale, cap, file_cap, weight: 1.0 }
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub thresholds: BTreeMap<String, f64>,
    pub rules: BTreeMap<String, RuleConfig>,
    /// Glob patterns identifying test code, where the assertion rules apply and size
    /// limits are relaxed.
    pub test_patterns: Vec<String>,
    /// Glob patterns excluded from scanning entirely.
    pub exclude_patterns: Vec<String>,
    /// Rules that do not apply to test code. Test classes are *supposed* to hold
    /// independent methods that share no state, so design rules about cohesion fire
    /// constantly on a perfectly good test suite.
    pub test_exempt_rules: Vec<String>,
}

pub const THRESHOLD_KEYS: &[(&str, f64)] = &[
    // Function level
    ("function_cyclomatic_complexity_warning", 10.0),
    ("function_nesting_depth_warning", 3.0),
    ("function_lines_of_code_warning", 60.0),
    ("function_max_arguments", 4.0),
    ("constructor_max_arguments", 5.0),
    ("function_string_arguments_warning", 3.0),
    ("function_primitive_arguments_min_count", 4.0),
    ("function_primitive_arguments_ratio", 0.75),
    // Bumpy Road: a chunk counts as a bump at this nesting depth, and the rule fires
    // once a function contains this many bumps.
    ("bumpy_road_min_bump_depth", 2.0),
    ("bumpy_road_min_bumps", 2.0),
    // A conditional is "complex" at this many boolean operators; the rule fires once
    // a function holds this many such conditionals.
    ("complex_conditional_min_operators", 2.0),
    ("complex_conditional_min_count", 1.0),
    // Brain Method is compound: all three must hold at once.
    ("brain_method_lines_of_code", 45.0),
    ("brain_method_cyclomatic_complexity", 12.0),
    ("brain_method_nesting_depth", 3.0),
    // Duplication
    ("function_duplication_min_lines_of_code_for_check", 15.0),
    ("function_duplication_min_similarity_percentage", 85.0),
    // Test code
    ("assertion_block_max_statements", 8.0),
    ("duplicated_assertion_block_min_statements", 3.0),
    // File / module level
    ("file_lines_of_code_warning", 500.0),
    ("file_declaration_lines_warning", 300.0),
    ("module_max_functions", 20.0),
    ("module_mean_complexity_warning", 6.0),
    ("class_max_lines_of_code", 300.0),
    ("class_max_functions", 15.0),
    ("class_max_lcom4", 1.0),
    // Test files carry this multiplier on size limits; long test files are normal and
    // punishing them teaches the wrong lesson.
    ("test_file_size_multiplier", 3.0),
];

pub const RULE_DEFAULTS: &[(&str, RuleConfig)] = &[
    // Complex Method and Deep Nesting carry the most weight: they are the two
    // factors with the strongest published link to defect risk.
    (COMPLEX_METHOD, rule(0.6, 10.0, 2.5, 5.0)),
    (DEEP_NESTED_COMPLEXITY, rule(0.8, 2.0, 2.5, 5.0)),
    (LARGE_METHOD, rule(0.5, 60.0, 2.0, 4.0)),
    (BUMPY_ROAD, rule(0.5, 2.0, 2.0, 4.0)),
    (COMPLEX_CONDITIONAL, rule(0.4, 2.0, 1.5, 3.0)),
    (EXCESS_ARGUMENTS, rule(0.4, 3.0, 1.5, 3.0)),
    (BRAIN_METHOD, rule(1.5, 15.0, 3.0, 4.5)),
    (CONSTRUCTOR_OVER_INJECTION, rule(0.5, 3.0, 1.5, 2.0)),
    (STRING_HEAVY_ARGUMENTS, rule(0.3, 2.0, 1.0, 2.0)),
    (PRIMITIVE_OBSESSION, rule(0.4, 0.0, 0.4, 1.6)),
    (CODE_DUPLICATION, rule(0.8, 0.0, 0.8, 3.0)),
    // Test-specific
    (LARGE_ASSERTION_BLOCKS, rule(0.3, 8.0, 1.0, 2.0)),
    (DUPLICATED_ASSERTION_BLOCKS, rule(0.4, 0.0, 0.4, 2.0)),
    // File / module level
    (FILE_SIZE, rule(0.5, 500.0, 2.0, 2.0)),
    (DECLARATION_SIZE, rule(0.3, 300.0, 1.0, 1.0)),
    (FUNCTION_COUNT, rule(0.4, 15.0, 1.5, 1.5)),
    (OVERALL_COMPLEXITY, rule(0.6, 4.0, 2.5, 2.5)),
    (BRAIN_CLASS, rule(1.5, 0.0, 1.5, 3.0)),
    (LOW_COHESION, rule(0.6, 2.0, 2.0, 2.0)),
];

const TEST_PATTERNS: &[&str] = &[
    "*/test_*.py",
    "test_*.py",
    "*_test.py",
    "*/tests/*",
    "tests/*",
    // Django's `manage.py startapp` layout puts an app's tests in a single
    // `app/tests.py`, which none of the patterns above match.
    "tests.py",
    "*/tests.py",
    "*/conftest.py",
    "conftest.py",
    "*.test.ts",
    "*.test.tsx",
    "*.test.js",
    "*.test.jsx",
    "*.spec.ts",
    "*.spec.tsx",
    "*.spec.js",
    "*.spec.jsx",
    "*/__tests__/*",
    "*/e2e/*",
];

const EXCLUDE_PATTERNS: &[&str] = &[
    "*/node_modules/*",
    "*/.venv/*",
    "*/venv/*",
    "*/.git/*",
    "*/__pycache__/*",
    "*/migrations/*",
    "*/.next/*",
    "*/dist/*",
    "*/build/*",
    "*/staticfiles/*",
    "*/site-packages/*",
    "*.min.js",
    "*.d.ts",
    "*/coverage/*",
    "*/.mypy_cache/*",
    "*/.pytest_cache/*",
    // Vendored shadcn/ui primitives. These are generated third-party components, and
    // their near-identical thin wrappers are that library's intended design, not
    // duplication anyone should "fix".
    "*/components/ui/*",
];

/// Validated against real test suites: every one of these fires on idiomatic,
/// well-written test code, so charging for them would only teach bad habits.
const TEST_EXEMPT_RULES: &[&str] = &[
    LOW_COHESION,
    BRAIN_CLASS,
    FUNCTION_COUNT,
    PRIMITIVE_OBSESSION,
    STRING_HEAVY_ARGUMENTS,
];

pub const CONFIG_FILENAMES: &[&str] = &[".aqs.json", ".aqs/config.json"];

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

    pub fn is_test_path(&self, path: &str) -> bool {
        self.test_patterns.iter().any(|pattern| fnmatch(path, pattern))
    }

    pub fn is_excluded(&self, path: &str) -> bool {
        for pattern in &self.exclude_patterns {
            if fnmatch(path, pattern) {
                return true;
            }
            let bare = strip_glob_chars(pattern);
            if path.split('/').any(|part| fnmatch(part, bare)) {
                return true;
            }
        }
        false
    }

    pub fn is_test_exempt(&self, category: &str) -> bool {
        self.test_exempt_rules.iter().any(|rule| rule == category)
    }
}

/// Nearest config file at or above `start`.
pub fn find_config(start: &Path) -> Option<PathBuf> {
    let mut directory: Option<&Path> = if start.is_dir() { Some(start) } else { start.parent() };
    while let Some(current) = directory {
        for filename in CONFIG_FILENAMES {
            let candidate = current.join(filename);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        directory = current.parent();
    }
    None
}

/// The default profile with any config file's overrides applied.
///
/// Unknown rule or threshold names are a hard error, never silently ignored: a typo
/// that quietly disables a rule is the worst possible failure mode for a gate.
pub fn load_profile(root: &Path, explicit: Option<&Path>) -> Result<Profile> {
    let config = match explicit {
        Some(path) => Some(path.to_path_buf()),
        None => find_config(root),
    };
    let Some(config) = config else {
        return Ok(Profile::default());
    };

    let text = std::fs::read_to_string(&config)
        .with_context(|| format!("{}: cannot read config", config.display()))?;
    let data: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("{}: invalid JSON", config.display()))?;
    let Some(object) = data.as_object() else {
        bail!("{}: expected a JSON object at the top level", config.display());
    };

    let mut profile = Profile::default();

    if let Some(name) = object.get("name").and_then(|v| v.as_str()) {
        profile.name = name.to_string();
    }

    if let Some(thresholds) = object.get("thresholds") {
        let Some(map) = thresholds.as_object() else {
            bail!("thresholds must be an object");
        };
        for (key, value) in map {
            if !profile.thresholds.contains_key(key) {
                bail!(
                    "unknown threshold {key:?}. Run `aqs explain --thresholds` for valid names."
                );
            }
            let Some(number) = value.as_f64() else {
                bail!("threshold {key:?} must be a number, got {value}");
            };
            profile.thresholds.insert(key.clone(), number);
        }
    }

    if let Some(rules) = object.get("rules") {
        let Some(map) = rules.as_object() else {
            bail!("rules must be an object");
        };
        for (name, value) in map {
            let Some(existing) = profile.rules.get(name).copied() else {
                bail!("unknown rule {name:?}. Run `aqs explain --list` for valid names.");
            };
            let mut updated = existing;
            if let Some(weight) = value.as_f64() {
                updated.weight = weight;
            } else if let Some(fields) = value.as_object() {
                for (field, raw) in fields {
                    let Some(number) = raw.as_f64() else {
                        bail!("rule {name:?} field {field:?} must be a number");
                    };
                    match field.as_str() {
                        "base" => updated.base = number,
                        "scale" => updated.scale = number,
                        "cap" => updated.cap = number,
                        "file_cap" => updated.file_cap = number,
                        "weight" => updated.weight = number,
                        other => bail!("rule {name:?} has unknown field(s): {other}"),
                    }
                }
            } else {
                bail!("rule {name:?} must be a number or an object");
            }
            profile.rules.insert(name.clone(), updated);
        }
    }

    if let Some(patterns) = object.get("test_patterns").and_then(|v| v.as_array()) {
        profile.test_patterns =
            patterns.iter().filter_map(|v| v.as_str().map(String::from)).collect();
    }
    if let Some(patterns) = object.get("exclude_patterns").and_then(|v| v.as_array()) {
        profile.exclude_patterns =
            patterns.iter().filter_map(|v| v.as_str().map(String::from)).collect();
    }
    if let Some(rules) = object.get("test_exempt_rules").and_then(|v| v.as_array()) {
        profile.test_exempt_rules =
            rules.iter().filter_map(|v| v.as_str().map(String::from)).collect();
    }

    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(profile.is_test_path("apps/core/tests/test_models.py"));
        assert!(profile.is_test_path("src/thing.spec.ts"));
        assert!(!profile.is_test_path("apps/core/models.py"));

        assert!(profile.is_excluded("node_modules/react/index.js"));
        assert!(profile.is_excluded("src/components/ui/button.tsx"));
        assert!(profile.is_excluded("src/types/api.d.ts"));
        assert!(!profile.is_excluded("src/components/TenderTable.tsx"));
    }

    #[test]
    fn every_rule_has_documentation_and_defaults() {
        let profile = Profile::default();
        assert_eq!(profile.rules.len(), RULE_DEFAULTS.len());
        assert_eq!(profile.thresholds.len(), THRESHOLD_KEYS.len());
    }
}
