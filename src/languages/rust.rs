//! Rust.

use super::{set, LanguageSpec};
use std::collections::HashSet;

pub(super) fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "rust",
        extensions: &[".rs"],
        language: || tree_sitter_rust::LANGUAGE.into(),
        // A closure is anonymous unless bound to a name, which `function_name` handles
        // via the `let_declaration` parent.
        function_nodes: set(&["function_item", "closure_expression"]),
        // Only inherent `impl` blocks. A `trait` declaration is an interface, and its
        // methods are signatures rather than behaviour, so there is nothing to judge.
        class_nodes: set(&["impl_item"]),
        module_node: "source_file",
        statement_containers: set(&["block"]),
        // `try_expression` -- the `?` operator -- is deliberately absent. It is linear
        // error propagation, not a branch a reader has to hold in mind, and counting it
        // would penalise idiomatic Rust for being explicit about failure.
        decision_nodes: set(&[
            "if_expression",
            "while_expression",
            "for_expression",
            "loop_expression",
            "match_arm",
        ]),
        linear_branch: None,
        boolean_op_nodes: set(&["binary_expression"]),
        boolean_operators: set(&["&&", "||"]),
        nesting_nodes: set(&[
            "if_expression",
            "while_expression",
            "for_expression",
            "loop_expression",
            "match_expression",
        ]),
        continuation_nodes: set(&["else_clause"]),
        continuation_fields: &[],
        param_containers: set(&["parameters", "closure_parameters"]),
        param_field: None,
        implicit_params: set(&["self", "&self", "&mut self", "mut self"]),
        implicit_param_nodes: set(&["self_parameter"]),
        explicit_receiver: true,
        implicit_self_calls: false,
        namespace_function: None,
        field_declaration_nodes: HashSet::new(),
        instance_field_nodes: HashSet::new(),
        primitive_types: set(&[
            "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
            "f32", "f64", "bool", "char", "str", "&str", "String", "&String",
        ]),
        string_types: set(&["str", "&str", "String", "&String"]),
        // A Rust parameter carries its type in a `type` field rather than in a wrapper
        // node, which `parameter_type` already falls back to.
        type_annotation_nodes: HashSet::new(),
        // `new` is the convention, so a `new` with a long argument list is the same
        // smell Constructor Over-Injection describes elsewhere.
        constructor_nodes: HashSet::new(),
        constructor_names: set(&["new"]),
        constructor_prefixes: &[],
        assertion_nodes: HashSet::new(),
        assertion_call_prefixes: &["assert", "debug_assert", "expect", "panic"],
        declaration_nodes: set(&["struct_item", "enum_item", "trait_item", "type_item"]),
        external_interface_field: Some("trait"),
    }
}
