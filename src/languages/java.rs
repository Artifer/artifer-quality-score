//! Java.

use super::{set, LanguageSpec};
use std::collections::HashSet;
use tree_sitter::Node;

/// The `default` arm of a switch, which Java writes with the same node as a `case`.
///
/// A default arm is the absence of a decision, not one more of them -- the same call
/// TypeScript gets for free, where `switch_default` is a node kind of its own.
fn is_default_label(node: &Node, source: &[u8]) -> bool {
    node.kind() == "switch_label"
        && node
            .utf8_text(source)
            .unwrap_or("")
            .trim_start()
            .starts_with("default")
}

/// Whether a declaration carries the `static` modifier.
///
/// A class of static helpers is a namespace, not an object: it holds no instance state,
/// so cohesion has nothing to judge and would otherwise report every helper as a
/// responsibility of its own.
fn is_static(node: &Node, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.iter().any(|child| {
        child.kind() == "modifiers" && child.utf8_text(source).unwrap_or("").contains("static")
    })
}

// @aqs(disable:"Large Method") a vocabulary table, not logic; splitting it would hide it
pub(super) fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "java",
        extensions: &[".java"],
        language: || tree_sitter_java::LANGUAGE.into(),
        function_nodes: set(&[
            "method_declaration",
            "constructor_declaration",
            "lambda_expression",
        ]),
        // An interface, enum or record body holds methods and is judged like a class.
        // Nothing exempts an interface by hand: its methods have no body, so
        // `is_all_stubs` already recognises it and cohesion skips it.
        class_nodes: set(&[
            "class_declaration",
            "interface_declaration",
            "enum_declaration",
            "record_declaration",
        ]),
        module_node: "program",
        statement_containers: set(&["block", "constructor_body", "program"]),
        // `switch_label` covers both switch forms -- the classic `case x:` and the
        // arrow `case x ->` -- and `linear_branch` drops the `default` ones.
        decision_nodes: set(&[
            "if_statement",
            "for_statement",
            "enhanced_for_statement",
            "while_statement",
            "do_statement",
            "catch_clause",
            "switch_label",
            "ternary_expression",
        ]),
        linear_branch: Some(is_default_label),
        boolean_op_nodes: set(&["binary_expression"]),
        boolean_operators: set(&["&&", "||"]),
        nesting_nodes: set(&[
            "if_statement",
            "for_statement",
            "enhanced_for_statement",
            "while_statement",
            "do_statement",
            "try_statement",
            "switch_expression",
        ]),
        // Java hangs an `else if` straight off the first `if`'s `alternative` field,
        // with no clause node in between -- the same shape as Go.
        continuation_nodes: HashSet::new(),
        continuation_fields: &["alternative"],
        param_containers: set(&["formal_parameters"]),
        param_field: None,
        // `this` is implicit: a method never names its receiver.
        implicit_params: HashSet::new(),
        implicit_param_nodes: HashSet::new(),
        namespace_function: Some(is_static),
        explicit_receiver: false,
        implicit_self_calls: true,
        field_declaration_nodes: set(&["field_declaration"]),
        instance_field_nodes: HashSet::new(),
        // Boxed types count too: `Integer` in a signature is the same missing domain
        // type as `int`, only nullable.
        primitive_types: set(&[
            "byte",
            "short",
            "int",
            "long",
            "float",
            "double",
            "boolean",
            "char",
            "String",
            "Byte",
            "Short",
            "Integer",
            "Long",
            "Float",
            "Double",
            "Boolean",
            "Character",
        ]),
        string_types: set(&["String", "CharSequence"]),
        // A Java parameter carries its type in a `type` field rather than in a wrapper
        // node, which `parameter_type` already falls back to.
        type_annotation_nodes: HashSet::new(),
        constructor_nodes: set(&["constructor_declaration"]),
        constructor_names: HashSet::new(),
        constructor_prefixes: &[],
        assertion_nodes: HashSet::new(),
        // JUnit and AssertJ (`assertThat`, `assertEquals`), and Mockito's `verify`.
        assertion_call_prefixes: &["assert", "verify", "expect"],
        // A class is behaviour; an interface, enum or record is shape.
        declaration_nodes: set(&[
            "interface_declaration",
            "enum_declaration",
            "record_declaration",
        ]),
        external_interface_field: None,
    }
}
