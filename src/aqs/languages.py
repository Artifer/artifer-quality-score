"""Per-language AST vocabulary.

Every detector is written against this declarative spec rather than against a
specific grammar, so adding a language means adding a ``LanguageSpec`` and no
detector changes.

The node-type names here were probed empirically against the pinned grammar
versions (see ``pyproject.toml``). They are grammar-version-sensitive: bumping a
``tree-sitter-*`` pin can change node names and therefore change scores, which
is exactly why the pins are exact rather than ranges.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from functools import lru_cache
from typing import Callable

import tree_sitter_javascript
import tree_sitter_python
import tree_sitter_typescript
from tree_sitter import Language, Parser


@dataclass(frozen=True)
class LanguageSpec:
    name: str
    extensions: frozenset[str]
    _language: Callable[[], object]

    #: Nodes that introduce a callable scope.
    function_nodes: frozenset[str]
    #: Nodes that introduce a class scope.
    class_nodes: frozenset[str]
    #: The translation-unit root node type.
    module_node: str

    #: Each occurrence adds 1 to cyclomatic complexity.
    decision_nodes: frozenset[str]
    #: Nodes representing a short-circuit boolean operator. For grammars that
    #: model these as a generic binary node, `boolean_operators` disambiguates
    #: by operator text.
    boolean_op_nodes: frozenset[str]
    boolean_operators: frozenset[str]

    #: Nodes that open a nesting level.
    nesting_nodes: frozenset[str]
    #: Clauses that continue an existing branch rather than nesting deeper, so
    #: that `else if` / `elif` chains read as flat instead of as a staircase.
    continuation_nodes: frozenset[str]

    #: Nodes holding a callable's parameter list.
    param_containers: frozenset[str]
    #: Implicit receiver parameters that should not count against a signature.
    implicit_params: frozenset[str]

    #: Type names treated as primitives for Primitive Obsession.
    primitive_types: frozenset[str]
    #: Type names treated as strings for String Heavy Function Arguments.
    string_types: frozenset[str]

    #: Node types carrying a type annotation.
    type_annotation_nodes: frozenset[str]

    #: Name of a constructor / initialiser, for Constructor Over-Injection.
    constructor_names: frozenset[str]

    #: Statement nodes that are assertions.
    assertion_nodes: frozenset[str]
    #: Call-callee prefixes that mark an assertion (e.g. `expect`, `assert*`).
    assertion_call_prefixes: frozenset[str]

    #: Nodes that declare types rather than behaviour (Lines of Declarations).
    declaration_nodes: frozenset[str] = field(default_factory=frozenset)

    #: Field name holding a node's identifier, tried in order.
    name_fields: tuple[str, ...] = ("name",)

    @property
    def language(self) -> Language:
        return _language_for(self.name)

    def parser(self) -> Parser:
        return _parser_for(self.name)


PYTHON = LanguageSpec(
    name="python",
    extensions=frozenset({".py", ".pyi"}),
    _language=tree_sitter_python.language,
    function_nodes=frozenset({"function_definition", "lambda"}),
    class_nodes=frozenset({"class_definition"}),
    module_node="module",
    decision_nodes=frozenset(
        {
            "if_statement",
            "elif_clause",
            "for_statement",
            "while_statement",
            "except_clause",
            "conditional_expression",
            "case_clause",
            "if_clause",  # comprehension filter
            "for_in_clause",  # comprehension generator
        }
    ),
    boolean_op_nodes=frozenset({"boolean_operator"}),
    boolean_operators=frozenset({"and", "or"}),
    nesting_nodes=frozenset(
        {
            "if_statement",
            "for_statement",
            "while_statement",
            "try_statement",
            "with_statement",
            "match_statement",
        }
    ),
    continuation_nodes=frozenset({"elif_clause", "else_clause"}),
    param_containers=frozenset({"parameters", "lambda_parameters"}),
    implicit_params=frozenset({"self", "cls", "mcs", "metacls"}),
    primitive_types=frozenset(
        {"str", "int", "float", "bool", "bytes", "complex", "bytearray"}
    ),
    string_types=frozenset({"str", "bytes"}),
    type_annotation_nodes=frozenset({"type"}),
    constructor_names=frozenset({"__init__", "__new__"}),
    assertion_nodes=frozenset({"assert_statement"}),
    assertion_call_prefixes=frozenset({"assert", "expect", "self.assert"}),
    declaration_nodes=frozenset(),
)

_TS_DECISION = frozenset(
    {
        "if_statement",
        "for_statement",
        "for_in_statement",
        "while_statement",
        "do_statement",
        "catch_clause",
        "switch_case",  # switch_default is deliberately excluded
        "ternary_expression",
    }
)
_TS_NESTING = frozenset(
    {
        "if_statement",
        "for_statement",
        "for_in_statement",
        "while_statement",
        "do_statement",
        "try_statement",
        "switch_statement",
    }
)
_TS_FUNCTIONS = frozenset(
    {
        "function_declaration",
        "function_expression",
        "generator_function",
        "generator_function_declaration",
        "arrow_function",
        "method_definition",
    }
)


def _typescript_spec(name: str, extensions: set[str], lang_fn) -> LanguageSpec:
    return LanguageSpec(
        name=name,
        extensions=frozenset(extensions),
        _language=lang_fn,
        function_nodes=_TS_FUNCTIONS,
        class_nodes=frozenset({"class_declaration", "class"}),
        module_node="program",
        decision_nodes=_TS_DECISION,
        # TS/JS model `&&`, `||` and `??` as binary_expression; the operator set
        # below is what distinguishes them from arithmetic.
        boolean_op_nodes=frozenset({"binary_expression"}),
        boolean_operators=frozenset({"&&", "||", "??"}),
        nesting_nodes=_TS_NESTING,
        continuation_nodes=frozenset({"else_clause"}),
        param_containers=frozenset({"formal_parameters"}),
        implicit_params=frozenset({"this"}),
        primitive_types=frozenset(
            {"string", "number", "boolean", "bigint", "symbol"}
        ),
        string_types=frozenset({"string"}),
        type_annotation_nodes=frozenset({"type_annotation"}),
        constructor_names=frozenset({"constructor"}),
        assertion_nodes=frozenset(),
        assertion_call_prefixes=frozenset({"expect", "assert", "chai.assert"}),
        declaration_nodes=frozenset(
            {
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
            }
        ),
    )


# The tsx grammar is a superset of the ts grammar and parses both; using it for
# `.ts` too avoids a second grammar and keeps node names identical across both.
TYPESCRIPT = _typescript_spec(
    "typescript",
    {".ts", ".tsx", ".mts", ".cts"},
    tree_sitter_typescript.language_tsx,
)
JAVASCRIPT = _typescript_spec(
    "javascript",
    {".js", ".jsx", ".mjs", ".cjs"},
    tree_sitter_javascript.language,
)

SPECS: tuple[LanguageSpec, ...] = (PYTHON, TYPESCRIPT, JAVASCRIPT)

_BY_EXTENSION: dict[str, LanguageSpec] = {
    ext: spec for spec in SPECS for ext in spec.extensions
}
_BY_NAME: dict[str, LanguageSpec] = {spec.name: spec for spec in SPECS}


def spec_for_extension(extension: str) -> LanguageSpec | None:
    return _BY_EXTENSION.get(extension.lower())


def spec_for_name(name: str) -> LanguageSpec | None:
    return _BY_NAME.get(name)


def supported_extensions() -> frozenset[str]:
    return frozenset(_BY_EXTENSION)


@lru_cache(maxsize=None)
def _language_for(name: str) -> Language:
    return Language(_BY_NAME[name]._language())


@lru_cache(maxsize=None)
def _parser_for(name: str) -> Parser:
    return Parser(_language_for(name))
