//! The rule catalogue's identifiers, weights and thresholds.
//!
//! The taxonomy follows CodeScene's published code-health factors. The weights are ours:
//! theirs are calibrated against proprietary baseline data, so there is no public number
//! to copy.
//!
//! Threshold keys reuse CodeScene's public naming where a published equivalent exists,
//! so anyone who has written a `code-health-rules.json` finds them familiar.

use serde::Serialize;

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
pub const DUPLICATED_BLOCK: &str = "Duplicated Block";
pub const LARGE_ASSERTION_BLOCKS: &str = "Large Assertion Blocks";
pub const DUPLICATED_ASSERTION_BLOCKS: &str = "Duplicated Assertion Blocks";
pub const FILE_SIZE: &str = "Lines of Code in a Single File";
pub const DECLARATION_SIZE: &str = "Lines of Declarations in a Single File";
pub const FUNCTION_COUNT: &str = "Number of Functions in a Single Module";
pub const OVERALL_COMPLEXITY: &str = "Overall Code Complexity";
pub const BRAIN_CLASS: &str = "Brain Class";
pub const LOW_COHESION: &str = "Low Cohesion";

// Observation identifiers. Same contract as the rule names above — they appear in JSON,
// in `aqs explain`, and in `@aqs` directives — but they are deliberately absent from
// `RULE_DEFAULTS`, because an observation has no penalty to configure. See
// `models::Observation` for why some things are reported rather than priced.
pub const CROSS_FILE_DUPLICATION: &str = "Cross-File Duplication";

