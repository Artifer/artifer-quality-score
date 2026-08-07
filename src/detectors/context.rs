//! The per-file measurement context every detector reads from.
//!
//! Each function is measured once, here, and the results shared. A detector reports a
//! measurement and the threshold it crossed; none of them knows anything about points.

use super::*;

/// A detection, before it has a price.
pub struct RawFinding {
    pub category: &'static str,
    pub details: String,
    pub range: Range,
    pub value: f64,
    pub threshold: f64,
    pub function: Option<String>,
}

/// A named function and its measurements, computed once and shared.
pub struct FunctionContext<'tree> {
    pub node: Node<'tree>,
    pub name: String,
    pub range: Range,
    pub loc: usize,
    pub complexity: usize,
    pub nesting: usize,
    pub parameters: Vec<Node<'tree>>,
    pub is_constructor: bool,
}

pub struct FileContext<'tree, 'src> {
    pub path: String,
    pub spec: &'static LanguageSpec,
    pub profile: &'src Profile,
    pub root: Node<'tree>,
    pub source: &'src [u8],
    pub is_test: bool,
    pub loc: usize,
    pub functions: Vec<FunctionContext<'tree>>,
}

impl<'tree, 'src> FileContext<'tree, 'src> {
    pub fn new(
        path: String,
        spec: &'static LanguageSpec,
        profile: &'src Profile,
        root: Node<'tree>,
        source: &'src [u8],
    ) -> Self {
        let mut functions = Vec::new();
        for node in metrics::iter_functions(&root, spec, source) {
            let name = metrics::function_name(&node, source)
                .unwrap_or_else(|| "<anonymous>".to_string());
            let is_constructor = spec.constructor_names.contains(name.as_str());
            functions.push(FunctionContext {
                node,
                range: metrics::to_range(&node),
                loc: metrics::logical_lines(&node),
                complexity: metrics::cyclomatic_complexity(&node, spec, source),
                nesting: metrics::nesting_depth(&node, spec, source),
                parameters: metrics::parameters(&node, spec, source),
                is_constructor,
                name,
            });
        }
        Self {
            is_test: profile.test_patterns.matches(&path),
            loc: metrics::logical_lines(&root),
            path,
            spec,
            profile,
            root,
            source,
            functions,
        }
    }

    pub(crate) fn threshold(&self, key: &str) -> f64 {
        self.profile.threshold(key)
    }

    /// A size limit, relaxed for test files.
    pub(crate) fn size_threshold(&self, key: &str) -> f64 {
        let value = self.profile.threshold(key);
        if self.is_test {
            value * self.profile.threshold("test_file_size_multiplier")
        } else {
            value
        }
    }
}

/// Just the declaration line — where a reader wants the marker.
pub(crate) fn signature_range(function: &FunctionContext) -> Range {
    Range {
        start_line: function.range.start_line,
        start_column: function.range.start_column,
        end_line: function.range.start_line,
        end_column: function.range.end_column,
    }
}

/// Format a ratio the way Python's `{:.0%}` does.
pub(crate) fn percent(ratio: f64) -> String {
    format!("{:.0}%", ratio * 100.0)
}

// --------------------------------------------------------------------------- //
// Function level
// --------------------------------------------------------------------------- //

