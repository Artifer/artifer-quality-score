"""AST measurement primitives shared by the detectors.

Two conventions here shape every metric downstream, so they are worth stating
plainly:

**Named nested functions are separate units; anonymous ones are not.** A named
nested helper (`def helper()`, `const load = () => {}`) is measured on its own
and excluded from its parent. An anonymous inline callback (`items.map(x => ...)`,
`key=lambda r: r.id`) is measured *as part of* the enclosing function, because
that is where its complexity actually lands when you read the code.

**Comments and docstrings never count as LoC.** Otherwise the fastest way to
improve a score is to delete the documentation, and a metric an LLM optimises
against must not reward that.
"""

from __future__ import annotations

from dataclasses import dataclass

from tree_sitter import Node

from .languages import LanguageSpec
from .models import Range

_COMMENT_NODES = frozenset({"comment", "block_comment", "line_comment"})
#: Parent node types from which an otherwise-anonymous function takes its name.
_NAMING_PARENTS = frozenset(
    {
        "variable_declarator",
        "assignment",
        "pair",
        "public_field_definition",
        "field_definition",
    }
)


def to_range(node: Node) -> Range:
    return Range(
        start_line=node.start_point[0] + 1,
        start_column=node.start_point[1] + 1,
        end_line=node.end_point[0] + 1,
        end_column=node.end_point[1] + 1,
    )


def is_docstring(node: Node) -> bool:
    """A bare string statement, i.e. a Python docstring or a JS directive."""
    if node.type != "expression_statement" or node.named_child_count != 1:
        return False
    return node.named_children[0].type in {"string", "template_string"}


def logical_lines(node: Node) -> int:
    """Non-blank, non-comment, non-docstring lines spanned by `node`."""
    lines: set[int] = set()

    def visit(current: Node) -> None:
        if current.type in _COMMENT_NODES or is_docstring(current):
            return
        if current.child_count == 0:
            for line in range(current.start_point[0], current.end_point[0] + 1):
                lines.add(line)
            return
        for child in current.children:
            visit(child)

    visit(node)
    return len(lines)


def function_name(node: Node, spec: LanguageSpec) -> str | None:
    """The function's name, or None if it is a genuinely anonymous callback."""
    for field in spec.name_fields:
        named = node.child_by_field_name(field)
        if named is not None:
            return named.text.decode("utf-8", "replace")

    # `constructor(...)`, `get foo()` and friends carry a property_identifier.
    if node.type == "method_definition":
        for child in node.named_children:
            if child.type in {"property_identifier", "private_property_identifier"}:
                return child.text.decode("utf-8", "replace")

    # `const load = async () => {}` is a named unit even though the arrow is not.
    parent = node.parent
    if parent is not None and parent.type in _NAMING_PARENTS:
        for field in ("name", "left", "key"):
            named = parent.child_by_field_name(field)
            if named is not None:
                return named.text.decode("utf-8", "replace")
    return None


def is_scope_boundary(node: Node, spec: LanguageSpec) -> bool:
    """Whether `node` is a function measured as its own unit."""
    return node.type in spec.function_nodes and function_name(node, spec) is not None


def iter_functions(root: Node, spec: LanguageSpec) -> list[Node]:
    """Every named function in the tree, outermost first."""
    found: list[Node] = []

    def visit(node: Node) -> None:
        for child in node.children:
            if is_scope_boundary(child, spec):
                found.append(child)
            visit(child)

    visit(root)
    return found


def iter_own_nodes(root: Node, spec: LanguageSpec):
    """Walk `root`, not descending into nested named functions.

    `root` itself is yielded even when it is a function, so this reads as "the
    nodes belonging to this unit".
    """
    yield root
    stack = list(reversed(root.children))
    while stack:
        node = stack.pop()
        if is_scope_boundary(node, spec):
            continue
        yield node
        stack.extend(reversed(node.children))


def iter_all_nodes(root: Node):
    """Walk the whole subtree, including nested named functions.

    Use this for rules that ask "anywhere in this file" (classes, assertions,
    declarations). Use `iter_own_nodes` for per-function metrics, which must not
    absorb a nested helper's complexity.
    """
    stack = [root]
    while stack:
        node = stack.pop()
        yield node
        stack.extend(reversed(node.children))


def _is_boolean_operator(node: Node, spec: LanguageSpec) -> bool:
    if node.type not in spec.boolean_op_nodes:
        return False
    operator = node.child_by_field_name("operator")
    if operator is None:
        # Python models `and`/`or` as distinct node types already.
        return True
    return operator.text.decode("utf-8", "replace") in spec.boolean_operators


