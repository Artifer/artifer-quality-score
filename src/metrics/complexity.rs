//! Branching, nesting, and the shape of control flow.

use crate::languages::LanguageSpec;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

pub(crate) fn is_boolean_operator(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    if !spec.boolean_op_nodes.contains(node.kind()) {
        return false;
    }
    match node.child_by_field_name("operator") {
        // Python models `and`/`or` as distinct node types already.
        None => true,
        Some(operator) => spec.boolean_operators.contains(text(&operator, source)),
    }
}

/// McCabe complexity: one, plus one per decision point.
pub fn cyclomatic_complexity(node: &Node, spec: &LanguageSpec, source: &[u8]) -> usize {
    let mut complexity = 1;
    for current in own_nodes(node, spec, source) {
        if current.id() == node.id() {
            continue;
        }
        if spec.decision_nodes.contains(current.kind())
            || is_boolean_operator(&current, spec, source)
        {
            complexity += 1;
        }
    }
    complexity
}

/// True for the `if` of an `else if`, which reads flat, not nested.
pub(crate) fn continues_branch(node: &Node, spec: &LanguageSpec) -> bool {
    node.parent()
        .map(|parent| spec.continuation_nodes.contains(parent.kind()))
        .unwrap_or(false)
}

/// Deepest run of nested control-flow constructs inside `node`.
pub fn nesting_depth(node: &Node, spec: &LanguageSpec, source: &[u8]) -> usize {
    fn visit(current: &Node, depth: usize, spec: &LanguageSpec, source: &[u8]) -> usize {
        let mut deepest = depth;
        for child in children(current) {
            if is_scope_boundary(&child, spec, source) {
                continue;
            }
            let mut child_depth = depth;
            if spec.nesting_nodes.contains(child.kind()) && !continues_branch(&child, spec) {
                child_depth = depth + 1;
            }
            deepest = deepest.max(visit(&child, child_depth, spec, source));
        }
        deepest
    }
    visit(node, 0, spec, source)
}

pub fn function_body<'tree>(node: &Node<'tree>, _spec: &LanguageSpec) -> Option<Node<'tree>> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }
    named_children(node)
        .into_iter()
        .rev()
        .find(|child| matches!(child.kind(), "block" | "statement_block"))
}

/// A self-contained chunk of nested logic inside a function.
pub struct Bump {
    pub depth: usize,
}

/// Top-level statements of `node` that each hide their own nested logic.
///
/// A function with several of these is a "bumpy road": instead of one coherent level
/// of abstraction it strings together multiple lumps of nested detail, each of which
/// wants to be its own function.
pub fn bumps(node: &Node, spec: &LanguageSpec, source: &[u8], min_depth: usize) -> Vec<Bump> {
    let Some(body) = function_body(node, spec) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for statement in named_children(&body) {
        if is_comment(&statement) {
            continue;
        }
        let mut depth = nesting_depth(&statement, spec, source);
        if spec.nesting_nodes.contains(statement.kind()) {
            depth += 1;
        }
        if depth >= min_depth {
            found.push(Bump { depth });
        }
    }
    found
}

pub(crate) fn condition_nodes<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
) -> Vec<Node<'tree>> {
    let mut conditions = Vec::new();
    for current in own_nodes(node, spec, source) {
        if !spec.decision_nodes.contains(current.kind()) {
            continue;
        }
        if let Some(condition) = current.child_by_field_name("condition") {
            conditions.push(condition);
        }
    }
    conditions
}

pub fn count_boolean_operators(node: &Node, spec: &LanguageSpec, source: &[u8]) -> usize {
    own_nodes(node, spec, source)
        .iter()
        .filter(|current| is_boolean_operator(current, spec, source))
        .count()
}

/// Branch conditions combining at least `min_operators` boolean operators.
pub fn complex_conditionals<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
    min_operators: usize,
) -> Vec<(Node<'tree>, usize)> {
    let mut found = Vec::new();
    for condition in condition_nodes(node, spec, source) {
        let operators = count_boolean_operators(&condition, spec, source);
        if operators >= min_operators {
            found.push((condition, operators));
        }
    }
    found
}

