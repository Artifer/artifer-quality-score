//! AST measurement primitives shared by the detectors.
//!
//! Two conventions here shape every metric downstream, so they are worth stating
//! plainly:
//!
//! **Named nested functions are separate units; anonymous ones are not.** A named
//! nested helper (`def helper()`, `const load = () => {}`) is measured on its own and
//! excluded from its parent. An anonymous inline callback (`items.map(x => ...)`,
//! `key=lambda r: r.id`) is measured *as part of* the enclosing function, because
//! that is where its complexity actually lands when you read the code.
//!
//! **Comments and docstrings never count as LoC.** Otherwise the fastest way to
//! improve a score is to delete the documentation, and a metric an LLM optimises
//! against must not reward that.
//!
//! Traversal order matches the Python reference implementation exactly (pre-order,
//! document order). It has to: the per-rule cap allocation in `scoring` breaks ties
//! by discovery order, so a different walk order would produce different penalties.

use crate::languages::LanguageSpec;
use crate::models::Range;
use std::collections::HashSet;
use tree_sitter::Node;

pub const COMMENT_NODES: &[&str] = &["comment", "block_comment", "line_comment"];

/// Parent node types from which an otherwise-anonymous function takes its name.
const NAMING_PARENTS: &[&str] = &[
    "variable_declarator",
    "assignment",
    "pair",
    "public_field_definition",
    "field_definition",
    // Rust binds a closure with `let name = |..| ..`, naming it via `pattern`.
    "let_declaration",
];

/// Base classes that mark a type as an interface rather than an implementation.
const ABSTRACT_BASES: &[&str] = &["ABC", "ABCMeta", "Protocol", "Interface"];

/// Decorators that mark a method as having no implementation here.
const ABSTRACT_DECORATORS: &[&str] = &["abstractmethod", "abstractproperty", "abc.abstract"];

const LITERAL_NODES: &[&str] = &[
    "string",
    "string_fragment",
    "integer",
    "float",
    "number",
    "true",
    "false",
    "none",
    "null",
];

const IDENTIFIER_NODES: &[&str] = &["identifier", "property_identifier", "type_identifier"];

fn is_comment(node: &Node) -> bool {
    COMMENT_NODES.contains(&node.kind())
}

pub fn text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

pub fn to_range(node: &Node) -> Range {
    Range {
        start_line: node.start_position().row + 1,
        start_column: node.start_position().column + 1,
        end_line: node.end_position().row + 1,
        end_column: node.end_position().column + 1,
    }
}

/// All children, including anonymous ones, in document order.
fn children<'tree>(node: &Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// Named children only, in document order.
pub fn named_children<'tree>(node: &Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// A bare string statement, i.e. a Python docstring or a JS directive.
pub fn is_docstring(node: &Node) -> bool {
    if node.kind() != "expression_statement" || node.named_child_count() != 1 {
        return false;
    }
    let inner = node.named_child(0).expect("named_child_count checked");
    matches!(inner.kind(), "string" | "template_string")
}

/// Non-blank, non-comment, non-docstring lines spanned by `node`.
pub fn logical_lines(node: &Node) -> usize {
    let mut lines: HashSet<usize> = HashSet::new();
    let mut stack = vec![*node];
    while let Some(current) = stack.pop() {
        if is_comment(&current) || is_docstring(&current) {
            continue;
        }
        if current.child_count() == 0 {
            for line in current.start_position().row..=current.end_position().row {
                lines.insert(line);
            }
            continue;
        }
        stack.extend(children(&current));
    }
    lines.len()
}

/// The function's name, or None if it is a genuinely anonymous callback.
pub fn function_name(node: &Node, source: &[u8]) -> Option<String> {
    if let Some(named) = node.child_by_field_name("name") {
        return Some(text(&named, source).to_string());
    }

    // `constructor(...)`, `get foo()` and friends carry a property_identifier.
    if node.kind() == "method_definition" {
        for child in named_children(node) {
            if matches!(child.kind(), "property_identifier" | "private_property_identifier") {
                return Some(text(&child, source).to_string());
            }
        }
    }

    // `const load = async () => {}` is a named unit even though the arrow is not.
    let parent = node.parent()?;
    if NAMING_PARENTS.contains(&parent.kind()) {
        for field in ["name", "left", "key", "pattern"] {
            if let Some(named) = parent.child_by_field_name(field) {
                return Some(text(&named, source).to_string());
            }
        }
    }
    None
}

/// Whether `node` is a function measured as its own unit.
pub fn is_scope_boundary(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    spec.function_nodes.contains(node.kind()) && function_name(node, source).is_some()
}