/// Every observation category. Directives validate names against this as well as against
/// the rules, so `@aqs(disable:"Cross-File Duplication")` is accepted and a typo is not.
pub const OBSERVATION_CATEGORIES: &[&str] = &[CROSS_FILE_DUPLICATION];

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
        let growth = if self.scale > 0.0 {
            excess / self.scale
        } else {
            0.0
        };
        (self.base * (1.0 + growth)).min(self.cap) * self.weight
    }
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
    // Cross-file matching is exact rather than fuzzy, so it can afford a much lower floor
    // than the intra-file rule: the same function in six files is worth knowing about at
    // 5 lines, where a 5-line near-match within one file would mostly be noise.
    ("cross_file_duplication_min_lines_of_code", 5.0),
    // Repeated blocks *within* a function-sized unit. The token floor is the real
    // control: three consecutive statements is a common enough shape that matching on
    // count alone reports coincidences, so a window must also be substantial.
    ("duplicated_block_min_statements", 3.0),
    // 30 is set from the canonical case rather than picked round: a log-plus-metric-plus-
    // return failure block — the shape this rule exists to catch — normalises to about 35
    // tokens, and a floor of 40 silently missed it. Three bare assignments come to ~12 and
    // stay below.
    ("duplicated_block_min_tokens", 30.0),
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
    (
        COMPLEX_METHOD,
        RuleConfig {
            base: 0.6,
            scale: 10.0,
            cap: 2.5,
            file_cap: 5.0,
            weight: 1.0,
        },
    ),
    (
        DEEP_NESTED_COMPLEXITY,
        RuleConfig {
            base: 0.8,
            scale: 2.0,
            cap: 2.5,
            file_cap: 5.0,
            weight: 1.0,
        },
    ),
    (
        LARGE_METHOD,
        RuleConfig {
            base: 0.5,
            scale: 60.0,
            cap: 2.0,
            file_cap: 4.0,
            weight: 1.0,
        },
    ),
    (
        BUMPY_ROAD,
        RuleConfig {
            base: 0.5,
            scale: 2.0,
            cap: 2.0,
            file_cap: 4.0,
            weight: 1.0,
        },
    ),
    (
        COMPLEX_CONDITIONAL,
        RuleConfig {
            base: 0.4,
            scale: 2.0,
            cap: 1.5,
            file_cap: 3.0,
            weight: 1.0,
        },
    ),
    (
        EXCESS_ARGUMENTS,
        RuleConfig {
            base: 0.4,
            scale: 3.0,
            cap: 1.5,
            file_cap: 3.0,
            weight: 1.0,
        },
    ),
    (
        BRAIN_METHOD,
        RuleConfig {
            base: 1.5,
            scale: 15.0,
            cap: 3.0,
            file_cap: 4.5,
            weight: 1.0,
        },
    ),
    (
        CONSTRUCTOR_OVER_INJECTION,
        RuleConfig {
            base: 0.5,
            scale: 3.0,
            cap: 1.5,
            file_cap: 2.0,
            weight: 1.0,
        },
    ),
    (
        STRING_HEAVY_ARGUMENTS,
        RuleConfig {
            base: 0.3,
            scale: 2.0,
            cap: 1.0,
            file_cap: 2.0,
            weight: 1.0,
        },
    ),
    (
        PRIMITIVE_OBSESSION,
        RuleConfig {
            base: 0.4,
            scale: 0.0,
            cap: 0.4,
            file_cap: 1.6,
            weight: 1.0,
        },
    ),
    (
        CODE_DUPLICATION,
        RuleConfig {
            base: 0.8,
            scale: 0.0,
            cap: 0.8,
            file_cap: 3.0,
            weight: 1.0,
        },
    ),
    // Cheaper per hit than a duplicated whole function, and grows with the copy count:
    // the same block in eight places is a different problem from the same block twice.
    (
        DUPLICATED_BLOCK,
        RuleConfig {
            base: 0.3,
            scale: 2.0,
            cap: 1.2,
            file_cap: 2.5,
            weight: 1.0,
        },
    ),
    // Test-specific
    (
        LARGE_ASSERTION_BLOCKS,
        RuleConfig {
            base: 0.3,
            scale: 8.0,
            cap: 1.0,
            file_cap: 2.0,
            weight: 1.0,
        },
    ),
    (
        DUPLICATED_ASSERTION_BLOCKS,
        RuleConfig {
            base: 0.4,
            scale: 0.0,
            cap: 0.4,
            file_cap: 2.0,
            weight: 1.0,
        },
    ),
    // File / module level
    (
        FILE_SIZE,
        RuleConfig {
            base: 0.5,
            scale: 500.0,
            cap: 2.0,
            file_cap: 2.0,
            weight: 1.0,
        },
    ),
    (
        DECLARATION_SIZE,
        RuleConfig {
            base: 0.3,
            scale: 300.0,
            cap: 1.0,
            file_cap: 1.0,
            weight: 1.0,
        },
    ),
    (
        FUNCTION_COUNT,
        RuleConfig {
            base: 0.4,
            scale: 15.0,
            cap: 1.5,
            file_cap: 1.5,
            weight: 1.0,
        },
    ),
    (
        OVERALL_COMPLEXITY,
        RuleConfig {
            base: 0.6,
            scale: 4.0,
            cap: 2.5,
            file_cap: 2.5,
            weight: 1.0,
        },
    ),
    (
        BRAIN_CLASS,
        RuleConfig {
            base: 1.5,
            scale: 0.0,
            cap: 1.5,
            file_cap: 3.0,
            weight: 1.0,
        },
    ),
    (
        LOW_COHESION,
        RuleConfig {
            base: 0.6,
            scale: 2.0,
            cap: 2.0,
            file_cap: 2.0,
            weight: 1.0,
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_penalty_grows_with_the_excess_and_is_then_weighted() {
        let rule = RuleConfig {
            base: 1.0,
            scale: 2.0,
            cap: 10.0,
            file_cap: 10.0,
            weight: 0.5,
        };
        // Four past the threshold is two scales: three times the base, at half weight.
        assert_eq!(rule.penalty(7.0, 3.0), 1.5);
        // The cap applies before the weight does.
        assert_eq!(rule.penalty(99.0, 3.0), 5.0);
    }
}
