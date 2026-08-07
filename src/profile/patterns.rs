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

    pub fn as_slice(&self) -> &[String] {
        &self.0
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

    pub fn iter(&self) -> std::slice::Iter<'_, String> {
        self.0.iter()
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
];

pub const EXCLUDE_PATTERNS: &[&str] = &[
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
];

impl<S: Into<String>> FromIterator<S> for PatternSet {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self(iter.into_iter().map(Into::into).collect())
    }
}