/// Every named function in the tree, outermost first.
pub fn iter_functions<'tree>(
    root: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
) -> Vec<Node<'tree>> {
    let mut found = Vec::new();
    collect_functions(root, spec, source, &mut found);
    found
}

fn collect_functions<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
    found: &mut Vec<Node<'tree>>,
) {
    for child in children(node) {
        if is_scope_boundary(&child, spec, source) {
            found.push(child);
        }
        collect_functions(&child, spec, source, found);
    }
}

/// Walk `root`, not descending into nested named functions.
///
/// `root` itself is included even when it is a function, so this reads as "the nodes
/// belonging to this unit".
pub fn own_nodes<'tree>(
    root: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
) -> Vec<Node<'tree>> {
    let mut out = vec![*root];
    let mut stack: Vec<Node<'tree>> = children(root).into_iter().rev().collect();
    while let Some(node) = stack.pop() {
        if is_scope_boundary(&node, spec, source) {
            continue;
        }
        out.push(node);
        stack.extend(children(&node).into_iter().rev());
    }
    out
}

/// Walk the whole subtree, including nested named functions.
///
/// Use this for rules that ask "anywhere in this file" (classes, assertions,
/// declarations). Use `own_nodes` for per-function metrics, which must not absorb a
/// nested helper's complexity.
pub fn all_nodes<'tree>(root: &Node<'tree>) -> Vec<Node<'tree>> {
    let mut out = Vec::new();
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        out.push(node);
        stack.extend(children(&node).into_iter().rev());
    }
    out
}

fn is_boolean_operator(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    if !spec.boolean_op_nodes.contains(node.kind()) {
        return false;
    }
    match node.child_by_field_name("operator") {
        // Python models `and`/`or` as distinct node types already.
        None => true,
        Some(operator) => spec.boolean_operators.contains(text(&operator, source)),
    }
}

/// McCabe complexity: one, plus one per decision point.
pub fn cyclomatic_complexity(node: &Node, spec: &LanguageSpec, source: &[u8]) -> usize {
    let mut complexity = 1;
    for current in own_nodes(node, spec, source) {
        if current.id() == node.id() {
            continue;
        }
        if spec.decision_nodes.contains(current.kind())
            || is_boolean_operator(&current, spec, source)
        {
            complexity += 1;
        }
    }
    complexity
}

/// True for the `if` of an `else if`, which reads flat, not nested.
fn continues_branch(node: &Node, spec: &LanguageSpec) -> bool {
    node.parent()
        .map(|parent| spec.continuation_nodes.contains(parent.kind()))
        .unwrap_or(false)
}

/// Deepest run of nested control-flow constructs inside `node`.
pub fn nesting_depth(node: &Node, spec: &LanguageSpec, source: &[u8]) -> usize {
    fn visit(current: &Node, depth: usize, spec: &LanguageSpec, source: &[u8]) -> usize {
        let mut deepest = depth;
        for child in children(current) {
            if is_scope_boundary(&child, spec, source) {
                continue;
            }
            let mut child_depth = depth;
            if spec.nesting_nodes.contains(child.kind()) && !continues_branch(&child, spec) {
                child_depth = depth + 1;
            }
            deepest = deepest.max(visit(&child, child_depth, spec, source));
        }
        deepest
    }
    visit(node, 0, spec, source)
}

pub fn function_body<'tree>(node: &Node<'tree>, _spec: &LanguageSpec) -> Option<Node<'tree>> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }
    named_children(node)
        .into_iter()
        .rev()
        .find(|child| matches!(child.kind(), "block" | "statement_block"))
}

/// A self-contained chunk of nested logic inside a function.
pub struct Bump {
    pub depth: usize,
}

/// Top-level statements of `node` that each hide their own nested logic.
///
/// A function with several of these is a "bumpy road": instead of one coherent level
/// of abstraction it strings together multiple lumps of nested detail, each of which
/// wants to be its own function.
pub fn bumps(node: &Node, spec: &LanguageSpec, source: &[u8], min_depth: usize) -> Vec<Bump> {
    let Some(body) = function_body(node, spec) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for statement in named_children(&body) {
        if is_comment(&statement) {
            continue;
        }
        let mut depth = nesting_depth(&statement, spec, source);
        if spec.nesting_nodes.contains(statement.kind()) {
            depth += 1;
        }
        if depth >= min_depth {
            found.push(Bump { depth });
        }
    }
    found
}

fn condition_nodes<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
) -> Vec<Node<'tree>> {
    let mut conditions = Vec::new();
    for current in own_nodes(node, spec, source) {
        if !spec.decision_nodes.contains(current.kind()) {
            continue;
        }
        if let Some(condition) = current.child_by_field_name("condition") {
            conditions.push(condition);
        }
    }
    conditions
}

