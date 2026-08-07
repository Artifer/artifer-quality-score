//! Per-language AST vocabulary.
//!
//! Every detector is written against this declarative spec rather than against a
//! specific grammar, so adding a language means adding a `LanguageSpec` and no
//! detector changes.
//!
//! The node-type names here were probed empirically against the pinned grammar
//! versions in `Cargo.toml` and match the Python reference implementation exactly.

use std::collections::HashSet;
use std::sync::OnceLock;
use tree_sitter::Language;

pub struct LanguageSpec {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    language: fn() -> Language,

    /// Nodes that introduce a callable scope.
    pub function_nodes: HashSet<&'static str>,
    /// Nodes that introduce a class scope.
    pub class_nodes: HashSet<&'static str>,
    /// The translation-unit root node type.
    pub module_node: &'static str,

    /// Each occurrence adds 1 to cyclomatic complexity.
    pub decision_nodes: HashSet<&'static str>,
    /// Nodes representing a short-circuit boolean operator. For grammars that model
    /// these as a generic binary node, `boolean_operators` disambiguates by operator
    /// text.
    pub boolean_op_nodes: HashSet<&'static str>,
    pub boolean_operators: HashSet<&'static str>,

    /// Nodes that open a nesting level.
    pub nesting_nodes: HashSet<&'static str>,
    /// Clauses that continue an existing branch rather than nesting deeper, so that
    /// `else if` / `elif` chains read as flat instead of as a staircase.
    pub continuation_nodes: HashSet<&'static str>,

    /// Nodes holding a callable's parameter list.
    pub param_containers: HashSet<&'static str>,
    /// Implicit receiver parameters that should not count against a signature.
    pub implicit_params: HashSet<&'static str>,

    /// Type names treated as primitives for Primitive Obsession.
    pub primitive_types: HashSet<&'static str>,
    /// Type names treated as strings for String Heavy Function Arguments.
    pub string_types: HashSet<&'static str>,

    /// Node types carrying a type annotation.
    pub type_annotation_nodes: HashSet<&'static str>,

    /// Name of a constructor / initialiser, for Constructor Over-Injection.
    pub constructor_names: HashSet<&'static str>,

    /// Statement nodes that are assertions.
    pub assertion_nodes: HashSet<&'static str>,
    /// Call-callee prefixes that mark an assertion (e.g. `expect`, `assert*`).
    pub assertion_call_prefixes: &'static [&'static str],

    /// Nodes that declare types rather than behaviour (Lines of Declarations).
    pub declaration_nodes: HashSet<&'static str>,
}

impl LanguageSpec {
    pub fn language(&self) -> Language {
        (self.language)()
    }
}

fn set(items: &[&'static str]) -> HashSet<&'static str> {
    items.iter().copied().collect()
}

fn python_spec() -> LanguageSpec {
    LanguageSpec {
        name: "python",
        extensions: &[".py", ".pyi"],
        language: || tree_sitter_python::LANGUAGE.into(),
        function_nodes: set(&["function_definition", "lambda"]),
        class_nodes: set(&["class_definition"]),
        module_node: "module",
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
        param_containers: set(&["parameters", "lambda_parameters"]),
        implicit_params: set(&["self", "cls", "mcs", "metacls"]),
        primitive_types: set(&[
            "str", "int", "float", "bool", "bytes", "complex", "bytearray",
        ]),
        string_types: set(&["str", "bytes"]),
        type_annotation_nodes: set(&["type"]),
        constructor_names: set(&["__init__", "__new__"]),
        assertion_nodes: set(&["assert_statement"]),
        assertion_call_prefixes: &["assert", "expect", "self.assert"],
        declaration_nodes: HashSet::new(),
    }
}

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
        class_nodes: set(&["class_declaration", "class"]),
        module_node: "program",
        decision_nodes: set(TS_DECISION),
        // TS/JS model `&&`, `||` and `??` as binary_expression; the operator set
        // below is what distinguishes them from arithmetic.
        boolean_op_nodes: set(&["binary_expression"]),
        boolean_operators: set(&["&&", "||", "??"]),
        nesting_nodes: set(TS_NESTING),
        continuation_nodes: set(&["else_clause"]),
        param_containers: set(&["formal_parameters"]),
        implicit_params: set(&["this"]),
        primitive_types: set(&["string", "number", "boolean", "bigint", "symbol"]),
        string_types: set(&["string"]),
        type_annotation_nodes: set(&["type_annotation"]),
        constructor_names: set(&["constructor"]),
        assertion_nodes: HashSet::new(),
        assertion_call_prefixes: &["expect", "assert", "chai.assert"],
        declaration_nodes: set(&[
            "interface_declaration",
            "type_alias_declaration",
            "enum_declaration",
        ]),
    }
}

pub fn specs() -> &'static [LanguageSpec; 3] {
    static SPECS: OnceLock<[LanguageSpec; 3]> = OnceLock::new();
    SPECS.get_or_init(|| {
        [
            python_spec(),
            // The tsx grammar is a superset of the ts grammar and parses both; using
            // it for `.ts` too avoids a second grammar and keeps node names
            // identical across both.
            typescript_like(
                "typescript",
                &[".ts", ".tsx", ".mts", ".cts"],
                || tree_sitter_typescript::LANGUAGE_TSX.into(),
            ),
            typescript_like(
                "javascript",
                &[".js", ".jsx", ".mjs", ".cjs"],
                || tree_sitter_javascript::LANGUAGE.into(),
            ),
        ]
    })
}

/// The spec for a file extension, which must include the leading dot.
pub fn spec_for_extension(extension: &str) -> Option<&'static LanguageSpec> {
    let lowered = extension.to_ascii_lowercase();
    specs().iter().find(|spec| spec.extensions.contains(&lowered.as_str()))
}

/// The spec for a path, by its extension.
pub fn spec_for_path(path: &str) -> Option<&'static LanguageSpec> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let dot = name.rfind('.')?;
    spec_for_extension(&name[dot..])
}

pub fn spec_for_name(name: &str) -> Option<&'static LanguageSpec> {
    specs().iter().find(|spec| spec.name == name)
}

pub fn is_supported_path(path: &str) -> bool {
    spec_for_path(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_every_supported_extension() {
        assert_eq!(spec_for_path("a/b.py").unwrap().name, "python");
        assert_eq!(spec_for_path("a/b.tsx").unwrap().name, "typescript");
        assert_eq!(spec_for_path("a/b.ts").unwrap().name, "typescript");
        assert_eq!(spec_for_path("a/b.mjs").unwrap().name, "javascript");
        assert!(spec_for_path("a/b.rs").is_none());
        assert!(spec_for_path("Makefile").is_none());
    }

    #[test]
    fn every_grammar_loads() {
        for spec in specs().iter() {
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&spec.language()).expect("grammar loads");
            assert!(parser.parse("", None).is_some());
        }
    }
}
