//! Python.

use super::{set, LanguageSpec};
use std::collections::HashSet;

// @aqs(disable:"Large Method") a vocabulary table, not logic; splitting it would hide it
pub(super) fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "python",
        extensions: &[".py", ".pyi"],
        language: || tree_sitter_python::LANGUAGE.into(),
        function_nodes: set(&["function_definition", "lambda"]),
        class_nodes: set(&["class_definition"]),
        module_node: "module",
        statement_containers: set(&["block", "module"]),
        decision_nodes: set(&[
            "if_statement",
            "elif_clause",
            "for_statement",
            "while_statement",
            "except_clause",
            "conditional_expression",
            "case_clause",
            "if_clause",     // comprehension filter
            "for_in_clause", // comprehension generator
        ]),
        linear_branch: None,
        boolean_op_nodes: set(&["boolean_operator"]),
        boolean_operators: set(&["and", "or"]),
        nesting_nodes: set(&[
            "if_statement",
            "for_statement",
            "while_statement",
            "try_statement",
            "with_statement",
            "match_statement",
        ]),
        continuation_nodes: set(&["elif_clause", "else_clause"]),
        continuation_fields: &[],
        param_containers: set(&["parameters", "lambda_parameters"]),
        param_field: None,
        receiver_field: None,
        implicit_params: set(&["self", "cls", "mcs", "metacls"]),
        implicit_param_nodes: HashSet::new(),
        explicit_receiver: true,
        implicit_self_calls: false,
        namespace_function: None,
        field_declaration_nodes: HashSet::new(),
        instance_field_nodes: HashSet::new(),
        primitive_types: set(&[
            "str",
            "int",
            "float",
            "bool",
            "bytes",
            "complex",
            "bytearray",
        ]),
        string_types: set(&["str", "bytes"]),
        type_annotation_nodes: set(&["type"]),
        constructor_nodes: HashSet::new(),
        constructor_names: set(&["__init__", "__new__"]),
        constructor_prefixes: &[],
        assertion_nodes: set(&["assert_statement"]),
        assertion_call_prefixes: &["assert", "expect", "self.assert"],
        declaration_nodes: HashSet::new(),
        external_interface_field: None,
    }
}
