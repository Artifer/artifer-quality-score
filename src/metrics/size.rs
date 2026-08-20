//! Counting what a reader actually has to read.
//!
//! **Comments and docstrings never count as LoC.** Otherwise the fastest way to improve
//! a score is to delete the documentation, and a metric an LLM optimises against must
//! not reward that.

use crate::languages::LanguageSpec;
use std::collections::HashSet;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

/// A bare string statement, i.e. a Python docstring or a JS directive.
pub fn is_docstring(node: &Node) -> bool {
    if node.kind() != "expression_statement" || node.named_child_count() != 1 {
        return false;
    }
    let inner = node.named_child(0).expect("named_child_count checked");
    matches!(inner.kind(), "string" | "template_string")
}

/// Non-blank, non-comment, non-docstring lines spanned by `node`.
/// Whether a node is something other than code: a comment, a docstring, or the markup
/// a `.php` file wraps its code islands in. None of it is a line this tool could explain
/// a finding about, so none of it is counted as one.
fn is_prose(node: &Node) -> bool {
    is_comment(node) || is_docstring(node) || is_markup(node)
}

pub fn logical_lines(node: &Node) -> usize {
    let mut lines: HashSet<usize> = HashSet::new();
    let mut stack = vec![*node];
    while let Some(current) = stack.pop() {
        if is_prose(&current) {
            continue;
        }
        if current.child_count() == 0 {
            for line in current.start_position().row..=current.end_position().row {
                lines.insert(line);
            }
            continue;
        }
        stack.extend(children(&current));
    }
    lines.len()
}

pub(crate) const LITERAL_NODES: &[&str] = &[
    "string",
    "string_fragment",
    "integer",
    "float",
    "number",
    "true",
    "false",
    "none",
    "null",
];

pub(crate) const IDENTIFIER_NODES: &[&str] = &["identifier", "property_identifier", "type_identifier"];

/// Structure-only token sequence, for similarity comparison.
///
/// Identifiers and literals collapse to their node type, so two blocks that differ
/// only in names and constants compare as identical. Comments are dropped.
pub fn normalised_tokens(node: &Node, spec: &LanguageSpec, source: &[u8]) -> Vec<&'static str> {
    let mut tokens = Vec::new();
    for current in own_nodes(node, spec, source) {
        if current.child_count() != 0 || is_comment(&current) {
            continue;
        }
        let kind = current.kind();
        if IDENTIFIER_NODES.contains(&kind) {
            tokens.push("<id>");
        } else if LITERAL_NODES.contains(&kind) {
            tokens.push("<lit>");
        } else {
            tokens.push(kind);
        }
    }
    tokens
}

