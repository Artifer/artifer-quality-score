//! PHP.

use super::{set, LanguageSpec};
use std::collections::HashSet;
use tree_sitter::Node;

/// Whether a declaration carries the `static` modifier.
///
/// A class of static helpers is a namespace, not an object: it holds no instance state,
/// so cohesion has nothing to judge.
fn is_static(node: &Node, _source: &[u8]) -> bool {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.iter().any(|child| child.kind() == "static_modifier")
}

// @aqs(disable:"Large Method") a vocabulary table, not logic; splitting it would hide it
pub(super) fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "php",
        extensions: &[".php", ".phtml"],
        // The mixed grammar, not `LANGUAGE_PHP_ONLY`: a `.php` file is allowed to be
        // markup with code islands in it, and one that is still has to parse.
        language: || tree_sitter_php::LANGUAGE_PHP.into(),
        function_nodes: set(&[
            "method_declaration",
            "function_definition",
            "anonymous_function",
            "arrow_function",
        ]),
        // A trait is a class that happens to be mixed in: it holds methods and state,
        // and its cohesion is the author's to answer for. An interface's methods have
        // no body, so `is_all_stubs` already exempts it.
        class_nodes: set(&[
            "class_declaration",
            "interface_declaration",
            "trait_declaration",
            "enum_declaration",
        ]),
        module_node: "program",
        statement_containers: set(&["compound_statement", "program"]),
        // `default_statement` and `match_default_expression` are absent for the same
        // reason `switch_default` is in TypeScript: a default arm is the absence of a
        // decision, not one more of them.
        decision_nodes: set(&[
            "if_statement",
            "else_if_clause",
            "for_statement",
            "foreach_statement",
            "while_statement",
            "do_statement",
            "catch_clause",
            "case_statement",
            "match_conditional_expression",
            "conditional_expression",
        ]),
        linear_branch: None,
        boolean_op_nodes: set(&["binary_expression"]),
        // `and`/`or` are the low-precedence spellings; `??` short-circuits too.
        boolean_operators: set(&["&&", "||", "and", "or", "??"]),
        nesting_nodes: set(&[
            "if_statement",
            "for_statement",
            "foreach_statement",
            "while_statement",
            "do_statement",
            "try_statement",
            "switch_statement",
            "match_expression",
        ]),
        // An `elseif` is its own clause holding its own condition and body, so a chain
        // of them is already flat -- there is no inner `if_statement` to discount.
        continuation_nodes: HashSet::new(),
        continuation_fields: &[],
        param_containers: set(&["formal_parameters"]),
        param_field: None,
        // `$this` is implicit in a signature and mandatory at a use site, which is what
        // makes a field read and a method call the same shape here as in Python.
        implicit_params: HashSet::new(),
        implicit_param_nodes: HashSet::new(),
        receiver_field: None,
        namespace_function: Some(is_static),
        explicit_receiver: false,
        implicit_self_calls: false,
        // Both halves of a promoted constructor property (`__construct(private int
        // $amount)`) and of a plain one declare the same thing.
        field_declaration_nodes: set(&["property_declaration", "property_promotion_parameter"]),
        instance_field_nodes: HashSet::new(),
        primitive_types: set(&["int", "float", "string", "bool", "?int", "?float", "?string", "?bool"]),
        string_types: set(&["string", "?string"]),
        // A PHP parameter carries its type in a `type` field rather than in a wrapper
        // node, which `parameter_type` already falls back to.
        type_annotation_nodes: HashSet::new(),
        constructor_nodes: HashSet::new(),
        constructor_names: set(&["__construct"]),
        constructor_prefixes: &[],
        assertion_nodes: HashSet::new(),
        // PHPUnit writes `$this->assertSame(..)` and `self::assertSame(..)`; Pest and
        // Codeception write `expect(..)`.
        assertion_call_prefixes: &[
            "assert",
            "$this->assert",
            "self::assert",
            "static::assert",
            "expect",
        ],
        // A class or trait is behaviour; an interface or enum is shape.
        declaration_nodes: set(&["interface_declaration", "enum_declaration"]),
        external_interface_field: None,
    }
}
