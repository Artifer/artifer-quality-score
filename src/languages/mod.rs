//! Per-language AST vocabulary.
//!
//! Every detector is written against this declarative spec rather than against a
//! specific grammar, so adding a language means adding a `LanguageSpec` and no
//! detector changes.
//!
//! The node-type names here were probed empirically against the pinned grammar
//! versions in `Cargo.toml` -- see `examples/probe_rust.rs`, `probe_go.rs` and
//! `probe_ruby.rs` for how -- and the four the Python reference implementation also had
//! match it exactly. Go and Ruby came after the reference was retired.
//!
//! One grammar-level trap the probes exist to catch: tree-sitter-ruby names an
//! anonymous keyword token after the node it opens, so an `if` node *contains* an `if`
//! token. Every match of a node against the vocabulary below therefore goes through
//! `metrics::is_kind`, which checks that the node is a named one.

use std::collections::HashSet;
use std::sync::OnceLock;
use tree_sitter::{Language, Node};

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
    /// Nodes whose named children are a sequence of statements. The rules about *runs*
    /// of statements -- duplicated blocks, walls of assertions -- read this to know
    /// where a sequence lives. Go needs both `block` and the `statement_list` it wraps
    /// them in.
    pub statement_containers: HashSet<&'static str>,

    /// Each occurrence adds 1 to cyclomatic complexity.
    pub decision_nodes: HashSet<&'static str>,
    /// A decision node that is not really a decision, where node kind alone cannot say
    /// so. Go's `if err != nil { return .. }` is Rust's `?` with the sugar removed, and
    /// `?` is already not counted; Java writes `case` and `default` with the same node,
    /// and a default arm is the absence of a decision.
    pub linear_branch: Option<fn(&Node, &[u8]) -> bool>,
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
    /// Fields whose child continues an existing branch, for grammars that give an
    /// `else if` no clause node of its own. Go hangs the second `if` straight off the
    /// first one's `alternative` field, so node type alone cannot tell a continuation
    /// from a genuinely nested `if`.
    pub continuation_fields: &'static [&'static str],

    /// Nodes holding a callable's parameter list.
    pub param_containers: HashSet<&'static str>,
    /// Field holding the parameter list, where one is not enough to identify it. A Go
    /// `method_declaration` carries three `parameter_list` children -- the receiver,
    /// the parameters and the results -- so taking the first one would measure the
    /// receiver and call it the signature.
    pub param_field: Option<&'static str>,
    /// Implicit receiver parameters that should not count against a signature.
    pub implicit_params: HashSet<&'static str>,
    /// Node kinds that *are* an implicit receiver. Rust models `&self` as its own
    /// `self_parameter` node rather than a parameter whose text happens to be "self".
    pub implicit_param_nodes: HashSet<&'static str>,
    /// Whether a function is a namespace function rather than a method: it holds no
    /// instance state, so LCOM4 must not count it as a responsibility. The same
    /// reasoning excludes a Python `@staticmethod` and a Rust associated function, which
    /// `explicit_receiver` handles instead. The *form* differs per language -- Ruby says
    /// it with a node kind (`def self.parse`), Java with a `static` modifier -- which is
    /// why this is a predicate and not a set of kinds.
    pub namespace_function: Option<fn(&Node, &[u8]) -> bool>,

    /// Whether a method must name its receiver. Python and Rust do, so a function
    /// without one is an associated/static function and holds no instance state —
    /// which LCOM4 must not count as a responsibility. TS and JS leave `this`
    /// implicit, so absence proves nothing there.
    pub explicit_receiver: bool,

    /// Whether a method calls a sibling method by bare name, with no receiver. Ruby
    /// does; everywhere else a self-call is written `self.x` / `this.x` and is already
    /// indistinguishable from a field read.
    pub implicit_self_calls: bool,
    /// Node kinds that *declare* an instance field. Java and Ruby both read a field by
    /// bare name -- `amount`, not `this.amount` -- which is indistinguishable from a
    /// local until you know what the class declared. See `instance_field_nodes` for the
    /// other half.
    pub field_declaration_nodes: HashSet<&'static str>,
    /// Node kinds that are themselves a reference to instance state. Ruby writes a
    /// field as `@amount` -- one node, no receiver -- where every other language here
    /// writes `self.amount` as a receiver plus an attribute.
    pub instance_field_nodes: HashSet<&'static str>,

    /// Type names treated as primitives for Primitive Obsession.
    pub primitive_types: HashSet<&'static str>,
    /// Type names treated as strings for String Heavy Function Arguments.
    pub string_types: HashSet<&'static str>,

    /// Node types carrying a type annotation.
    pub type_annotation_nodes: HashSet<&'static str>,

    /// Node kinds that *are* a constructor. Java names one after its class, so there is
    /// no fixed name to look for -- only a node kind.
    pub constructor_nodes: HashSet<&'static str>,
    /// Name of a constructor / initialiser, for Constructor Over-Injection.
    pub constructor_names: HashSet<&'static str>,
    /// Prefixes that name a constructor by convention, where the language has no
    /// dedicated one. Go writes `NewServer(..)`; the prefix only counts when what
    /// follows is capitalised, so `NewlineCount` stays an ordinary function.
    pub constructor_prefixes: &'static [&'static str],

    /// Statement nodes that are assertions.
    pub assertion_nodes: HashSet<&'static str>,
    /// Call-callee prefixes that mark an assertion (e.g. `expect`, `assert*`).
    pub assertion_call_prefixes: &'static [&'static str],

    /// Nodes that declare types rather than behaviour (Lines of Declarations).
    pub declaration_nodes: HashSet<&'static str>,
    /// Field marking a class-like node as implementing an external interface, if the
    /// language has one. Rust's `impl Trait for Type` sets `trait`; the methods are
    /// dictated by the trait, so cohesion rules must not judge the grouping.
    pub external_interface_field: Option<&'static str>,
}

