//! AST measurement primitives shared by the detectors.
//!
//! Split by what is being measured. Every name is re-exported here, so callers keep
//! saying `metrics::cyclomatic_complexity` and never need to know which file it lives in.
//!
//! | module | measures |
//! |---|---|
//! | [`traversal`] | walking the tree, reading text |
//! | [`naming`] | what a function is called, and whether it is its own unit |
//! | [`size`] | lines of code, token signatures |
//! | [`complexity`] | branching, nesting, bumps, conditionals |
//! | [`signatures`] | parameters and their types |
//! | [`classes`] | classes, interfaces, and non-cohesion-bearing methods |
//! | [`assertions`] | assertions in all three dialects |
//!
//! Traversal order matches the original single-module implementation exactly (pre-order,
//! document order). It has to: the per-rule cap allocation in `scoring` breaks ties by
//! discovery order, so a different walk order would produce different penalties.

pub mod assertions;
pub mod classes;
pub mod complexity;
pub mod naming;
pub mod signatures;
pub mod size;
pub mod traversal;

pub use assertions::is_assertion;
pub use classes::{class_nodes, is_abstract_class, is_pure_accessor};
pub use complexity::{
    bumps, complex_conditionals, count_boolean_operators, cyclomatic_complexity, function_body,
    nesting_depth, Bump,
};
pub use naming::{class_name, function_name, is_scope_boundary, iter_functions};
pub use signatures::{
    has_receiver, is_primitive_type, is_string_type, parameter_type, parameters,
};
pub use size::{is_docstring, logical_lines, normalised_tokens};
pub use traversal::{all_nodes, named_children, own_nodes, text, to_range, COMMENT_NODES};

// Helpers the submodules share with each other but callers never need. Re-exported so
// that each submodule's `use super::*` resolves them.
pub(crate) use traversal::{children, is_comment};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages::LanguageSpec;
    use crate::languages::spec_for_name;
    use tree_sitter::Parser;

    fn parse(source: &str, language: &str) -> (tree_sitter::Tree, &'static LanguageSpec) {
        let spec = spec_for_name(language).expect("known language");
        let mut parser = Parser::new();
        parser.set_language(&spec.language()).expect("grammar loads");
        (parser.parse(source, None).expect("parses"), spec)
    }

    #[test]
    fn complexity_counts_branches_and_boolean_operators() {
        let source = "def f(a, b):\n    if a and b:\n        return 1\n    return 0\n";
        let (tree, spec) = parse(source, "python");
        let functions = iter_functions(&tree.root_node(), spec, source.as_bytes());
        assert_eq!(cyclomatic_complexity(&functions[0], spec, source.as_bytes()), 3);
    }

    #[test]
    fn elif_chains_stay_flat() {
        let source = "def f(x):\n    if x == 0:\n        return 0\n    elif x == 1:\n        return 1\n    else:\n        return 2\n";
        let (tree, spec) = parse(source, "python");
        let functions = iter_functions(&tree.root_node(), spec, source.as_bytes());
        assert_eq!(nesting_depth(&functions[0], spec, source.as_bytes()), 1);
    }

    #[test]
    fn else_if_chains_stay_flat_in_typescript() {
        let source = "function f(x: number) { if (x === 0) { return 0 } else if (x === 1) { return 1 } else { return 2 } }";
        let (tree, spec) = parse(source, "typescript");
        let functions = iter_functions(&tree.root_node(), spec, source.as_bytes());
        assert_eq!(nesting_depth(&functions[0], spec, source.as_bytes()), 1);
    }

    #[test]
    fn docstrings_do_not_count_as_lines_of_code() {
        let source = "def f():\n    \"\"\"Doc.\n\n    More.\n    \"\"\"\n    return 1\n";
        let (tree, spec) = parse(source, "python");
        let functions = iter_functions(&tree.root_node(), spec, source.as_bytes());
        assert_eq!(logical_lines(&functions[0]), 2);
    }

    #[test]
    fn self_is_not_a_parameter() {
        let source = "class C:\n    def f(self, a, b):\n        return a\n";
        let (tree, spec) = parse(source, "python");
        let functions = iter_functions(&tree.root_node(), spec, source.as_bytes());
        let method = functions.iter().find(|f| f.kind() == "function_definition").unwrap();
        assert_eq!(parameters(method, spec, source.as_bytes()).len(), 2);
    }

    #[test]
    fn destructured_object_is_one_parameter() {
        let source = "export function T({ a, b, c, d, e, f }: Props) { return null }";
        let (tree, spec) = parse(source, "typescript");
        let functions = iter_functions(&tree.root_node(), spec, source.as_bytes());
        assert_eq!(parameters(&functions[0], spec, source.as_bytes()).len(), 1);
    }

    #[test]
    fn abstract_base_classes_are_recognised() {
        let source = "from abc import ABC, abstractmethod\nclass Base(ABC):\n    @abstractmethod\n    def a(self) -> str: ...\n";
        let (tree, spec) = parse(source, "python");
        let classes = class_nodes(&tree.root_node(), spec);
        assert!(is_abstract_class(&classes[0], spec, source.as_bytes()));
    }

    #[test]
    fn assert_calls_are_detected_without_prefix_mangling() {
        // Guards the `lstrip("await ")` trap: character-set stripping would turn
        // `assertEqual(...)` into `ssertEqual(...)` and lose the match.
        let source = "def test_x():\n    assertEqual(a, b)\n";
        let (tree, spec) = parse(source, "python");
        let statements = all_nodes(&tree.root_node());
        assert!(statements
            .iter()
            .any(|n| is_assertion(n, spec, source.as_bytes())));
    }

    #[test]
    fn awaited_expect_is_detected() {
        let source = "it('x', async () => { await expect(page).toHaveText('a'); });";
        let (tree, spec) = parse(source, "typescript");
        let statements = all_nodes(&tree.root_node());
        assert!(statements
            .iter()
            .any(|n| is_assertion(n, spec, source.as_bytes())));
    }
}
