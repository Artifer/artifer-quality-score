"""The rule catalogue.

Each detector reports a measurement and the threshold it crossed; none of them
knows anything about points. Turning findings into a score is `scoring.py`'s job
alone, which is what makes the penalty model swappable.

`details` strings follow CodeScene's formats (`cc = 14`, `bumps = 4`,
`Nesting depth = 5 conditionals`) — they are compact, they read well in a diff,
and there is no reason to invent a second dialect.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from difflib import SequenceMatcher
from typing import Callable, Iterable, Iterator

from tree_sitter import Node

from . import metrics, profile as prof
from .languages import LanguageSpec
from .models import Range
from .profile import Profile


@dataclass(frozen=True)
class RawFinding:
    """A detection, before it has a price."""

    category: str
    details: str
    range: Range
    value: float
    threshold: float
    function: str | None = None


@dataclass
class FunctionContext:
    """A named function and its measurements, computed once and shared."""

    node: Node
    name: str
    range: Range
    loc: int
    complexity: int
    nesting: int
    parameters: list[Node]
    is_constructor: bool

    @property
    def signature(self) -> str:
        return self.name


@dataclass
class FileContext:
    path: str
    spec: LanguageSpec
    profile: Profile
    root: Node
    source: bytes
    is_test: bool
    loc: int
    functions: list[FunctionContext] = field(default_factory=list)

    def threshold(self, key: str) -> float:
        return self.profile.threshold(key)

    def size_threshold(self, key: str) -> float:
        """A size limit, relaxed for test files."""
        value = self.profile.threshold(key)
        if self.is_test:
            value *= self.profile.threshold("test_file_size_multiplier")
        return value


FunctionDetector = Callable[[FunctionContext, FileContext], Iterable[RawFinding]]
FileDetector = Callable[[FileContext], Iterable[RawFinding]]

FUNCTION_DETECTORS: list[FunctionDetector] = []
FILE_DETECTORS: list[FileDetector] = []


def function_detector(fn: FunctionDetector) -> FunctionDetector:
    FUNCTION_DETECTORS.append(fn)
    return fn


def file_detector(fn: FileDetector) -> FileDetector:
    FILE_DETECTORS.append(fn)
    return fn


def build_context(
    path: str, spec: LanguageSpec, profile: Profile, root: Node, source: bytes
) -> FileContext:
    context = FileContext(
        path=path,
        spec=spec,
        profile=profile,
        root=root,
        source=source,
        is_test=profile.is_test_path(path),
        loc=metrics.logical_lines(root),
    )
    for node in metrics.iter_functions(root, spec):
        name = metrics.function_name(node, spec) or "<anonymous>"
        context.functions.append(
            FunctionContext(
                node=node,
                name=name,
                range=metrics.to_range(node),
                loc=metrics.logical_lines(node),
                complexity=metrics.cyclomatic_complexity(node, spec),
                nesting=metrics.nesting_depth(node, spec),
                parameters=metrics.parameters(node, spec),
                is_constructor=name in spec.constructor_names,
            )
        )
    return context


def _signature_range(fn: FunctionContext) -> Range:
    """Just the declaration line — where a reader wants the marker."""
    return Range(
        start_line=fn.range.start_line,
        start_column=fn.range.start_column,
        end_line=fn.range.start_line,
        end_column=fn.range.end_column,
    )


# --------------------------------------------------------------------------- #
# Function level
# --------------------------------------------------------------------------- #


@function_detector
def complex_method(fn: FunctionContext, ctx: FileContext) -> Iterator[RawFinding]:
    threshold = ctx.threshold("function_cyclomatic_complexity_warning")
    if fn.complexity > threshold:
        yield RawFinding(
            category=prof.COMPLEX_METHOD,
            details=f"cc = {fn.complexity}",
            range=_signature_range(fn),
            value=fn.complexity,
            threshold=threshold,
            function=fn.name,
        )


@function_detector
def large_method(fn: FunctionContext, ctx: FileContext) -> Iterator[RawFinding]:
    threshold = ctx.size_threshold("function_lines_of_code_warning")
    if fn.loc > threshold:
        yield RawFinding(
            category=prof.LARGE_METHOD,
            details=f"LoC = {fn.loc} lines",
            range=_signature_range(fn),
            value=fn.loc,
            threshold=threshold,
            function=fn.name,
        )


@function_detector
def deep_nested_complexity(
    fn: FunctionContext, ctx: FileContext
) -> Iterator[RawFinding]:
    threshold = ctx.threshold("function_nesting_depth_warning")
    if fn.nesting > threshold:
        yield RawFinding(
            category=prof.DEEP_NESTED_COMPLEXITY,
            details=f"Nesting depth = {fn.nesting} conditionals",
            range=_signature_range(fn),
            value=fn.nesting,
            threshold=threshold,
            function=fn.name,
        )


@function_detector
def bumpy_road(fn: FunctionContext, ctx: FileContext) -> Iterator[RawFinding]:
    min_bumps = ctx.threshold("bumpy_road_min_bumps")
    found = metrics.bumps(
        fn.node, ctx.spec, int(ctx.threshold("bumpy_road_min_bump_depth"))
    )
    if len(found) >= min_bumps:
        yield RawFinding(
            category=prof.BUMPY_ROAD,
            details=f"bumps = {len(found)}",
            range=_signature_range(fn),
            value=len(found),
            threshold=min_bumps,
            function=fn.name,
        )


@function_detector
def complex_conditional(fn: FunctionContext, ctx: FileContext) -> Iterator[RawFinding]:
    min_count = ctx.threshold("complex_conditional_min_count")
    found = metrics.complex_conditionals(
        fn.node, ctx.spec, int(ctx.threshold("complex_conditional_min_operators"))
    )
    if len(found) >= min_count:
        plural = "expression" if len(found) == 1 else "expressions"
        # Anchor on the first offending conditional, not the signature: this is
        # an expression-level smell and the exact spot is the actionable part.
        yield RawFinding(
            category=prof.COMPLEX_CONDITIONAL,
            details=f"{len(found)} complex conditional {plural}",
            range=metrics.to_range(found[0][0]),
            value=len(found),
            threshold=min_count,
            function=fn.name,
        )


@function_detector
def excess_arguments(fn: FunctionContext, ctx: FileContext) -> Iterator[RawFinding]:
    # Constructors are covered by Constructor Over-Injection; charging both would
    # double-count the same signature.
    if fn.is_constructor:
        return
    threshold = ctx.threshold("function_max_arguments")
    count = len(fn.parameters)
    if count > threshold:
        yield RawFinding(
            category=prof.EXCESS_ARGUMENTS,
            details=f"Arguments = {count}",
            range=_signature_range(fn),
            value=count,
            threshold=threshold,
            function=fn.name,
        )


@function_detector
def constructor_over_injection(
    fn: FunctionContext, ctx: FileContext
) -> Iterator[RawFinding]:
    if not fn.is_constructor:
        return
    threshold = ctx.threshold("constructor_max_arguments")
    count = len(fn.parameters)
    if count > threshold:
        yield RawFinding(
            category=prof.CONSTRUCTOR_OVER_INJECTION,
            details=f"Arguments = {count}",
            range=_signature_range(fn),
            value=count,
            threshold=threshold,
            function=fn.name,
        )


@function_detector
def brain_method(fn: FunctionContext, ctx: FileContext) -> Iterator[RawFinding]:
    """Compound rule: size *and* complexity *and* nesting, all at once.

    CodeScene's version also weighs a centrality score, which needs whole-repo
    call-graph data we deliberately do not collect (see README, "What we left
    out"). Size, complexity and depth together still isolate the one function in
    a module that has quietly become its centre of gravity.
    """
    loc_threshold = ctx.threshold("brain_method_lines_of_code")
    cc_threshold = ctx.threshold("brain_method_cyclomatic_complexity")
    nesting_threshold = ctx.threshold("brain_method_nesting_depth")
    if (
        fn.loc > loc_threshold
        and fn.complexity > cc_threshold
        and fn.nesting > nesting_threshold
    ):
        yield RawFinding(
            category=prof.BRAIN_METHOD,
            details=(
                f"LoC = {fn.loc} lines, cc = {fn.complexity}, "
                f"nesting depth = {fn.nesting}"
            ),
            range=_signature_range(fn),
            value=fn.complexity,
            threshold=cc_threshold,
            function=fn.name,
        )


@function_detector
def string_heavy_arguments(
    fn: FunctionContext, ctx: FileContext
) -> Iterator[RawFinding]:
    threshold = ctx.threshold("function_string_arguments_warning")
    strings = sum(
        1
        for param in fn.parameters
        if metrics.is_string_type(metrics.parameter_type(param, ctx.spec), ctx.spec)
    )
    if strings > threshold:
        yield RawFinding(
            category=prof.STRING_HEAVY_ARGUMENTS,
            details=f"{strings} string arguments",
            range=_signature_range(fn),
            value=strings,
            threshold=threshold,
            function=fn.name,
        )


@function_detector
def primitive_obsession(fn: FunctionContext, ctx: FileContext) -> Iterator[RawFinding]:
    """A wide signature made almost entirely of built-in types.

    Only annotated parameters are considered — an unannotated parameter carries
    no evidence either way, and guessing would make the score depend on how
    thoroughly the code is typed rather than on how well it is designed.
    """
    min_count = ctx.threshold("function_primitive_arguments_min_count")
    ratio_threshold = ctx.threshold("function_primitive_arguments_ratio")
    types = [metrics.parameter_type(param, ctx.spec) for param in fn.parameters]
    annotated = [name for name in types if name]
    if len(annotated) < min_count:
        return
    primitives = sum(
        1 for name in annotated if metrics.is_primitive_type(name, ctx.spec)
    )
    ratio = primitives / len(annotated)
    if ratio >= ratio_threshold:
        yield RawFinding(
            category=prof.PRIMITIVE_OBSESSION,
            details=(
                f"{primitives} of {len(annotated)} arguments are primitives "
                f"({ratio:.0%})"
            ),
            range=_signature_range(fn),
            value=primitives,
            threshold=min_count,
            function=fn.name,
        )


# --------------------------------------------------------------------------- #
# File / module level
# --------------------------------------------------------------------------- #


@file_detector
def code_duplication(ctx: FileContext) -> Iterator[RawFinding]:
    """Near-identical function bodies within one file.

    Compares structure only: identifiers and literals are normalised away, so
    two handlers that differ solely in field names still register. This is
    intra-file by design — cross-file clone detection needs an index, which
    would cost the sub-second scan time the TDD loop depends on.
    """
    min_loc = ctx.threshold("function_duplication_min_lines_of_code_for_check")
    min_similarity = (
        ctx.threshold("function_duplication_min_similarity_percentage") / 100.0
    )
    candidates = [
        (fn, metrics.normalised_tokens(fn.node, ctx.spec))
        for fn in ctx.functions
        if fn.loc >= min_loc
    ]
    reported: set[str] = set()
    for index, (fn, tokens) in enumerate(candidates):
        for other, other_tokens in candidates[index + 1 :]:
            if not tokens or not other_tokens:
                continue
            shorter, longer = sorted((len(tokens), len(other_tokens)))
            # Length alone caps the achievable ratio, so this skips the
            # expensive comparison for pairs that cannot possibly match.
            if shorter / longer < min_similarity:
                continue
            ratio = SequenceMatcher(None, tokens, other_tokens).ratio()
            if ratio < min_similarity:
                continue
            key = f"{other.name}:{other.range.start_line}"
            if key in reported:
                continue
            reported.add(key)
            yield RawFinding(
                category=prof.CODE_DUPLICATION,
                details=f"{ratio:.0%} similar to {fn.name}",
                range=_signature_range(other),
                value=ratio * 100,
                threshold=min_similarity * 100,
                function=other.name,
            )


def _assertion_runs(ctx: FileContext) -> Iterator[tuple[list[Node], Node]]:
    """Maximal runs of consecutive assertion statements, with their block."""
    for node in metrics.iter_all_nodes(ctx.root):
        if node.type not in {"block", "statement_block", "module", "program"}:
            continue
        run: list[Node] = []
        for statement in node.named_children:
            if metrics.is_assertion(statement, ctx.spec):
                run.append(statement)
                continue
            if run:
                yield run, node
            run = []
        if run:
            yield run, node


def _enclosing_function(node: Node, ctx: FileContext) -> str | None:
    current = node.parent
    while current is not None:
        if metrics.is_scope_boundary(current, ctx.spec):
            return metrics.function_name(current, ctx.spec)
        current = current.parent
    return None


@file_detector
def large_assertion_blocks(ctx: FileContext) -> Iterator[RawFinding]:
    """A wall of consecutive asserts, which is a missing abstraction."""
    if not ctx.is_test:
        return
    threshold = ctx.threshold("assertion_block_max_statements")
    for run, _block in _assertion_runs(ctx):
        if len(run) > threshold:
            yield RawFinding(
                category=prof.LARGE_ASSERTION_BLOCKS,
                details=f"{len(run)} consecutive assertions",
                range=metrics.to_range(run[0]),
                value=len(run),
                threshold=threshold,
                function=_enclosing_function(run[0], ctx),
            )


@file_detector
def duplicated_assertion_blocks(ctx: FileContext) -> Iterator[RawFinding]:
    """The same assertion block copy-pasted across a test suite."""
    if not ctx.is_test:
        return
    min_statements = int(ctx.threshold("duplicated_assertion_block_min_statements"))
    groups: dict[tuple, list[Node]] = {}
    for run, _block in _assertion_runs(ctx):
        if len(run) < min_statements:
            continue
        signature = tuple(
            token
            for statement in run
            for token in metrics.normalised_tokens(statement, ctx.spec)
        )
        groups.setdefault(signature, []).append(run[0])

    for occurrences in groups.values():
        if len(occurrences) < 2:
            continue
        # Report on every repeat after the first; the original is not the problem.
        for node in occurrences[1:]:
            yield RawFinding(
                category=prof.DUPLICATED_ASSERTION_BLOCKS,
                details=f"assertion block repeated {len(occurrences)} times",
                range=metrics.to_range(node),
                value=len(occurrences),
                threshold=2,
                function=_enclosing_function(node, ctx),
            )


@file_detector
def file_size(ctx: FileContext) -> Iterator[RawFinding]:
    threshold = ctx.size_threshold("file_lines_of_code_warning")
    if ctx.loc > threshold:
        yield RawFinding(
            category=prof.FILE_SIZE,
            details=f"LoC = {ctx.loc} lines",
            range=Range(start_line=1, end_line=1),
            value=ctx.loc,
            threshold=threshold,
        )


@file_detector
def declaration_size(ctx: FileContext) -> Iterator[RawFinding]:
    """Type/interface/enum declarations dominating a file."""
    if not ctx.spec.declaration_nodes:
        return
    total = sum(
        metrics.logical_lines(node)
        for node in metrics.iter_all_nodes(ctx.root)
        if node.type in ctx.spec.declaration_nodes
    )
    threshold = ctx.size_threshold("file_declaration_lines_warning")
    if total > threshold:
        yield RawFinding(
            category=prof.DECLARATION_SIZE,
            details=f"LoC = {total} lines of declarations",
            range=Range(start_line=1, end_line=1),
            value=total,
            threshold=threshold,
        )


@file_detector
def function_count(ctx: FileContext) -> Iterator[RawFinding]:
    threshold = ctx.size_threshold("module_max_functions")
    count = len(ctx.functions)
    if count > threshold:
        yield RawFinding(
            category=prof.FUNCTION_COUNT,
            details=f"{count} functions",
            range=Range(start_line=1, end_line=1),
            value=count,
            threshold=threshold,
        )


@file_detector
def overall_complexity(ctx: FileContext) -> Iterator[RawFinding]:
    """Mean complexity across a module — diffuse difficulty, not one hotspot.

    Requires at least three functions: with fewer, the per-function rules
    already say everything there is to say.
    """
    if len(ctx.functions) < 3:
        return
    threshold = ctx.threshold("module_mean_complexity_warning")
    mean = sum(fn.complexity for fn in ctx.functions) / len(ctx.functions)
    if mean > threshold:
        yield RawFinding(
            category=prof.OVERALL_COMPLEXITY,
            details=f"mean cc = {mean:.1f} across {len(ctx.functions)} functions",
            range=Range(start_line=1, end_line=1),
            value=mean,
            threshold=threshold,
        )


def _class_methods(class_node: Node, ctx: FileContext) -> list[FunctionContext]:
    inside = []
    for fn in ctx.functions:
        if (
            fn.node.start_byte >= class_node.start_byte
            and fn.node.end_byte <= class_node.end_byte
            and fn.node is not class_node
        ):
            inside.append(fn)
    return inside


@file_detector
def brain_class(ctx: FileContext) -> Iterator[RawFinding]:
    """A large, many-method class that also contains a Brain Method."""
    loc_threshold = ctx.size_threshold("class_max_lines_of_code")
    fn_threshold = ctx.size_threshold("class_max_functions")
    for class_node in metrics.class_nodes(ctx.root, ctx.spec):
        methods = _class_methods(class_node, ctx)
        loc = metrics.logical_lines(class_node)
        has_brain_method = any(
            fn.loc > ctx.threshold("brain_method_lines_of_code")
            and fn.complexity > ctx.threshold("brain_method_cyclomatic_complexity")
            and fn.nesting > ctx.threshold("brain_method_nesting_depth")
            for fn in methods
        )
        if loc > loc_threshold and len(methods) > fn_threshold and has_brain_method:
            name = metrics.function_name(class_node, ctx.spec) or "<anonymous class>"
            yield RawFinding(
                category=prof.BRAIN_CLASS,
                details=(
                    f"LoC = {loc} lines, {len(methods)} functions, "
                    f"contains a Brain Method"
                ),
                range=Range(
                    start_line=class_node.start_point[0] + 1,
                    end_line=class_node.start_point[0] + 1,
                ),
                value=loc,
                threshold=loc_threshold,
                function=name,
            )


def _self_attributes(fn: FunctionContext, ctx: FileContext) -> set[str]:
    """Instance fields a method touches (`self.x` / `this.x`)."""
    receivers = {"self", "this"}
    fields: set[str] = set()
    for node in metrics.iter_own_nodes(fn.node, ctx.spec):
        if node.type not in {"attribute", "member_expression"}:
            continue
        obj = node.child_by_field_name("object")
        attr = node.child_by_field_name("attribute") or node.child_by_field_name(
            "property"
        )
        if obj is None or attr is None:
            continue
        if obj.text.decode("utf-8", "replace") in receivers:
            fields.add(attr.text.decode("utf-8", "replace"))
    return fields


@file_detector
def low_cohesion(ctx: FileContext) -> Iterator[RawFinding]:
    """LCOM4: how many unrelated responsibilities a class actually holds.

    Methods are connected when they share an instance field or one calls the
    other; the metric is the number of connected components. Constructors are
    excluded, since they touch every field and would make any class look
    perfectly cohesive.
    """
    threshold = ctx.threshold("class_max_lcom4")
    for class_node in metrics.class_nodes(ctx.root, ctx.spec):
        if metrics.is_abstract_class(class_node, ctx.spec):
            continue
        methods = [fn for fn in _class_methods(class_node, ctx) if not fn.is_constructor]
        if len(methods) < 2:
            continue

        fields = {fn.name: _self_attributes(fn, ctx) for fn in methods}
        names = {fn.name for fn in methods}
        parent = {fn.name: fn.name for fn in methods}

        def find(name: str) -> str:
            while parent[name] != name:
                parent[name] = parent[parent[name]]
                name = parent[name]
            return name

        def union(left: str, right: str) -> None:
            left_root, right_root = find(left), find(right)
            if left_root != right_root:
                parent[right_root] = left_root

        for index, fn in enumerate(methods):
            # Shared instance state connects two methods.
            for other in methods[index + 1 :]:
                if fields[fn.name] & fields[other.name]:
                    union(fn.name, other.name)
            # So does one calling the other.
            for called in fields[fn.name] & names:
                union(fn.name, called)

        components = len({find(fn.name) for fn in methods})
        if components > threshold:
            name = metrics.function_name(class_node, ctx.spec) or "<anonymous class>"
            yield RawFinding(
                category=prof.LOW_COHESION,
                details=(
                    f"LCOM4 = {components} unrelated responsibilities "
                    f"across {len(methods)} functions"
                ),
                range=Range(
                    start_line=class_node.start_point[0] + 1,
                    end_line=class_node.start_point[0] + 1,
                ),
                value=components,
                threshold=threshold,
                function=name,
            )


def detect(ctx: FileContext) -> list[RawFinding]:
    """Every applicable finding for a file, in a stable order."""
    findings: list[RawFinding] = []
    for detector in FILE_DETECTORS:
        findings.extend(detector(ctx))
    for fn in ctx.functions:
        for detector in FUNCTION_DETECTORS:
            findings.extend(detector(fn, ctx))

    if ctx.is_test and ctx.profile.test_exempt_rules:
        findings = [
            f for f in findings if f.category not in ctx.profile.test_exempt_rules
        ]
    # A rule set to weight 0 should vanish from the report, not sit there at -0.00.
    return [f for f in findings if ctx.profile.rule(f.category).weight > 0.0]