pub fn count_boolean_operators(node: &Node, spec: &LanguageSpec, source: &[u8]) -> usize {
    own_nodes(node, spec, source)
        .iter()
        .filter(|current| is_boolean_operator(current, spec, source))
        .count()
}

/// Branch conditions combining at least `min_operators` boolean operators.
pub fn complex_conditionals<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
    min_operators: usize,
) -> Vec<(Node<'tree>, usize)> {
    let mut found = Vec::new();
    for condition in condition_nodes(node, spec, source) {
        let operators = count_boolean_operators(&condition, spec, source);
        if operators >= min_operators {
            found.push((condition, operators));
        }
    }
    found
}

/// Declared parameters, excluding implicit receivers like `self`.
pub fn parameters<'tree>(
    node: &Node<'tree>,
    spec: &LanguageSpec,
    source: &[u8],
) -> Vec<Node<'tree>> {
    let container = named_children(node)
        .into_iter()
        .find(|child| spec.param_containers.contains(child.kind()));
    let Some(container) = container else {
        return Vec::new();
    };

    let mut params = Vec::new();
    for param in named_children(&container) {
        if is_comment(&param) {
            continue;
        }
        // Rust models `&self` as its own node kind rather than a parameter whose text
        // happens to read "self", so both forms have to be recognised.
        if spec.implicit_param_nodes.contains(param.kind()) {
            continue;
        }
        if spec.implicit_params.contains(text(&param, source).trim()) {
            continue;
        }
        params.push(param);
    }
    params
}

/// The annotated type of a parameter, when it has one.
pub fn parameter_type(param: &Node, spec: &LanguageSpec, source: &[u8]) -> Option<String> {
    for current in named_children(param) {
        if spec.type_annotation_nodes.contains(current.kind()) {
            return Some(text(&current, source).trim_start_matches(':').trim().to_string());
        }
        // Python's typed_parameter nests the annotation under a `type` node.
        if current.kind() == "type" {
            return Some(text(&current, source).trim().to_string());
        }
    }
    param
        .child_by_field_name("type")
        .map(|annotation| text(&annotation, source).trim_start_matches(':').trim().to_string())
}

fn bare_type(type_name: &str) -> &str {
    let trimmed = type_name.trim().trim_end_matches('?').trim();
    trimmed.strip_suffix("| None").map(str::trim).unwrap_or(trimmed)
}

pub fn is_primitive_type(type_name: Option<&str>, spec: &LanguageSpec) -> bool {
    match type_name {
        None => false,
        Some(name) if name.is_empty() => false,
        Some(name) => spec.primitive_types.contains(bare_type(name)),
    }
}

pub fn is_string_type(type_name: Option<&str>, spec: &LanguageSpec) -> bool {
    match type_name {
        None => false,
        Some(name) if name.is_empty() => false,
        Some(name) => {
            let trimmed = name.trim().trim_end_matches('?').trim();
            spec.string_types.contains(trimmed)
        }
    }
}

pub fn class_nodes<'tree>(root: &Node<'tree>, spec: &LanguageSpec) -> Vec<Node<'tree>> {
    all_nodes(root)
        .into_iter()
        .filter(|node| spec.class_nodes.contains(node.kind()) && node.id() != root.id())
        .collect()
}

/// A body that is only a docstring, `pass`, `...`, or NotImplementedError.
fn has_stub_body(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    let Some(body) = function_body(node, spec) else {
        return false;
    };
    for statement in named_children(&body) {
        if is_comment(&statement) || is_docstring(&statement) {
            continue;
        }
        if matches!(statement.kind(), "pass_statement" | "empty_statement") {
            continue;
        }
        let body_text = text(&statement, source).trim();
        if body_text == "..." || body_text == "...;" || body_text.contains("NotImplementedError") {
            continue;
        }
        return false;
    }
    true
}

/// The identifying name of a class-like node.
///
/// Rust `impl` blocks have no `name`; their identity is the `type` they implement.
pub fn class_name(node: &Node, source: &[u8]) -> Option<String> {
    if let Some(name) = function_name(node, source) {
        return Some(name);
    }
    node.child_by_field_name("type")
        .map(|t| text(&t, source).to_string())
}

