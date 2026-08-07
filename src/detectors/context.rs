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
    /// A malformed directive in this file, if any. The file is still scored as though the
    /// directive were absent, so a broken suppression can never lower a score.
    pub directive_error: Option<String>,
    /// Directives applying to the whole file.
    pub file_directives: Vec<Directive>,
    /// Directives resolved to the function they precede, by index into `functions`.
    pub function_directives: HashMap<usize, Vec<Directive>>,
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
        // Parsed here rather than passed in: the context already has the path, tree and
        // source, so taking them as a sixth argument only widened the signature.
        let (directives, directive_error) = match parse_directives(&root, source, &path) {
            Ok(directives) => (directives, None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        };
        let (file_directives, function_directives) =
            resolve_directives(directives, &functions);
        Self {
            file_directives,
            function_directives,
            directive_error,
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


/// Attach each directive to what it governs.
///
/// A function-scoped directive applies to the next function that starts after it, which
/// is what makes `@aqs(...)` read as an annotation on the declaration below.
fn resolve_directives(
    directives: Vec<Directive>,
    functions: &[FunctionContext],
) -> (Vec<Directive>, HashMap<usize, Vec<Directive>>) {
    let mut file_level = Vec::new();
    let mut per_function: HashMap<usize, Vec<Directive>> = HashMap::new();

    for directive in directives {
        if directive.file_scoped {
            file_level.push(directive);
            continue;
        }
        let target = functions
            .iter()
            .enumerate()
            .filter(|(_, f)| f.range.start_line > directive.line)
            .min_by_key(|(_, f)| f.range.start_line);
        if let Some((index, _)) = target {
            per_function.entry(index).or_default().push(directive);
        }
        // A directive with no function after it governs nothing; `detect` reports it as
        // unused rather than failing, since a stale one is a cleanup task, not an error.
    }
    (file_level, per_function)
}

impl FileContext<'_, '_> {
    /// The directive suppressing `category`, if any.
    ///
    /// A function-scoped directive wins over a file-scoped one, which is the order a
    /// reader expects: the annotation nearest the code governs it.
    pub fn suppression(&self, function: Option<usize>, category: &str) -> Option<&Directive> {
        let on_function = function
            .and_then(|index| self.function_directives.get(&index))
            .and_then(|directives| directives.iter().find(|d| d.covers(category)));
        on_function.or_else(|| self.file_directives.iter().find(|d| d.covers(category)))
    }
}

/// Collect `@aqs` directives from the file's comments.
fn parse_directives(
    root: &Node,
    source: &[u8],
    path: &str,
) -> anyhow::Result<Vec<Directive>> {
    let comments: Vec<(String, usize)> = metrics::all_nodes(root)
        .iter()
        .filter(|node| metrics::COMMENT_NODES.contains(&node.kind()))
        .map(|node| {
            (
                metrics::text(node, source).to_string(),
                node.start_position().row + 1,
            )
        })
        .collect();
    crate::directives::parse_all(&comments, path)
}
