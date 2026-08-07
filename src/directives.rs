//! Source-comment overrides.
//!
//! Sometimes the metric is wrong about a specific piece of code and the honest answer is
//! to say so *at the site*, not to bend the code or to disable a rule repository-wide:
//!
//! ```text
//! // @aqs(disable:"Complex Conditional") the bounds check reads better inline
//! fn extend(&self, found: Match, window: Window) -> Match {
//! ```
//!
//! The shape follows CodeScene's published code comment directives, which are familiar
//! and have been through real use. Three deliberate differences:
//!
//! **A reason is required.** An unexplained suppression is invisible debt — the same
//! failure mode as a config typo that silently disables a rule, which this tool treats as
//! fatal. Explaining yourself costs five words.
//!
//! **An unknown rule name is an error.** A directive that quietly does nothing is worse
//! than one that refuses to load, so a misspelled category fails the scan and names the
//! file and line.
//!
//! **File-level rules can be suppressed too**, via `@aqs-file(...)`, because a generated
//! or vendored file has no function to hang a directive on.
//!
//! Suppressions are never silent: every report counts them, and `--json` lists each one
//! with its reason so a reviewer can audit what a branch chose to ignore.

use crate::profile::RULE_DEFAULTS;
use anyhow::{bail, Result};

/// The marker introducing a function-scoped directive.
const FUNCTION_MARKER: &str = "@aqs(";
/// The marker introducing a file-scoped directive.
const FILE_MARKER: &str = "@aqs-file(";

/// What one directive turns off, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Directive {
    /// Categories named by this directive. Empty when `all` is set.
    pub categories: Vec<String>,
    /// Whether this is `disable-all`.
    pub all: bool,
    /// The author's justification, which is mandatory.
    pub reason: String,
    /// 1-indexed line the directive appears on.
    pub line: usize,
    /// Whether it applies to the whole file rather than the next function.
    pub file_scoped: bool,
}

impl Directive {
    pub fn covers(&self, category: &str) -> bool {
        self.all || self.categories.iter().any(|c| c == category)
    }
}

fn known_category(name: &str) -> bool {
    RULE_DEFAULTS.iter().any(|(rule, _)| *rule == name)
}

/// Extract the `disable:"..."` names and the `disable-all` flag from a directive body.
fn parse_body(body: &str) -> Result<(Vec<String>, bool)> {
    let mut categories = Vec::new();
    let mut all = false;

    for clause in body.split(',') {
        let clause = clause.trim();
        if clause == "disable-all" {
            all = true;
            continue;
        }
        let Some(rest) = clause.strip_prefix("disable:") else {
            bail!(
                "unrecognised directive clause {clause:?}. \
                 Expected `disable:\"Rule Name\"` or `disable-all`."
            );
        };
        let name = rest.trim().trim_matches('"');
        if name.is_empty() {
            bail!("`disable:` needs a rule name in double quotes");
        }
        if !known_category(name) {
            bail!("unknown rule {name:?}. Run `aqs explain --list` for valid names.");
        }
        categories.push(name.to_string());
    }

    if categories.is_empty() && !all {
        bail!("a directive must disable at least one rule");
    }
    Ok((categories, all))
}

/// Strip a line's leading comment punctuation and whitespace.
fn strip_comment_prefix(line: &str) -> &str {
    line.trim_start()
        .trim_start_matches(['/', '#', '*', '!', '-'])
        .trim_start()
}

/// Parse one comment node, which may span several lines.
///
/// A directive must be the **first** thing on its line, after the comment punctuation.
/// Without that rule, prose describing the feature — `/// Collect `@aqs(...)`
/// directives` — parses as a directive, which is exactly what happened the first time
/// this shipped. Per-line matching still allows a directive inside a block comment.
///
/// `line` is the comment's own first line; the returned directive records the line the
/// marker was actually found on, so errors are locatable.
fn parse_comment(text: &str, line: usize, path: &str) -> Result<Option<Directive>> {
    for (offset, raw) in text.lines().enumerate() {
        let content = strip_comment_prefix(raw);
        let (body_at, file_scoped) = if let Some(rest) = content.strip_prefix(FILE_MARKER) {
            (rest, true)
        } else if let Some(rest) = content.strip_prefix(FUNCTION_MARKER) {
            (rest, false)
        } else {
            continue;
        };

        let at = line + offset;
        let Some(close) = body_at.find(')') else {
            bail!("{path}:{at}: unterminated directive, expected a closing `)`");
        };

        let locate = |error: anyhow::Error| anyhow::anyhow!("{path}:{at}: {error}");
        let (categories, all) = parse_body(&body_at[..close]).map_err(locate)?;

        let reason = body_at[close + 1..]
            .trim()
            .trim_end_matches("*/")
            .trim()
            .to_string();
        if reason.is_empty() {
            bail!(
                "{path}:{at}: a directive must say why. Write the reason after the \
                 closing `)`, for example:\n  \
                 @aqs(disable:\"Complex Method\") dispatch table, splitting it would obscure it"
            );
        }

        return Ok(Some(Directive {
            categories,
            all,
            reason,
            line: at,
            file_scoped,
        }));
    }
    Ok(None)
}