/// Whether a method is a pure projection of stored state.
///
/// One statement, no branching, **no calls at all**, and it touches a field of `self`:
/// `fn threshold(&self) -> f64 { self.thresholds[key] }`, `def get(self): return
/// super().get()`, `return self.amount * 2`.
///
/// LCOM4 counts every method touching a distinct field as its own component, so a
/// value object with six accessors scores 6 and reads as six unrelated jobs. Splitting
/// it would be absurd, so accessors are excluded like constructors and abstract stubs.
///
/// The "no calls" condition is what keeps this narrow, and it is load-bearing.
/// `self.orders.append(o)` and `len(self.orders)` both call something, so a class that
/// merely appends to two different collections is still correctly condemned — that is
/// real low cohesion, not an accessor.
pub fn is_pure_accessor(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    let Some(body) = function_body(node, spec) else {
        return false;
    };
    let mut only: Option<Node> = None;
    for statement in named_children(&body) {
        if is_comment(&statement) || is_docstring(&statement) {
            continue;
        }
        if only.is_some() {
            return false; // more than one meaningful statement
        }
        only = Some(statement);
    }
    let Some(statement) = only else {
        return false; // an empty body is a stub, handled by `has_stub_body`
    };

    let mut touches_state = false;
    for current in own_nodes(&statement, spec, source) {
        let kind = current.kind();
        if spec.nesting_nodes.contains(kind) || spec.decision_nodes.contains(kind) {
            return false;
        }
        if matches!(
            kind,
            "call" | "call_expression" | "macro_invocation" | "await_expression" | "await"
        ) {
            // `super().get(...)` is the one call that is still a pure delegation.
            let text_body = text(&current, source);
            if !(text_body.contains("super()") || text_body.contains("super.")) {
                return false;
            }
            touches_state = true;
        }
        if matches!(kind, "attribute" | "member_expression" | "field_expression") {
            touches_state = true;
        }
    }
    touches_state
}

/// Whether a class is an interface — abstract, Protocol, or all-stubs.
///
/// Interfaces have no shared state and no internal calls by definition, so cohesion
/// metrics always condemn them. Flagging an ABC for "low cohesion" would be telling
/// the author to stop declaring interfaces.
pub fn is_abstract_class(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    if node.kind().contains("abstract") {
        return true;
    }
    // `impl Trait for Type`: the method set is dictated by the trait, so grouping is
    // not a choice the author made and cohesion has nothing to say about it. Same
    // reasoning as the exemptions for abstract bases and super()-only overrides.
    if let Some(field) = spec.external_interface_field {
        if node.child_by_field_name(field).is_some() {
            return true;
        }
    }
    // An `abstract` modifier on a TS class declaration.
    if node.prev_sibling().map(|p| p.kind() == "abstract").unwrap_or(false) {
        return true;
    }

    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        for child in named_children(&superclasses) {
            let raw = text(&child, source);
            let base = raw.split('[').next().unwrap_or(raw);
            let leaf = base.rsplit('.').next().unwrap_or(base);
            if ABSTRACT_BASES.contains(&leaf) {
                return true;
            }
        }
    }

    for current in all_nodes(node) {
        if !matches!(current.kind(), "decorator" | "decorator_list") {
            continue;
        }
        let decorator = text(&current, source);
        if ABSTRACT_DECORATORS.iter().any(|marker| decorator.contains(marker)) {
            return true;
        }
    }

    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let methods: Vec<Node> = all_nodes(&body)
        .into_iter()
        .filter(|child| {
            spec.function_nodes.contains(child.kind()) && function_name(child, source).is_some()
        })
        .collect();
    !methods.is_empty() && methods.iter().all(|m| has_stub_body(m, spec, source))
}

/// Whether a statement asserts something, in either assertion dialect.
pub fn is_assertion(node: &Node, spec: &LanguageSpec, source: &[u8]) -> bool {
    if spec.assertion_nodes.contains(node.kind()) {
        return true;
    }
    if node.kind() != "expression_statement" || node.named_child_count() == 0 {
        return false;
    }
    let expression = node.named_child(0).expect("named_child_count checked");
    // `macro_invocation` covers Rust's assert!/assert_eq!/debug_assert! family.
    if !matches!(
        expression.kind(),
        "call_expression" | "call" | "await_expression" | "macro_invocation"
    ) {
        return false;
    }
    let raw = text(&expression, source);
    let stripped = raw.strip_prefix("await ").unwrap_or(raw).trim_start();
    spec.assertion_call_prefixes.iter().any(|prefix| stripped.starts_with(prefix))
}

/// Structure-only token sequence, for similarity comparison.
///
/// Identifiers and literals collapse to their node type, so two blocks that differ
/// only in names and constants compare as identical. Comments are dropped.
pub fn normalised_tokens(node: &Node, spec: &LanguageSpec, source: &[u8]) -> Vec<&'static str> {
    let mut tokens = Vec::new();
    for current in own_nodes(node, spec, source) {
        if current.child_count() != 0 || is_comment(&current) {
            continue;
        }
        let kind = current.kind();
        if IDENTIFIER_NODES.contains(&kind) {
            tokens.push("<id>");
        } else if LITERAL_NODES.contains(&kind) {
            tokens.push("<lit>");
        } else {
            tokens.push(kind);
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
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