impl LanguageSpec {
    pub fn language(&self) -> Language {
        (self.language)()
    }
}

fn set(items: &[&'static str]) -> HashSet<&'static str> {
    items.iter().copied().collect()
}

mod go;
mod java;
mod php;
mod python;
mod ruby;
mod rust;
mod typescript;

pub fn specs() -> &'static [LanguageSpec; 8] {
    static SPECS: OnceLock<[LanguageSpec; 8]> = OnceLock::new();
    SPECS.get_or_init(|| {
        [
            python::spec(),
            typescript::typescript(),
            typescript::javascript(),
            rust::spec(),
            go::spec(),
            ruby::spec(),
            java::spec(),
            php::spec(),
        ]
    })
}

/// The spec for a file extension, which must include the leading dot.
pub fn spec_for_extension(extension: &str) -> Option<&'static LanguageSpec> {
    let lowered = extension.to_ascii_lowercase();
    specs()
        .iter()
        .find(|spec| spec.extensions.contains(&lowered.as_str()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_every_supported_extension() {
        assert_eq!(spec_for_path("a/b.py").unwrap().name, "python");
        assert_eq!(spec_for_path("a/b.tsx").unwrap().name, "typescript");
        assert_eq!(spec_for_path("a/b.ts").unwrap().name, "typescript");
        assert_eq!(spec_for_path("a/b.mjs").unwrap().name, "javascript");
        assert_eq!(spec_for_path("a/b.vue").unwrap().name, "javascript");
        assert_eq!(spec_for_path("a/b.rs").unwrap().name, "rust");
        assert_eq!(spec_for_path("a/b.go").unwrap().name, "go");
        assert_eq!(spec_for_path("a/b.rb").unwrap().name, "ruby");
        assert_eq!(spec_for_path("a/b.rake").unwrap().name, "ruby");
        assert_eq!(spec_for_path("a/b.java").unwrap().name, "java");
        assert_eq!(spec_for_path("a/b.php").unwrap().name, "php");
        assert!(spec_for_path("a/b.txt").is_none());
        assert!(spec_for_path("Makefile").is_none());
    }

    #[test]
    fn every_grammar_loads() {
        for spec in specs().iter() {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&spec.language())
                .expect("grammar loads");
            assert!(parser.parse("", None).is_some());
        }
    }
}
