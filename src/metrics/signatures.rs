//! Parameters and the types they carry.
//!
//! Only annotated parameters count towards the type rules. An unannotated parameter is
//! absence of evidence, not evidence of bad design — otherwise the score would measure
//! how thoroughly the code is typed rather than how well it is designed.

use crate::languages::LanguageSpec;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

/// The node holding a callable's parameters.
///
/// By position for most languages, but by field where position is ambiguous: a Go
/// `method_declaration` has three `parameter_list` children -- receiver, parameters,
/// results -- and the first one is the receiver.
fn param_container<'tree>(node: &Node<'tree>, spec: &LanguageSpec) -> Option<Node<'tree>> {
    if let Some(field) = spec.param_field {
        return node.child_by_field_name(field);
    }
    named_children(node)
        .into_iter()
        .find(|child| spec.param_containers.contains(child.kind()))
}

/// How many parameters one declaration declares.
///
/// Normally one. Go lets several share a type -- `func f(a, b, c int)` is one
/// `parameter_declaration` naming three parameters -- and a caller still has to pass
/// three arguments, so that is what Long Parameter List has to count.
fn declared_count(param: &Node) -> usize {
    let mut cursor = param.walk();
    param
        .children_by_field_name("name", &mut cursor)
        .count()
        .max(1)
}

/// Declared parameters, excluding implicit receivers like `self`.
pub fn parameters<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
) -> Vec<Node<'tree>> {
    let Some(container) = param_container(node, spec) else {
        return Vec::new();
    };

    let mut params = Vec::new();
    for param in named_children(&container) {
        if is_comment(&param) {
            continue;
        }
        // Rust models `&self` as its own node kind rather than a parameter whose text
        // happens to read "self", so both forms have to be recognised.
        if spec.implicit_param_nodes.contains(param.kind()) {
            continue;
        }
        if spec.implicit_params.contains(text(&param, source).trim()) {
            continue;
        }
        for _ in 0..declared_count(&param) {
            params.push(param);
        }
    }
    params
}

/// The annotated type of a parameter, when it has one.
/// Whether a function names a receiver (`self`, `cls`, `&self`).
///
/// Only meaningful where the language writes receivers explicitly. A Python
/// `@staticmethod` or a Rust associated function holds no instance state, so it is not
/// an instance method and LCOM4 must not count it as a responsibility of its own.
pub fn has_receiver(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    // Go declares the receiver outside the parameter list entirely.
    if let Some(field) = spec.receiver_field {
        return node.child_by_field_name(field).is_some();
    }
    let Some(container) = param_container(node, spec) else {
        return false;
    };
    named_children(&container).iter().any(|param| {
        spec.implicit_param_nodes.contains(param.kind())
            || spec.implicit_params.contains(text(param, source).trim())
    })
}

pub fn parameter_type(param: &Node, spec: &LanguageSpec, source: &[u8]) -> Option<String> {
    for current in named_children(param) {
        if spec.type_annotation_nodes.contains(current.kind()) {
            return Some(
                text(&current, source)
                    .trim_start_matches(':')
                    .trim()
                    .to_string(),
            );
        }
        // Python's typed_parameter nests the annotation under a `type` node.
        if current.kind() == "type" {
            return Some(text(&current, source).trim().to_string());
        }
    }
    param.child_by_field_name("type").map(|annotation| {
        text(&annotation, source)
            .trim_start_matches(':')
            .trim()
            .to_string()
    })
}

pub(crate) fn bare_type(type_name: &str) -> &str {
    let trimmed = type_name.trim().trim_end_matches('?').trim();
    trimmed
        .strip_suffix("| None")
        .map(str::trim)
        .unwrap_or(trimmed)
}

pub fn is_primitive_type(type_name: Option<&str>, spec: &LanguageSpec) -> bool {
    match type_name {
        None => false,
        Some("") => false,
        Some(name) => spec.primitive_types.contains(bare_type(name)),
    }
}

/// Whether a function is a constructor, by node kind, exact name, or convention.
///
/// Three spellings. Java gives a constructor a node of its own, because its name is
/// whatever the class is called. Python and Ruby have a fixed name. Go has no
/// constructor syntax at all -- `NewServer(..)` is the whole convention, and the capital
/// after the prefix is what keeps `NewlineCount` out.
pub fn is_constructor(node: &Node, name: &str, spec: &LanguageSpec) -> bool {
    if is_kind(node, &spec.constructor_nodes) || spec.constructor_names.contains(name) {
        return true;
    }
    spec.constructor_prefixes.iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_uppercase))
    })
}

pub fn is_string_type(type_name: Option<&str>, spec: &LanguageSpec) -> bool {
    match type_name {
        None => false,
        Some("") => false,
        Some(name) => {
            let trimmed = name.trim().trim_end_matches('?').trim();
            spec.string_types.contains(trimmed)
        }
    }
}