def cyclomatic_complexity(node: Node, spec: LanguageSpec) -> int:
    """McCabe complexity: one, plus one per decision point."""
    complexity = 1
    for current in iter_own_nodes(node, spec):
        if current is node:
            continue
        if current.type in spec.decision_nodes or _is_boolean_operator(current, spec):
            complexity += 1
    return complexity


def _continues_branch(node: Node, spec: LanguageSpec) -> bool:
    """True for the `if` of an `else if`, which reads flat, not nested."""
    parent = node.parent
    return parent is not None and parent.type in spec.continuation_nodes


def nesting_depth(node: Node, spec: LanguageSpec) -> int:
    """Deepest run of nested control-flow constructs inside `node`."""

    def visit(current: Node, depth: int) -> int:
        deepest = depth
        for child in current.children:
            if is_scope_boundary(child, spec):
                continue
            child_depth = depth
            if child.type in spec.nesting_nodes and not _continues_branch(child, spec):
                child_depth = depth + 1
            deepest = max(deepest, visit(child, child_depth))
        return deepest

    return visit(node, 0)


def function_body(node: Node, spec: LanguageSpec) -> Node | None:
    body = node.child_by_field_name("body")
    if body is not None:
        return body
    for child in reversed(node.named_children):
        if child.type in {"block", "statement_block"}:
            return child
    return None


@dataclass(frozen=True)
class Bump:
    """A self-contained chunk of nested logic inside a function."""

    node: Node
    depth: int


def bumps(node: Node, spec: LanguageSpec, min_depth: int) -> list[Bump]:
    """Top-level statements of `node` that each hide their own nested logic.

    A function with several of these is a "bumpy road": instead of one coherent
    level of abstraction it strings together multiple lumps of nested detail,
    each of which wants to be its own function.
    """
    body = function_body(node, spec)
    if body is None:
        return []
    found: list[Bump] = []
    for statement in body.named_children:
        if statement.type in _COMMENT_NODES:
            continue
        depth = nesting_depth(statement, spec)
        if statement.type in spec.nesting_nodes:
            depth += 1
        if depth >= min_depth:
            found.append(Bump(node=statement, depth=depth))
    return found


def _condition_nodes(node: Node, spec: LanguageSpec) -> list[Node]:
    conditions: list[Node] = []
    for current in iter_own_nodes(node, spec):
        if current.type not in spec.decision_nodes:
            continue
        condition = current.child_by_field_name("condition")
        if condition is not None:
            conditions.append(condition)
    return conditions


def count_boolean_operators(node: Node, spec: LanguageSpec) -> int:
    return sum(
        1
        for current in iter_own_nodes(node, spec)
        if _is_boolean_operator(current, spec)
    )


def complex_conditionals(
    node: Node, spec: LanguageSpec, min_operators: int
) -> list[tuple[Node, int]]:
    """Branch conditions combining at least `min_operators` boolean operators."""
    found: list[tuple[Node, int]] = []
    for condition in _condition_nodes(node, spec):
        operators = count_boolean_operators(condition, spec)
        if operators >= min_operators:
            found.append((condition, operators))
    return found


def parameters(node: Node, spec: LanguageSpec) -> list[Node]:
    """Declared parameters, excluding implicit receivers like `self`."""
    container = None
    for child in node.named_children:
        if child.type in spec.param_containers:
            container = child
            break
    if container is None:
        return []

    params: list[Node] = []
    for param in container.named_children:
        if param.type in _COMMENT_NODES:
            continue
        text = param.text.decode("utf-8", "replace").strip()
        if text in spec.implicit_params:
            continue
        params.append(param)
    return params


def parameter_type(param: Node, spec: LanguageSpec) -> str | None:
    """The annotated type of a parameter, when it has one."""
    for current in param.named_children:
        if current.type in spec.type_annotation_nodes:
            text = current.text.decode("utf-8", "replace")
            return text.lstrip(":").strip()
        # Python's typed_parameter nests the annotation under a `type` node.
        if current.type == "type":
            return current.text.decode("utf-8", "replace").strip()
    annotation = param.child_by_field_name("type")
    if annotation is not None:
        return annotation.text.decode("utf-8", "replace").lstrip(":").strip()
    return None


