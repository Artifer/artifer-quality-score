//! TypeScript, JavaScript, and the Vue single-file components that resolve to them.

use super::{set, LanguageSpec};
use std::collections::HashSet;
use tree_sitter::Language;

const TS_FUNCTIONS: &[&str] = &[
    "function_declaration",
    "function_expression",
    "generator_function",
    "generator_function_declaration",
    "arrow_function",
    "method_definition",
];

const TS_DECISION: &[&str] = &[
    "if_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
    "catch_clause",
    "switch_case", // switch_default is deliberately excluded
    "ternary_expression",
];

const TS_NESTING: &[&str] = &[
    "if_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
    "try_statement",
    "switch_statement",
];

fn typescript_like(
    name: &'static str,
    extensions: &'static [&'static str],
    language: fn() -> Language,
) -> LanguageSpec {
    LanguageSpec {
        name,
        extensions,
        language,
        function_nodes: set(TS_FUNCTIONS),
        // `abstract class` is a node kind of its own, and a class like any other for
        // every rule but cohesion.
        class_nodes: set(&["class_declaration", "abstract_class_declaration", "class"]),
        module_node: "program",
        statement_containers: set(&["statement_block", "program"]),
        decision_nodes: set(TS_DECISION),
        // TS/JS model `&&`, `||` and `??` as binary_expression; the operator set
        // below is what distinguishes them from arithmetic.
        linear_branch: None,
        boolean_op_nodes: set(&["binary_expression"]),
        boolean_operators: set(&["&&", "||", "??"]),
        nesting_nodes: set(TS_NESTING),
        continuation_nodes: set(&["else_clause"]),
        continuation_fields: &[],
        param_containers: set(&["formal_parameters"]),
        param_field: None,
        implicit_params: set(&["this"]),
        implicit_param_nodes: HashSet::new(),
        explicit_receiver: false,
        implicit_self_calls: false,
        namespace_function: None,
        field_declaration_nodes: HashSet::new(),
        instance_field_nodes: HashSet::new(),
        primitive_types: set(&["string", "number", "boolean", "bigint", "symbol"]),
        string_types: set(&["string"]),
        type_annotation_nodes: set(&["type_annotation"]),
        constructor_nodes: HashSet::new(),
        constructor_names: set(&["constructor"]),
        constructor_prefixes: &[],
        assertion_nodes: HashSet::new(),
        assertion_call_prefixes: &["expect", "assert", "chai.assert"],
        declaration_nodes: set(&[
            "interface_declaration",
            "type_alias_declaration",
            "enum_declaration",
        ]),
        external_interface_field: None,
    }
}

/// The tsx grammar is a superset of the ts grammar and parses both; using it for `.ts`
/// too avoids a second grammar and keeps node names identical across both.
pub(super) fn typescript() -> LanguageSpec {
    typescript_like("typescript", &[".ts", ".tsx", ".mts", ".cts"], || {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    })
}

/// `.vue` and `.svelte` resolve here so a single-file component is discovered at all;
/// whether its script is JavaScript or TypeScript is decided from the block's own
/// `lang` attribute when the file is read (`sfc.rs`).
pub(super) fn javascript() -> LanguageSpec {
    typescript_like(
        "javascript",
        &[".js", ".jsx", ".mjs", ".cjs", ".vue", ".svelte"],
        || tree_sitter_javascript::LANGUAGE.into(),
    )
}
