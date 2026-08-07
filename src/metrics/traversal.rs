//! Walking a syntax tree, and reading text out of it.
//!
//! Two walks, and the difference is load-bearing. `own_nodes` stops at a nested named
//! function, so per-function metrics never absorb a helper's complexity. `all_nodes`
//! descends everywhere, for rules that ask "anywhere in this file".

use crate::languages::LanguageSpec;
use crate::models::Range;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

pub const COMMENT_NODES: &[&str] = &["comment", "block_comment", "line_comment"];

pub(crate) fn is_comment(node: &Node) -> bool {
    COMMENT_NODES.contains(&node.kind())
}

pub fn text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

pub fn to_range(node: &Node) -> Range {
    Range {
        start_line: node.start_position().row + 1,
        start_column: node.start_position().column + 1,
        end_line: node.end_position().row + 1,
        end_column: node.end_position().column + 1,
    }
}

/// All children, including anonymous ones, in document order.
pub(crate) fn children<'tree>(node: &Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// Named children only, in document order.
pub fn named_children<'tree>(node: &Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// Walk `root`, not descending into nested named functions.
///
/// `root` itself is included even when it is a function, so this reads as "the nodes
/// belonging to this unit".
pub fn own_nodes<'tree>(
    root: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
) -> Vec<Node<'tree>> {
    let mut out = vec![*root];
    let mut stack: Vec<Node<'tree>> = children(root).into_iter().rev().collect();
    while let Some(node) = stack.pop() {
        if is_scope_boundary(&node, spec, source) {
            continue;
        }
        out.push(node);
        stack.extend(children(&node).into_iter().rev());
    }
    out
}

/// Walk the whole subtree, including nested named functions.
///
/// Use this for rules that ask "anywhere in this file" (classes, assertions,
/// declarations). Use `own_nodes` for per-function metrics, which must not absorb a
/// nested helper's complexity.
pub fn all_nodes<'tree>(root: &Node<'tree>) -> Vec<Node<'tree>> {
    let mut out = Vec::new();
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        out.push(node);
        stack.extend(children(&node).into_iter().rev());
    }
    out
}