/// Every directive in a file, in source order.
pub fn parse_all(comments: &[(String, usize)], path: &str) -> Result<Vec<Directive>> {
    let mut found = Vec::new();
    for (text, line) in comments {
        if let Some(directive) = parse_comment(text, *line, path)? {
            found.push(directive);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Option<Directive>> {
        parse_comment(text, 7, "a.rs")
    }

    #[test]
    fn an_ordinary_comment_is_not_a_directive() {
        assert_eq!(parse("// just explaining something").unwrap(), None);
        assert_eq!(parse("# see @aqs docs").unwrap(), None);
    }

    #[test]
    fn prose_mentioning_the_marker_is_not_a_directive() {
        // Documentation describing the feature must not suppress anything. This is not
        // hypothetical: the first version parsed its own doc comments as directives.
        for text in [
            "/// Collect `@aqs(...)` directives from the file's comments.",
            "//! Sometimes a rule is wrong; use @aqs(disable:\"X\") to say so.",
            "// see @aqs-file(disable:\"Lines of Code in a Single File\") in the README",
        ] {
            assert_eq!(parse(text).unwrap(), None, "should be prose: {text}");
        }
    }

    #[test]
    fn a_directive_inside_a_block_comment_is_found() {
        let directive = parse("/*\n * @aqs(disable:\"Complex Method\") generated code\n */")
            .unwrap()
            .expect("a directive");
        assert_eq!(directive.categories, vec!["Complex Method"]);
        assert_eq!(directive.reason, "generated code");
        // The reported line is where the marker is, not where the comment starts.
        assert_eq!(directive.line, 8);
    }

    #[test]
    fn parses_one_rule_with_a_reason() {
        let directive = parse("// @aqs(disable:\"Complex Method\") dispatch table")
            .unwrap()
            .expect("a directive");
        assert_eq!(directive.categories, vec!["Complex Method"]);
        assert!(!directive.all);
        assert_eq!(directive.reason, "dispatch table");
        assert!(!directive.file_scoped);
        assert!(directive.covers("Complex Method"));
        assert!(!directive.covers("Large Method"));
    }

    #[test]
    fn parses_several_rules_and_any_comment_style() {
        for text in [
            "// @aqs(disable:\"Complex Method\", disable:\"Large Method\") generated",
            "# @aqs(disable:\"Complex Method\", disable:\"Large Method\") generated",
            "/* @aqs(disable:\"Complex Method\", disable:\"Large Method\") generated */",
        ] {
            let directive = parse(text).unwrap().expect("a directive");
            assert_eq!(directive.categories, vec!["Complex Method", "Large Method"], "{text}");
        }
    }

    #[test]
    fn disable_all_covers_everything() {
        let directive = parse("// @aqs(disable-all) rewritten next sprint")
            .unwrap()
            .expect("a directive");
        assert!(directive.all);
        assert!(directive.covers("Brain Method"));
        assert!(directive.covers("Low Cohesion"));
    }

    #[test]
    fn file_scope_is_distinct() {
        let directive = parse("// @aqs-file(disable:\"Lines of Code in a Single File\") generated")
            .unwrap()
            .expect("a directive");
        assert!(directive.file_scoped);
        assert_eq!(directive.categories, vec!["Lines of Code in a Single File"]);
    }

    #[test]
    fn a_missing_reason_is_an_error() {
        // An unexplained suppression is invisible debt.
        let error = parse("// @aqs(disable:\"Complex Method\")").unwrap_err();
        assert!(format!("{error}").contains("must say why"), "{error}");
    }

    #[test]
    fn an_unknown_rule_name_is_an_error() {
        // A directive that quietly does nothing is worse than one that fails loudly.
        let error = parse("// @aqs(disable:\"Complex Methd\") typo").unwrap_err();
        assert!(format!("{error}").contains("unknown rule"), "{error}");
        assert!(format!("{error}").contains("a.rs:7"), "should locate it: {error}");
    }

    #[test]
    fn malformed_directives_are_errors() {
        for text in [
            "// @aqs(disable:\"Complex Method\" no closing paren",
            "// @aqs(silence:\"Complex Method\") wrong verb",
            "// @aqs() nothing named",
            "// @aqs(disable:\"\") empty name",
        ] {
            assert!(parse(text).is_err(), "should be rejected: {text}");
        }
    }
}
