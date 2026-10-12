//! Go.

use super::{set, LanguageSpec};
use std::collections::HashSet;
use tree_sitter::Node;

/// `if err != nil { .. return .. }`: Go's spelling of Rust's `?`.
///
/// The reader does not hold this branch in mind -- it is the failure path, going
/// straight back out -- and Rust's `?` is already not counted. Counting it would mean
/// every Go function is judged for how many things it calls that can fail.
///
/// Narrow on purpose: the condition must compare an `err`-named value against `nil`,
/// there must be no `else`, and the body must end in `return`. An `if err != nil` that
/// recovers and carries on *is* a decision, and still counts.
fn is_go_error_guard(node: &Node, source: &[u8]) -> bool {
    if node.kind() != "if_statement" || node.child_by_field_name("alternative").is_some() {
        return false;
    }
    let comparison = node
        .child_by_field_name("condition")
        .filter(|condition| condition.kind() == "binary_expression");
    let Some(condition) = comparison else {
        return false;
    };
    let field = |name: &str| {
        condition
            .child_by_field_name(name)
            .map(|n| n.utf8_text(source).unwrap_or("").trim().to_string())
            .unwrap_or_default()
    };
    let name = field("left");
    if field("operator") != "!=" || field("right") != "nil" {
        return false;
    }
    if !name.ends_with("err") && !name.ends_with("Err") {
        return false;
    }
    node.child_by_field_name("consequence")
        .and_then(|block| last_statement_kind(&block))
        .is_some_and(|kind| kind == "return_statement")
}

/// The kind of the last statement in a block, looking through Go's `statement_list`.
fn last_statement_kind(block: &Node) -> Option<&'static str> {
    let mut cursor = block.walk();
    let last = block.named_children(&mut cursor).last()?;
    if last.kind() == "statement_list" {
        return last_statement_kind(&last);
    }
    Some(last.kind())
}

// @aqs(disable:"Large Method") a vocabulary table, not logic; splitting it would hide it
pub(super) fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "go",
        extensions: &[".go"],
        language: || tree_sitter_go::LANGUAGE.into(),
        // `func_literal` is Go's anonymous function; `naming.rs` gives it a name when it
        // is bound with `helper := func() {..}`.
        function_nodes: set(&["function_declaration", "method_declaration", "func_literal"]),
        // Deliberately empty. Go has no node that owns a type *and* its methods -- a
        // method is a top-level declaration that names its receiver -- so there is no
        // class scope to measure. The class rules (LCOM4 cohesion, Brain Class, function
        // count per class) ask a question Go's syntax cannot answer, and inventing an
        // answer from `type X struct` would judge every type as having zero methods.
        class_nodes: HashSet::new(),
        module_node: "source_file",
        // Go's `block` is a wrapper: the statements themselves hang off the
        // `statement_list` inside it, which is what actually holds a sequence.
        statement_containers: set(&["statement_list"]),
        // `default_case` is excluded for the same reason `switch_default` is in TS: the
        // default arm is the absence of a decision, not one more of them.
        decision_nodes: set(&[
            "if_statement",
            "for_statement",
            "expression_case",
            "type_case",
            "communication_case",
        ]),
        linear_branch: Some(is_go_error_guard),
        boolean_op_nodes: set(&["binary_expression"]),
        boolean_operators: set(&["&&", "||"]),
        nesting_nodes: set(&[
            "if_statement",
            "for_statement",
            "expression_switch_statement",
            "type_switch_statement",
            "select_statement",
        ]),
        // Go models `else if` as a bare `if_statement` in the outer `if`'s `alternative`
        // field, with no clause node in between, so the continuation is a field rather
        // than a node type.
        continuation_nodes: HashSet::new(),
        continuation_fields: &["alternative"],
        param_containers: set(&["parameter_list"]),
        param_field: Some("parameters"),
        // A receiver is named by the author (`m`, `s`, `srv`), so there is no list of
        // names to recognise it by -- only the field it sits in, which `param_field`
        // already steps past. Go has no class nodes, so nothing asks about receivers.
        implicit_params: HashSet::new(),
        implicit_param_nodes: HashSet::new(),
        explicit_receiver: true,
        implicit_self_calls: false,
        namespace_function: None,
        field_declaration_nodes: HashSet::new(),
        instance_field_nodes: HashSet::new(),
        primitive_types: set(&[
            "bool",
            "string",
            "int",
            "int8",
            "int16",
            "int32",
            "int64",
            "uint",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "uintptr",
            "byte",
            "rune",
            "float32",
            "float64",
            "complex64",
            "complex128",
        ]),
        string_types: set(&["string", "*string", "[]byte"]),
        // A Go parameter carries its type in a `type` field rather than in a wrapper
        // node, which `parameter_type` already falls back to.
        type_annotation_nodes: HashSet::new(),
        constructor_nodes: HashSet::new(),
        constructor_names: HashSet::new(),
        // `NewThing(..)` is the convention, and a long argument list on one is the same
        // smell Constructor Over-Injection describes elsewhere.
        constructor_prefixes: &["New"],
        assertion_nodes: HashSet::new(),
        // `t.Error`/`t.Fatal` cover the `f` variants; `assert`/`require` cover testify,
        // which is what most Go suites reach for.
        assertion_call_prefixes: &["assert", "require", "t.Error", "t.Fatal", "expect"],
        // `type_declaration` wraps the `type_spec`, so listing only the outer node keeps
        // a struct from being counted twice.
        declaration_nodes: set(&["type_declaration"]),
        external_interface_field: None,
    }
}
