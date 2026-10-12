//! Ruby.

use super::{set, LanguageSpec};
use std::collections::HashSet;

// @aqs(disable:"Large Method") a vocabulary table, not logic; splitting it would hide it
pub(super) fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "ruby",
        extensions: &[".rb", ".rake", ".gemspec"],
        language: || tree_sitter_ruby::LANGUAGE.into(),
        // `block` (`{ .. }`) and `do_block` are Ruby's anonymous callbacks. They are
        // listed so a `->(z) { .. }` bound to a name becomes its own unit; an anonymous
        // `items.each do |i|` has no name and so is measured as part of its method,
        // which is where a reader meets it.
        function_nodes: set(&["method", "singleton_method", "lambda", "block", "do_block"]),
        // A `module` is judged like a class: a mixin holds methods and state the same
        // way one does. A namespace module holds no methods of its own, and
        // `class_methods` only counts methods whose *nearest* class is this one, so it
        // is skipped rather than credited with everything nested inside it.
        class_nodes: set(&["class", "module", "singleton_class"]),
        module_node: "program",
        statement_containers: set(&[
            "program",
            "body_statement",
            "block_body",
            "then",
            "else",
            "do",
            "ensure",
        ]),
        // `else` is not a decision, and neither is `ensure`. Modifier forms are: a
        // trailing `if`/`unless`/`while` is the same branch written right to left.
        decision_nodes: set(&[
            "if",
            "elsif",
            "unless",
            "while",
            "until",
            "for",
            "when",
            "in_clause",
            "rescue",
            "conditional",
            "if_modifier",
            "unless_modifier",
            "while_modifier",
            "until_modifier",
            "rescue_modifier",
        ]),
        linear_branch: None,
        boolean_op_nodes: set(&["binary"]),
        // `and`/`or` are the low-precedence spellings of the same two operators.
        boolean_operators: set(&["&&", "||", "and", "or"]),
        // Modifiers are absent on purpose: `return unless valid?` is a guard clause, and
        // the whole point of writing it that way is that it does not indent the method.
        nesting_nodes: set(&[
            "if",
            "unless",
            "while",
            "until",
            "for",
            "case",
            "case_match",
            "begin",
        ]),
        // An `elsif` is a child of the `if` it continues, and is not a nesting node, so
        // a chain of them stays flat without any continuation rule.
        continuation_nodes: HashSet::new(),
        continuation_fields: &[],
        param_containers: set(&["method_parameters", "block_parameters", "lambda_parameters"]),
        param_field: None,
        // `self` is implicit in Ruby: a method never names its receiver, so its absence
        // proves nothing about whether the method holds instance state.
        implicit_params: HashSet::new(),
        implicit_param_nodes: HashSet::new(),
        // `def self.parse` is a namespace function, not a method with an instance.
        namespace_function: Some(|node, _| node.kind() == "singleton_method"),
        explicit_receiver: false,
        implicit_self_calls: true,
        field_declaration_nodes: HashSet::new(),
        instance_field_nodes: set(&["instance_variable"]),
        // Ruby annotates nothing, and the type rules only judge what is annotated --
        // an unannotated parameter is absence of evidence, not evidence of bad design.
        primitive_types: HashSet::new(),
        string_types: HashSet::new(),
        type_annotation_nodes: HashSet::new(),
        constructor_nodes: HashSet::new(),
        constructor_names: set(&["initialize"]),
        constructor_prefixes: &[],
        assertion_nodes: HashSet::new(),
        // minitest (`assert_*`, `refute_*`), RSpec (`expect(..).to`), and the
        // `must_`/`wont_` spec syntax.
        assertion_call_prefixes: &[
            "assert",
            "refute",
            "expect",
            "is_expected",
            "must_",
            "wont_",
            "should",
        ],
        // Ruby declares no types, so there is nothing for Lines of Declarations to see.
        declaration_nodes: HashSet::new(),
        external_interface_field: None,
    }
}
