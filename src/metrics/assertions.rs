//! Recognising assertions: Python's `assert` statement, JS/TS `expect(..)` calls,
//! Rust's `assert!` macro family, and the `assert_*` / `expect(..).to` calls that Go
//! and Ruby suites are written with.

use crate::languages::LanguageSpec;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

/// Whether a statement asserts something, in either assertion dialect.
pub fn is_assertion(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    if is_kind(node, &spec.assertion_nodes) {
        return true;
    }
    // Most grammars wrap a statement-position call in an `expression_statement`; Ruby
    // has no such node, so there the call *is* the statement.
    let expression = if node.kind() == "expression_statement" {
        match node.named_child(0) {
            Some(child) => child,
            None => return false,
        }
    } else {
        *node
    };
    if !is_call(&expression) {
        return false;
    }
    let raw = text(&expression, source);
    let stripped = raw.strip_prefix("await ").unwrap_or(raw).trim_start();
    spec.assertion_call_prefixes
        .iter()
        .any(|prefix| stripped.starts_with(prefix))
}
