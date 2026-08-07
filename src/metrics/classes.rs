//! Classes, interfaces, and the methods that say nothing about cohesion.
//!
//! Constructors touch every field, pure accessors touch exactly one, associated
//! functions touch no instance at all, and interfaces have no shared state by
//! definition. None of them tells you how a class is divided, so each is excluded from
//! LCOM4 — otherwise the metric condemns value objects and ABCs for existing.

use crate::languages::LanguageSpec;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

/// Base classes that mark a type as an interface rather than an implementation.
pub(crate) const ABSTRACT_BASES: &[&str] = &["ABC", "ABCMeta", "Protocol", "Interface"];

/// Decorators that mark a method as having no implementation here.
pub(crate) const ABSTRACT_DECORATORS: &[&str] = &["abstractmethod", "abstractproperty", "abc.abstract"];

pub fn class_nodes<'tree>(root: &Node<'tree>, spec: &LanguageSpec) -> Vec<Node<'tree>> {
    all_nodes(root)
        .into_iter()
        .filter(|node| spec.class_nodes.contains(node.kind()) && node.id() != root.id())
        .collect()
}

/// A body that is only a docstring, `pass`, `...`, or NotImplementedError.
pub(crate) fn has_stub_body(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    let Some(body) = function_body(node, spec) else {
        return false;
    };
    for statement in named_children(&body) {
        if is_comment(&statement) || is_docstring(&statement) {
            continue;
        }
        if matches!(statement.kind(), "pass_statement" | "empty_statement") {
            continue;
        }
        let body_text = text(&statement, source).trim();
        let is_placeholder = matches!(body_text, "..." | "...;");
        if is_placeholder || body_text.contains("NotImplementedError") {
            continue;
        }
        return false;
    }
    true
}

/// The one meaningful statement in a body, or None if there are none or several.
pub(crate) fn sole_statement<'tree>(node: &Node<'tree>, spec: &LanguageSpec) -> Option<Node<'tree>> {
    let body = function_body(node, spec)?;
    let mut only: Option<Node> = None;
    for statement in named_children(&body) {
        if is_comment(&statement) || is_docstring(&statement) {
            continue;
        }
        if only.is_some() {
            return None;
        }
        only = Some(statement);
    }
    only
}

/// Whether a call is a plain delegation to the superclass.
pub(crate) fn is_super_call(node: &Node, source: &[u8]) -> bool {
    let body = text(node, source);
    body.contains("super()") || body.contains("super.")
}

/// Whether the statement is a bare delegation to the superclass.
///
/// `return super().update(deleted_at=timezone.now())` is a delegation even though it
/// computes an argument on the way. Requiring the whole statement to be call-free would
/// reject it and count the method as a responsibility of its own — which is how a
/// perfectly cohesive Django QuerySet ends up flagged.
pub(crate) fn delegates_to_super(statement: &Node, source: &[u8]) -> bool {
    let body = text(statement, source).trim_start();
    body.starts_with("return super()")
        || body.starts_with("super()")
        || body.starts_with("return super.")
        || body.starts_with("super.")
}

/// Whether a method is a pure projection of stored state, or a bare super() delegation.
///
/// One statement, no branching, and either it delegates to the superclass or it contains
/// no calls at all and touches a field of `self`:
/// `fn threshold(&self) -> f64 { self.thresholds[key] }`, `return self.amount * 2`.
///
/// LCOM4 counts every method touching a distinct field as its own component, so a value
/// object with six accessors scores 6 and reads as six unrelated jobs. Splitting it would
/// be absurd, so accessors are excluded like constructors and abstract stubs.
///
/// The "no calls" condition is what keeps the projection case narrow, and it is
/// load-bearing. `self.orders.append(o)` and `len(self.orders)` both call something, so a
/// class that merely appends to two different collections is still correctly condemned —
/// that is real low cohesion, not an accessor.
pub fn is_pure_accessor(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    let Some(statement) = sole_statement(node, spec) else {
        return false;
    };
    if delegates_to_super(&statement, source) {
        return true;
    }

    let mut touches_state = false;
    for current in own_nodes(&statement, spec, source) {
        let kind = current.kind();
        if spec.nesting_nodes.contains(kind) || spec.decision_nodes.contains(kind) {
            return false;
        }
        let is_call = matches!(
            kind,
            "call" | "call_expression" | "macro_invocation" | "await_expression" | "await"
        );
        if is_call && !is_super_call(&current, source) {
            return false;
        }
        if is_call || matches!(kind, "attribute" | "member_expression" | "field_expression") {
            touches_state = true;
        }
    }
    touches_state
}

/// Whether the class declares an abstract base such as `ABC` or `Protocol`.
pub(crate) fn has_abstract_base(node: &Node, source: &[u8]) -> bool {
    let Some(superclasses) = node.child_by_field_name("superclasses") else {
        return false;
    };
    named_children(&superclasses).iter().any(|child| {
        let raw = text(child, source);
        let base = raw.split('[').next().unwrap_or(raw);
        let leaf = base.rsplit('.').next().unwrap_or(base);
        ABSTRACT_BASES.contains(&leaf)
    })
}

/// Whether any decorator in the class marks a method as abstract.
pub(crate) fn has_abstract_decorator(node: &Node, source: &[u8]) -> bool {
    all_nodes(node)
        .iter()
        .filter(|current| matches!(current.kind(), "decorator" | "decorator_list"))
        .any(|current| {
            let decorator = text(current, source);
            ABSTRACT_DECORATORS.iter().any(|marker| decorator.contains(marker))
        })
}

/// Whether every method in the class is a body-less stub.
pub(crate) fn is_all_stubs(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let methods: Vec<Node> = all_nodes(&body)
        .into_iter()
        .filter(|child| {
            spec.function_nodes.contains(child.kind()) && function_name(child, source).is_some()
        })
        .collect();
    !methods.is_empty() && methods.iter().all(|m| has_stub_body(m, spec, source))
}

/// Whether the class implements an interface defined elsewhere.
///
/// Rust's `impl Trait for Type` sets a `trait` field. The method set comes from the
/// trait, not from the author, so the grouping is not theirs to answer for.
pub(crate) fn implements_external_interface(node: &Node, spec: &LanguageSpec) -> bool {
    spec.external_interface_field
        .is_some_and(|field| node.child_by_field_name(field).is_some())
}

/// Whether a class is an interface — abstract, Protocol, all-stubs, or a trait impl.
///
/// Interfaces have no shared state and no internal calls by definition, so cohesion
/// metrics always condemn them. Flagging an ABC for "low cohesion" would be telling the
/// author to stop declaring interfaces.
pub fn is_abstract_class(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    node.kind().contains("abstract")
        || node.prev_sibling().is_some_and(|p| p.kind() == "abstract")
        || implements_external_interface(node, spec)
        || has_abstract_base(node, source)
        || has_abstract_decorator(node, source)
        || is_all_stubs(node, spec, source)
}

