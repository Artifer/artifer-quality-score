//! Deciding what a function is called, and whether it is a unit of its own.
//!
//! **Named nested functions are separate units; anonymous ones are not.** A named
//! helper (`def helper()`, `const load = () => {}`, `let f = |x| ..`) is measured on
//! its own. An anonymous inline callback (`items.map(x => ...)`) is measured as part of
//! the enclosing function, because that is where its complexity lands when you read it.

use crate::languages::LanguageSpec;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

/// Parent node types from which an otherwise-anonymous function takes its name.
pub(crate) const NAMING_PARENTS: &[&str] = &[
    "variable_declarator",
    "assignment",
    "pair",
    "public_field_definition",
    "field_definition",
    // Rust binds a closure with `let name = |..| ..`, naming it via `pattern`.
    "let_declaration",
    // Go: `helper := func() {..}`, `var helper = func() {..}`.
    "short_var_declaration",
    "var_spec",
    "assignment_statement",
];

/// The function's name, or None if it is a genuinely anonymous callback.
pub fn function_name(node: &Node, source: &[u8]) -> Option<String> {
    if let Some(named) = node.child_by_field_name("name") {
        return Some(text(&named, source).to_string());
    }

    // `constructor(...)`, `get foo()` and friends need no case of their own: the pinned
    // grammars give a method_definition its property identifier as the `name` field.
    binding_name(node, source)
}

/// `const load = async () => {}` and `let f = |x| ..` are named units even though the
/// closure itself is anonymous; the name comes from what it is bound to.
pub(crate) fn binding_name(node: &Node, source: &[u8]) -> Option<String> {
    let mut parent = node.parent()?;
    // Go puts both sides of `helper := func() {..}` in an `expression_list`, so the
    // binding that names the closure is one level further up.
    if parent.kind() == "expression_list" {
        parent = parent.parent()?;
    }
    if !NAMING_PARENTS.contains(&parent.kind()) {
        return None;
    }
    ["name", "left", "key", "pattern"]
        .into_iter()
        .find_map(|field| parent.child_by_field_name(field))
        .map(|named| text(&named, source).to_string())
}

/// The identifying name of a class-like node.
///
/// Rust `impl` blocks have no `name`; their identity is the `type` they implement.
pub fn class_name(node: &Node, source: &[u8]) -> Option<String> {
    if let Some(name) = function_name(node, source) {
        return Some(name);
    }
    node.child_by_field_name("type")
        .map(|t| text(&t, source).to_string())
}

/// Whether `node` is a function measured as its own unit.
pub fn is_scope_boundary(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    is_kind(node, &spec.function_nodes) && function_name(node, source).is_some()
}

/// Every named function in the tree, outermost first.
pub fn iter_functions<'tree>(
    root: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
) -> Vec<Node<'tree>> {
    let mut found = Vec::new();
    collect_functions(root, spec, source, &mut found);
    found
}

pub(crate) fn collect_functions<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
    found: &mut Vec<Node<'tree>>,
) {
    for child in children(node) {
        if is_scope_boundary(&child, spec, source) {
            found.push(child);
        }
        collect_functions(&child, spec, source, found);
    }
}
