//! Parameters and the types they carry.
//!
//! Only annotated parameters count towards the type rules. An unannotated parameter is
//! absence of evidence, not evidence of bad design — otherwise the score would measure
//! how thoroughly the code is typed rather than how well it is designed.

use crate::languages::LanguageSpec;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

/// Declared parameters, excluding implicit receivers like `self`.
pub fn parameters<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
) -> Vec<Node<'tree>> {
    let container = named_children(node)
        .into_iter()
        .find(|child| spec.param_containers.contains(child.kind()));
    let Some(container) = container else {
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
        params.push(param);
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
    let container = named_children(node)
        .into_iter()
        .find(|child| spec.param_containers.contains(child.kind()));
    let Some(container) = container else {
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
            return Some(text(&current, source).trim_start_matches(':').trim().to_string());
        }
        // Python's typed_parameter nests the annotation under a `type` node.
        if current.kind() == "type" {
            return Some(text(&current, source).trim().to_string());
        }
    }
    param
        .child_by_field_name("type")
        .map(|annotation| text(&annotation, source).trim_start_matches(':').trim().to_string())
}

pub(crate) fn bare_type(type_name: &str) -> &str {
    let trimmed = type_name.trim().trim_end_matches('?').trim();
    trimmed.strip_suffix("| None").map(str::trim).unwrap_or(trimmed)
}

pub fn is_primitive_type(type_name: Option<&str>, spec: &LanguageSpec) -> bool {
    match type_name {
        None => false,
        Some(name) if name.is_empty() => false,
        Some(name) => spec.primitive_types.contains(bare_type(name)),
    }
}

pub fn is_string_type(type_name: Option<&str>, spec: &LanguageSpec) -> bool {
    match type_name {
        None => false,
        Some(name) if name.is_empty() => false,
        Some(name) => {
            let trimmed = name.trim().trim_end_matches('?').trim();
            spec.string_types.contains(trimmed)
        }
    }
}

