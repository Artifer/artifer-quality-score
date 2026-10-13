//! Classes, interfaces, and the methods that say nothing about cohesion.
//!
//! Constructors touch every field, pure accessors touch exactly one, associated
//! functions touch no instance at all, and interfaces have no shared state by
//! definition. None of them tells you how a class is divided, so each is excluded from
//! LCOM4 — otherwise the metric condemns value objects and ABCs for existing.

use crate::languages::LanguageSpec;
use std::collections::HashSet;
use tree_sitter::Node;

#[allow(unused_imports)]
use super::*;

/// Base classes that mark a type as an interface rather than an implementation.
pub(crate) const ABSTRACT_BASES: &[&str] = &["ABC", "ABCMeta", "Protocol", "Interface"];

/// Decorators that mark a method as having no implementation here.
pub(crate) const ABSTRACT_DECORATORS: &[&str] =
    &["abstractmethod", "abstractproperty", "abc.abstract"];

pub fn class_nodes<'tree>(root: &Node<'tree>, spec: &LanguageSpec) -> Vec<Node<'tree>> {
    all_nodes(root)
        .into_iter()
        .filter(|node| is_kind(node, &spec.class_nodes) && node.id() != root.id())
        .collect()
}

/// A body that is only a docstring, `pass`, `...`, or NotImplementedError.
pub(crate) fn has_stub_body(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    // No body at all is the most stubbed a method can be: a TypeScript overload
    // signature, or Ruby's `def print_class(*) end` null object.
    let Some(body) = function_body(node, spec) else {
        return true;
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
pub(crate) fn sole_statement<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
) -> Option<Node<'tree>> {
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
    // Ruby's bare `super` forwards the arguments it was given and is a node of its own,
    // which is the one form no prefix would catch without also catching `superuser`.
    if statement.kind() == "super" {
        return true;
    }
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
/// `fn threshold(&self) -> f64 { self.thresholds[key] }`, `return self.amount * 2`,
/// `long getAmount() { return amount; }`.
///
/// LCOM4 counts every method touching a distinct field as its own component, so a value
/// object with six accessors scores 6 and reads as six unrelated jobs. Splitting it would
/// be absurd, so accessors are excluded like constructors and abstract stubs.
///
/// The "no calls" condition is what keeps the projection case narrow, and it is
/// load-bearing. `self.orders.append(o)` and `len(self.orders)` both call something, so a
/// class that merely appends to two different collections is still correctly condemned —
/// that is real low cohesion, not an accessor.
pub fn is_pure_accessor(
    node: &Node,
    spec: &LanguageSpec,
    source: &[u8],
    declared: &HashSet<String>,
) -> bool {
    let Some(statement) = sole_statement(node, spec) else {
        return false;
    };
    if delegates_to_super(&statement, source) {
        return true;
    }

    let mut touches_state = false;
    for current in own_nodes(&statement, spec, source) {
        if is_kind(&current, &spec.nesting_nodes) || is_kind(&current, &spec.decision_nodes) {
            return false;
        }
        if is_call(&current) {
            if !is_super_call(&current, source) {
                return false;
            }
            touches_state = true;
            continue;
        }
        touches_state |= reads_instance_state(&current, spec, source, declared);
    }
    touches_state
}

/// Whether a node reads a field of the object the method is running on.
///
/// Three spellings: a receiver plus a member (`self.amount`, `this.amount`), a node that
/// is the field (Ruby's `@amount`), and a bare name that the class declared as a field,
/// which is how Java, PHP and Ruby all let you reach one.
fn reads_instance_state(
    node: &Node,
    spec: &LanguageSpec,
    source: &[u8],
    declared: &HashSet<String>,
) -> bool {
    if FIELD_ACCESS_NODES.contains(&node.kind()) || is_kind(node, &spec.instance_field_nodes) {
        return true;
    }
    node.kind() == "identifier" && declared.contains(text(node, source))
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
            ABSTRACT_DECORATORS
                .iter()
                .any(|marker| decorator.contains(marker))
        })
}

/// Whether every method in the class is a body-less stub.
///
/// The constructor does not count: it is setup, not behaviour. A null object that
/// remembers its arguments and then does nothing at all -- `def print_class(*) end`,
/// nine times over -- is still an interface, and telling its author it has nine
/// unrelated responsibilities is telling them to stop writing null objects.
pub(crate) fn is_all_stubs(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let methods: Vec<Node> = all_nodes(&body)
        .into_iter()
        .filter(|child| {
            is_kind(child, &spec.function_nodes)
                && function_name(child, source)
                    .is_some_and(|name| !is_constructor(child, &name, spec))
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

/// Whether a class is an interface — an abstract base, a Protocol, all stubs, or a
/// trait impl.
///
/// Interfaces have no shared state and no internal calls by definition, so cohesion
/// metrics always condemn them. Flagging an ABC for "low cohesion" would be telling the
/// author to stop declaring interfaces.
pub fn is_abstract_class(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    implements_external_interface(node, spec)
        || has_abstract_base(node, source)
        || has_abstract_decorator(node, source)
        || is_all_stubs(node, spec, source)
}
