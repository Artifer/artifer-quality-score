//! Path patterns, and the matching they own.
//!
//! Matching lives with the patterns rather than on `Profile`: a `PatternSet` knows how to
//! decide whether a path is one of its own, and `Profile` just holds three of them.

use super::rules::*;
use crate::glob::{fnmatch, strip_glob_chars};

/// A set of `fnmatch` patterns that can say whether a path belongs to it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatternSet(pub Vec<String>);

impl PatternSet {
    pub fn new<I, S>(patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(patterns.into_iter().map(Into::into).collect())
    }

    /// Whether `path` matches any pattern in the set.
    ///
    /// Beyond a plain `fnmatch`, each pattern is also matched against every individual
    /// path segment. Without that, a directory pattern like `*/e2e/*` needs a leading
    /// segment and so fails on `e2e/flow.ts` — which is exactly where Playwright puts its
    /// specs when you scan from the repository root. The result would be that the same
    /// file counts as test code or production code depending on the scan root.
    pub fn matches(&self, path: &str) -> bool {
        for pattern in &self.0 {
            if fnmatch(path, pattern) {
                return true;
            }
            let bare = strip_glob_chars(pattern);
            if !bare.is_empty() && path.split('/').any(|part| fnmatch(part, bare)) {
                return true;
            }
        }
        false
    }

    pub fn contains(&self, value: &str) -> bool {
        self.0.iter().any(|item| item == value)
    }

    pub fn sorted(&self) -> Vec<&String> {
        let mut items: Vec<&String> = self.0.iter().collect();
        items.sort();
        items
    }

    pub fn push_if_absent(&mut self, pattern: String) {
        if !self.0.contains(&pattern) {
            self.0.push(pattern);
        }
    }
}

pub const TEST_PATTERNS: &[&str] = &[
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
    // Go's toolchain *requires* this suffix -- `go test` compiles nothing else -- so it
    // is the one test-path convention a project cannot deviate from.
    "*_test.go",
    // Maven and Gradle both put tests under `src/test/java`, which the `*/test/*`
    // pattern below already covers; these are the file-name conventions JUnit runners
    // and the Surefire/Failsafe plugins look for.
    "*Test.java",
    "*Tests.java",
    "*TestCase.java",
    "*IT.java",
    // PHPUnit's own convention, and the `tests/` directory the skeleton generates,
    // which the `*/tests/*` patterns above already cover.
    "*Test.php",
    // RSpec and minitest. `spec/` is the RSpec default and `test/` the minitest one;
    // neither is matched by the `tests/` patterns above.
    "*_spec.rb",
    "*_test.rb",
    "*/spec/*",
    "spec/*",
    "*/test/*",
    "test/*",
];

pub const EXCLUDE_PATTERNS: &[&str] = &[
    "*/node_modules/*",
    "*/.venv/*",
    "*/venv/*",
    "*/.git/*",
    "*/__pycache__/*",
    "*/migrations/*",
    "*/.next/*",
    // `next build && next export` writes its bundles to `out/_next/`, not `.next/`, so
    // the pattern above misses a static export entirely. These are webpack chunks —
    // minified, machine-generated, and nothing but repeated blocks: one docs site
    // contributed 130 Duplicated Block findings before this was excluded.
    "*/_next/*",
    "_next/*",
    "*/dist/*",
    "*/build/*",
    "*/staticfiles/*",
    "*/site-packages/*",
    "*.min.js",
    "*.d.ts",
    "*/coverage/*",
    // `go mod vendor` copies whole dependency trees into the repository. It is other
    // people's code, checked in verbatim, and scoring it would drown the project's own.
    "*/vendor/*",
    "vendor/*",
    // protoc and grpc-gateway write these; nobody edits them.
    "*.pb.go",
    "*.pb.gw.go",
    // Rails writes `db/schema.rb` from the migrations on every migrate, and it is
    // thousands of lines of repeated `create_table` blocks. The migrations themselves
    // are the Rails spelling of the `*/migrations/*` exclusion above.
    "db/schema.rb",
    "*/db/schema.rb",
    "db/migrate/*",
    "*/db/migrate/*",
    // `bundle install --path vendor/bundle` puts every gem's source in the repository.
    "*/.bundle/*",
    // Build output: Maven and Gradle write here, and so does cargo.
    "*/target/*",
    "target/*",
    // Composer installs every dependency's source here.
    "*/vendor/composer/*",
    // A Blade file is a template: mostly markup, with the logic expressed in
    // directives this tool does not parse. `vue.rs` makes the same call for a
    // `<template>` block -- a line it cannot parse is a line it cannot explain.
    "*.blade.php",
    // Annotation processors, protoc and friends write into these.
    "*/generated/*",
    "generated/*",
    "*/.mypy_cache/*",
    "*/.pytest_cache/*",
    // Vendored shadcn/ui primitives. These are generated third-party components, and
    // their near-identical thin wrappers are that library's intended design, not
    // duplication anyone should "fix". Both layouts seen in the wild are listed, and
    // `detect_shadcn_ui` also reads the authoritative path out of `components.json`
    // for projects that put it somewhere else again.
    "*/components/ui/*",
    "components/ui/*",
    "*/shared/ui/*",
    "shared/ui/*",
    // `reference/` holds source material and one-off exploration scripts, not shipped
    // code. Across the Artifer repos it is overwhelmingly PDFs, spreadsheets and
    // exported decks; where it does contain code it is scratch work. Leaving it in let
    // a single 1,717-line reference script define a whole library's score.
    "reference/*",
    "*/reference/*",
];

/// Validated against real test suites: every one of these fires on idiomatic,
/// well-written test code, so charging for them would only teach bad habits.
pub const TEST_EXEMPT_RULES: &[&str] = &[
    LOW_COHESION,
    BRAIN_CLASS,
    FUNCTION_COUNT,
    PRIMITIVE_OBSESSION,
    STRING_HEAVY_ARGUMENTS,
    // Test code repeats its arrange/act shape by design, and `Duplicated Assertion
    // Blocks` already asks the sharper version of this question about a suite.
    DUPLICATED_BLOCK,
];

impl<S: Into<String>> FromIterator<S> for PatternSet {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self(iter.into_iter().map(Into::into).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_lists_every_pattern_in_order() {
        let set = PatternSet::new(["*/b/*", "*.a", "c_*"]);
        assert_eq!(set.sorted(), ["*.a", "*/b/*", "c_*"]);
    }
}