def is_primitive_type(type_name: str | None, spec: LanguageSpec) -> bool:
    if not type_name:
        return False
    bare = type_name.strip().rstrip("?").strip()
    if bare.endswith("| None"):
        bare = bare[: -len("| None")].strip()
    return bare in spec.primitive_types


def is_string_type(type_name: str | None, spec: LanguageSpec) -> bool:
    if not type_name:
        return False
    bare = type_name.strip().rstrip("?").strip()
    return bare in spec.string_types


def class_nodes(root: Node, spec: LanguageSpec) -> list[Node]:
    return [
        node
        for node in iter_all_nodes(root)
        if node.type in spec.class_nodes and node is not root
    ]


#: Base classes that mark a type as an interface rather than an implementation.
_ABSTRACT_BASES = frozenset({"ABC", "ABCMeta", "Protocol", "Interface"})
#: Decorators that mark a method as having no implementation here.
_ABSTRACT_DECORATORS = ("abstractmethod", "abstractproperty", "abc.abstract")


def is_abstract_class(node: Node, spec: LanguageSpec) -> bool:
    """Whether a class is an interface — abstract, Protocol, or all-stubs.

    Interfaces have no shared state and no internal calls by definition, so
    cohesion metrics always condemn them. Flagging an ABC for "low cohesion"
    would be telling the author to stop declaring interfaces.
    """
    if "abstract" in node.type:
        return True

    # An `abstract` modifier on a TS class declaration.
    previous = node.prev_sibling
    if previous is not None and previous.type == "abstract":
        return True

    superclasses = node.child_by_field_name("superclasses")
    if superclasses is not None:
        for child in superclasses.named_children:
            base = child.text.decode("utf-8", "replace").split("[")[0]
            if base.split(".")[-1] in _ABSTRACT_BASES:
                return True

    for current in iter_all_nodes(node):
        if current.type not in {"decorator", "decorator_list"}:
            continue
        text = current.text.decode("utf-8", "replace")
        if any(marker in text for marker in _ABSTRACT_DECORATORS):
            return True

    body = node.child_by_field_name("body")
    if body is None:
        return False
    methods = [
        child
        for child in iter_all_nodes(body)
        if child.type in spec.function_nodes and function_name(child, spec)
    ]
    return bool(methods) and all(_has_stub_body(method, spec) for method in methods)


def _has_stub_body(node: Node, spec: LanguageSpec) -> bool:
    """A body that is only a docstring, `pass`, `...`, or NotImplementedError."""
    body = function_body(node, spec)
    if body is None:
        return False
    for statement in body.named_children:
        if statement.type in _COMMENT_NODES or is_docstring(statement):
            continue
        if statement.type in {"pass_statement", "empty_statement"}:
            continue
        text = statement.text.decode("utf-8", "replace").strip()
        if text in {"...", "...;"} or "NotImplementedError" in text:
            continue
        return False
    return True


def is_assertion(node: Node, spec: LanguageSpec) -> bool:
    """Whether a statement asserts something, in either assertion dialect."""
    if node.type in spec.assertion_nodes:
        return True
    if node.type != "expression_statement" or node.named_child_count == 0:
        return False
    expression = node.named_children[0]
    if expression.type not in {"call_expression", "call", "await_expression"}:
        return False
    text = expression.text.decode("utf-8", "replace")
    # `removeprefix`, not `lstrip("await ")` — the latter strips any leading
    # character from the set {a,w,i,t,space}, which silently mangles
    # `assertEqual(...)` into `ssertEqual(...)` and loses the match.
    text = text.removeprefix("await ").lstrip()
    return any(text.startswith(prefix) for prefix in spec.assertion_call_prefixes)


def normalised_tokens(node: Node, spec: LanguageSpec) -> tuple[str, ...]:
    """Structure-only token sequence, for similarity comparison.

    Identifiers and literals collapse to their node type, so two blocks that
    differ only in names and constants compare as identical. Comments are
    dropped.
    """
    tokens: list[str] = []
    for current in iter_own_nodes(node, spec):
        if current.child_count != 0 or current.type in _COMMENT_NODES:
            continue
        if current.type in {"identifier", "property_identifier", "type_identifier"}:
            tokens.append("<id>")
        elif current.type in {
            "string",
            "string_fragment",
            "integer",
            "float",
            "number",
            "true",
            "false",
            "none",
            "null",
        }:
            tokens.append("<lit>")
        else:
            tokens.append(current.type)
    return tuple(tokens)
