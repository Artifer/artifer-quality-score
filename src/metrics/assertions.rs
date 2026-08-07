//! Recognising assertions, in all three dialects: Python's `assert` statement,
//! JS/TS `expect(..)` calls, and Rust's `assert!` macro family.

use crate::languages::LanguageSpec;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

/// Whether a statement asserts something, in either assertion dialect.
pub fn is_assertion(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    if spec.assertion_nodes.contains(node.kind()) {
        return true;
    }
    if node.kind() != "expression_statement" || node.named_child_count() == 0 {
        return false;
    }
    let expression = node.named_child(0).expect("named_child_count checked");
    // `macro_invocation` covers Rust's assert!/assert_eq!/debug_assert! family.
    if !matches!(
        expression.kind(),
        "call_expression" | "call" | "await_expression" | "macro_invocation"
    ) {
        return false;
    }
    let raw = text(&expression, source);
    let stripped = raw.strip_prefix("await ").unwrap_or(raw).trim_start();
    spec.assertion_call_prefixes.iter().any(|prefix| stripped.starts_with(prefix))
}

