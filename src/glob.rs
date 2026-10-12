//! `fnmatch`-compatible glob matching.
//!
//! Deliberately reimplements Python's `fnmatch` rather than using a path-aware
//! glob crate. The difference matters: in `fnmatch`, `*` matches `/` as well, so
//! `*/tests/*` matches `a/b/tests/c.py`. A path-aware matcher would not, and every
//! exclude pattern in the shipped profile would silently stop working.
//!
//! Matching is case-sensitive, matching `fnmatch` on POSIX (where `os.path.normcase`
//! is the identity function).

use regex::Regex;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

/// Translate an `fnmatch` pattern into an anchored regex, following the same rules
/// as Python's `fnmatch.translate`.
/// Translate a `[...]` character class, returning the regex text and the index just
/// past the closing bracket. An unterminated `[` is a literal, as in `fnmatch`.
fn translate_class(chars: &[char], open_at: usize) -> (String, usize) {
    let mut close = open_at;
    let leads_class = chars.get(close).is_some_and(|c| *c == '!' || *c == ']');
    if leads_class {
        close += 1;
    }
    while close < chars.len() && chars[close] != ']' {
        close += 1;
    }
    if close >= chars.len() {
        return ("\\[".to_string(), open_at);
    }
    let mut class: String = chars[open_at..close].iter().collect();
    if class.starts_with('!') {
        class.replace_range(0..1, "^");
    }
    (format!("[{}]", class.replace('\\', "\\\\")), close + 1)
}

/// Translate an `fnmatch` pattern into an anchored regex, following the same rules as
/// Python's `fnmatch.translate`.
fn translate(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() * 2);
    // Anchored at both ends. Python's `fnmatch.translate` emits only the trailing `\z`
    // because `fnmatch` applies it with `re.match`, which anchors the start implicitly.
    // Rust's `is_match` searches anywhere, so the `^` must be explicit — without it
    // `test_*.py` matches `x_test_a.py`.
    out.push_str("^(?s:");
    let mut index = 0;

    while index < chars.len() {
        let current = chars[index];
        index += 1;
        match current {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '[' => {
                let (class, next) = translate_class(&chars, index);
                out.push_str(&class);
                index = next;
            }
            other => out.push_str(&regex::escape(&other.to_string())),
        }
    }
    out.push_str(")\\z");
    out
}

fn cache() -> &'static Mutex<HashMap<String, Regex>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Whether `name` matches `pattern`, with `fnmatch` semantics.
pub fn fnmatch(name: &str, pattern: &str) -> bool {
    // A panic elsewhere cannot corrupt a map of compiled patterns, so a poisoned lock is
    // still good to use.
    let mut guard = cache().lock().unwrap_or_else(PoisonError::into_inner);
    let regex = guard
        .entry(pattern.to_string())
        .or_insert_with(|| Regex::new(&translate(pattern)).expect("translated glob is valid"));
    regex.is_match(name)
}

/// Equivalent of Python's `str.strip("*/")`: remove leading and trailing `*` and `/`.
pub fn strip_glob_chars(pattern: &str) -> &str {
    pattern.trim_matches(|c| c == '*' || c == '/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_crosses_path_separators_like_fnmatch() {
        assert!(fnmatch("a/b/tests/c.py", "*/tests/*"));
        assert!(fnmatch("src/components/ui/button.tsx", "*/components/ui/*"));
    }

    #[test]
    fn character_classes_follow_fnmatch() {
        assert!(fnmatch("a1.py", "a[0-9].py"));
        assert!(!fnmatch("ab.py", "a[0-9].py"));
        assert!(fnmatch("ab.py", "a[!0-9].py"));
        assert!(!fnmatch("a1.py", "a[!0-9].py"));
        // A leading `]` is a member of the class, not its end.
        assert!(fnmatch("a].py", "a[]x].py"));
        // An unterminated `[` is a literal bracket.
        assert!(fnmatch("a[b", "a[b"));
        assert!(!fnmatch("ab", "a[b"));
    }

    #[test]
    fn anchors_the_whole_name() {
        assert!(fnmatch("test_a.py", "test_*.py"));
        assert!(!fnmatch("x_test_a.py", "test_*.py"));
    }

    #[test]
    fn question_mark_matches_one_character() {
        assert!(fnmatch("a.py", "?.py"));
        assert!(!fnmatch("ab.py", "?.py"));
    }

    #[test]
    fn is_case_sensitive() {
        assert!(!fnmatch("TEST_A.PY", "test_*.py"));
    }

    #[test]
    fn strips_glob_characters() {
        assert_eq!(strip_glob_chars("*/node_modules/*"), "node_modules");
        assert_eq!(strip_glob_chars("*.min.js"), ".min.js");
    }
}
