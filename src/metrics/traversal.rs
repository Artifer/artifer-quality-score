//! Walking a syntax tree, and reading text out of it.
//!
//! Two walks, and the difference is load-bearing. `own_nodes` stops at a nested named
//! function, so per-function metrics never absorb a helper's complexity. `all_nodes`
//! descends everywhere, for rules that ask "anywhere in this file".

use crate::languages::LanguageSpec;
use crate::models::Range;
use std::collections::HashSet;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

pub const COMMENT_NODES: &[&str] = &["comment", "block_comment", "line_comment"];

/// Node kinds that invoke something, across every grammar here.
///
/// Shared rather than per-spec because the rules that ask "does this call anything?"
/// -- pure accessors, assertions -- ask it of any language, and a grammar the list has
/// not heard of fails quietly: a Java `method_invocation` missing from it made every
/// one-line method that calls something look like a getter.
pub const CALL_NODES: &[&str] = &[
    "call",                       // Python, Ruby, Go
    "call_expression",            // TS / JS, Rust
    "method_invocation",          // Java
    "object_creation_expression", // Java, PHP
    "function_call_expression",   // PHP
    "member_call_expression",     // PHP
    "scoped_call_expression",     // PHP
    "macro_invocation",           // Rust
    "await_expression",
    "await",
];

pub(crate) fn is_call(node: &Node) -> bool {
    CALL_NODES.contains(&node.kind())
}

/// Node kinds that read a member off something, across every grammar here.
pub const FIELD_ACCESS_NODES: &[&str] = &[
    "attribute",                // Python
    "member_expression",        // TS / JS
    "field_expression",         // Rust
    "selector_expression",      // Go
    "member_access_expression", // PHP
    "member_call_expression",   // PHP
];

/// What a language calls the object a method is running on.
pub const SELF_RECEIVERS: &[&str] = &["self", "this", "$this"];

/// Markup a code grammar carries but does not describe: the HTML around a `<?php`
/// island. A line this tool cannot parse is a line it cannot explain a finding about,
/// so it is not counted as code -- the same call `vue.rs` makes for a `<template>`.
pub const MARKUP_NODES: &[&str] = &["text"];

pub(crate) fn is_markup(node: &Node) -> bool {
    MARKUP_NODES.contains(&node.kind())
}

pub(crate) fn is_comment(node: &Node) -> bool {
    COMMENT_NODES.contains(&node.kind())
}

/// Whether a node is one of the kinds a spec names.
///
/// The `is_named` half is load-bearing. Some grammars give an anonymous keyword token
/// the same kind string as the node it opens -- a tree-sitter-ruby `if` node *contains*
/// an `if` token -- so a bare `kinds.contains(node.kind())` counts every Ruby branch
/// twice and reads every block as twice as deeply nested. Every match of a node against
/// a spec's vocabulary goes through here for that reason.
pub fn is_kind(node: &Node, kinds: &HashSet<&'static str>) -> bool {
    node.is_named() && kinds.contains(node.kind())
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
