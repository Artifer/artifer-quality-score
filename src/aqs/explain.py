"""Remediation guidance, one entry per rule.

This is not decoration. The second job of this tool is to give a model something
to optimise against, and a bare category name plus a number is not enough to act
on — "Bumpy Road Ahead, bumps = 4" tells you nothing unless you already know what
a bump is. `aqs explain` is what makes a finding self-describing, so the fix
guidance travels with the report instead of living in someone's head.

Each entry stays short enough to paste several into a prompt without crowding out
the code.
"""

from __future__ import annotations

from dataclasses import dataclass

from . import profile as prof


@dataclass(frozen=True)
class RuleDoc:
    category: str
    summary: str
    why: str
    fix: str
    thresholds: tuple[str, ...] = ()


RULE_DOCS: dict[str, RuleDoc] = {
    prof.COMPLEX_METHOD: RuleDoc(
        category=prof.COMPLEX_METHOD,
        summary="Too many decision points in one function (McCabe cyclomatic complexity).",
        why=(
            "Every branch is another path a reader has to hold in their head and "
            "another path a test has to cover. Complexity is the single metric with "
            "the most consistent published link to defect density."
        ),
        fix=(
            "Extract cohesive groups of branches into named functions. Replace "
            "chains of if/elif on a value with a lookup table or dispatch dict. "
            "Return early instead of nesting the remainder in an else."
        ),
        thresholds=("function_cyclomatic_complexity_warning",),
    ),
    prof.LARGE_METHOD: RuleDoc(
        category=prof.LARGE_METHOD,
        summary="A function with too many lines of code.",
        why=(
            "Length is a proxy for how many things a function does. Past roughly a "
            "screenful you cannot see the whole thing at once, so you stop reasoning "
            "about it and start pattern-matching."
        ),
        fix=(
            "Find the seams — the comment headers, the blank-line groups — and "
            "extract each into a named function. The names you are forced to invent "
            "usually reveal the abstraction that was missing."
        ),
        thresholds=("function_lines_of_code_warning",),
    ),
    prof.DEEP_NESTED_COMPLEXITY: RuleDoc(
        category=prof.DEEP_NESTED_COMPLEXITY,
        summary="Control flow nested several levels deep.",
        why=(
            "Depth compounds: at four levels in, understanding a line means "
            "reconstructing four separate conditions that are all off-screen. It is "
            "one of the strongest structural predictors of defects."
        ),
        fix=(
            "Invert the guards and return, raise or continue early to flatten the "
            "happy path back to the left margin. Extract the innermost block into "
            "its own function. Replace nested loops with a comprehension or a "
            "helper that yields."
        ),
        thresholds=("function_nesting_depth_warning",),
    ),
    prof.BUMPY_ROAD: RuleDoc(
        category=prof.BUMPY_ROAD,
        summary=(
            "One function containing several separate lumps of nested logic — "
            "each 'bump' is a chunk that wants to be its own function."
        ),
        why=(
            "A bumpy road mixes levels of abstraction: the function is both the "
            "high-level sequence and the low-level detail of every step. Reading it "
            "means repeatedly diving down and surfacing again."
        ),
        fix=(
            "Extract each bump into a named function so the parent reads as a flat "
            "sequence of steps. The goal is a function whose body is all at one "
            "level of abstraction."
        ),
        thresholds=("bumpy_road_min_bumps", "bumpy_road_min_bump_depth"),
    ),
    prof.COMPLEX_CONDITIONAL: RuleDoc(
        category=prof.COMPLEX_CONDITIONAL,
        summary="A branch condition combining multiple boolean operators.",
        why=(
            "Compound conditions are where off-by-one logic errors live, and mixed "
            "and/or precedence is read wrongly by humans far more often than by "
            "compilers."
        ),
        fix=(
            "Name the condition: assign it to a well-named boolean variable, or "
            "extract a predicate function. `if is_eligible_for_discount(order)` "
            "needs no comment; the original did."
        ),
        thresholds=(
            "complex_conditional_min_operators",
            "complex_conditional_min_count",
        ),
    ),
    prof.EXCESS_ARGUMENTS: RuleDoc(
        category=prof.EXCESS_ARGUMENTS,
        summary="A function taking more arguments than a reader can track.",
        why=(
            "A long parameter list means callers must remember an order, and it "
            "usually signals that several parameters travel together and belong in "
            "one object."
        ),
        fix=(
            "Group the parameters that always appear together into a dataclass, "
            "TypedDict or interface. Make optional flags keyword-only. If the "
            "function needs a lot of unrelated state, it is probably doing several "
            "jobs."
        ),
        thresholds=("function_max_arguments",),
    ),
    prof.BRAIN_METHOD: RuleDoc(
        category=prof.BRAIN_METHOD,
        summary=(
            "A function that is long, complex and deeply nested at once — the "
            "module's centre of gravity."
        ),
        why=(
            "Brain Methods are where the behaviour of a whole module quietly "
            "accumulated. They are the highest-value refactoring target in a file: "
            "everything depends on them and nobody wants to touch them."
        ),
        fix=(
            "Refactor in stages rather than at once. Flatten the nesting first "
            "(early returns), then extract the bumps into named functions, then "
            "look at what the remaining skeleton is actually orchestrating. Add "
            "tests before you start; this is the function most likely to hide "
            "behaviour nobody remembers."
        ),
        thresholds=(
            "brain_method_lines_of_code",
            "brain_method_cyclomatic_complexity",
            "brain_method_nesting_depth",
        ),
    ),
    prof.CONSTRUCTOR_OVER_INJECTION: RuleDoc(
        category=prof.CONSTRUCTOR_OVER_INJECTION,
        summary="A constructor taking too many collaborators.",
        why=(
            "The number of things you must supply to build an object is the number "
            "of things it depends on. A long constructor list means the class has "
            "too many responsibilities, and it makes the class painful to test."
        ),
        fix=(
            "Group related collaborators into a single higher-level service, or "
            "split the class along the lines its dependencies suggest."
        ),
        thresholds=("constructor_max_arguments",),
    ),
    prof.STRING_HEAVY_ARGUMENTS: RuleDoc(
        category=prof.STRING_HEAVY_ARGUMENTS,
        summary="Several adjacent string parameters.",
        why=(
            "Adjacent same-typed parameters can be swapped at a call site and "
            "nothing will complain — not the type checker, not the tests, until "
            "production. Strings are the worst offender because everything is one."
        ),
        fix=(
            "Introduce types that mean something: an enum for a fixed set, a "
            "NewType or branded type for an identifier, a small value object for a "
            "pair that belongs together."
        ),
        thresholds=("function_string_arguments_warning",),
    ),
    prof.PRIMITIVE_OBSESSION: RuleDoc(
        category=prof.PRIMITIVE_OBSESSION,
        summary="A wide signature built almost entirely from built-in types.",
        why=(
            "When everything is a str or an int, the domain rules have nowhere to "
            "live, so validation gets copy-pasted to every call site and drifts."
        ),
        fix=(
            "Promote the concepts to types. A `Money`, `EmailAddress` or `TenderId` "
            "carries its own validation and cannot be confused with an unrelated "
            "value of the same shape."
        ),
        thresholds=(
            "function_primitive_arguments_min_count",
            "function_primitive_arguments_ratio",
        ),
    ),
    prof.CODE_DUPLICATION: RuleDoc(
        category=prof.CODE_DUPLICATION,
        summary=(
            "Two functions in this file with near-identical structure, ignoring "
            "names and literals."
        ),
        why=(
            "Duplicated logic means a fix has to be found and applied more than "
            "once. In practice one copy gets fixed and the other keeps the bug."
        ),
        fix=(
            "Extract the shared shape into one function and pass the differences as "
            "arguments. If the two copies are diverging for good reasons, make that "
            "explicit rather than leaving them accidentally similar."
        ),
        thresholds=(
            "function_duplication_min_lines_of_code_for_check",
            "function_duplication_min_similarity_percentage",
        ),
    ),
    prof.LARGE_ASSERTION_BLOCKS: RuleDoc(
        category=prof.LARGE_ASSERTION_BLOCKS,
        summary="A long run of consecutive assertions in a test.",
        why=(
            "A wall of asserts means the test has no single subject, and when it "
            "fails you learn only the first thing that broke. It is a missing "
            "abstraction, not thoroughness."
        ),
        fix=(
            "Assert on one composed object instead of many fields, extract a custom "
            "matcher or helper that expresses the intent, or split into several "
            "tests each with one reason to fail."
        ),
        thresholds=("assertion_block_max_statements",),
    ),
    prof.DUPLICATED_ASSERTION_BLOCKS: RuleDoc(
        category=prof.DUPLICATED_ASSERTION_BLOCKS,
        summary="The same block of assertions copy-pasted across tests.",
        why=(
            "Repeated assertion blocks make a test suite expensive to change: one "
            "behaviour change means editing the same lines in a dozen places."
        ),
        fix=(
            "Extract a shared assertion helper with a name that states what it "
            "guarantees, or drive the cases from a parametrised test."
        ),
        thresholds=("duplicated_assertion_block_min_statements",),
    ),
    prof.FILE_SIZE: RuleDoc(
        category=prof.FILE_SIZE,
        summary="A file with too many lines of code.",
        why=(
            "Large files are hard to navigate, they generate merge conflicts, and "
            "they are usually large because several unrelated concerns landed in the "
            "same place."
        ),
        fix=(
            "Split along the concerns already implicit in the file — the groups of "
            "functions that share state or vocabulary."
        ),
        thresholds=("file_lines_of_code_warning", "test_file_size_multiplier"),
    ),
    prof.DECLARATION_SIZE: RuleDoc(
        category=prof.DECLARATION_SIZE,
        summary="A file dominated by type, interface or enum declarations.",
        why=(
            "A very large declaration file is usually a shared dumping ground that "
            "everything imports, which couples otherwise unrelated modules together."
        ),
        fix="Move each group of types next to the code that owns them.",
        thresholds=("file_declaration_lines_warning",),
    ),
    prof.FUNCTION_COUNT: RuleDoc(
        category=prof.FUNCTION_COUNT,
        summary="A module declaring a large number of functions.",
        why=(
            "Past a certain count a module stops being a unit with a purpose and "
            "becomes a bag of utilities, which is where code goes to be forgotten "
            "and reimplemented."
        ),
        fix=(
            "Group the functions that share vocabulary or state into their own "
            "module, or into a class if they share state."
        ),
        thresholds=("module_max_functions",),
    ),
    prof.OVERALL_COMPLEXITY: RuleDoc(
        category=prof.OVERALL_COMPLEXITY,
        summary="High mean complexity across the whole module.",
        why=(
            "This is diffuse difficulty rather than one hotspot: no single function "
            "looks alarming, but every one of them is harder than it should be. It "
            "often points at a missing abstraction the whole file is working around."
        ),
        fix=(
            "Look for the condition or data shape that keeps reappearing across "
            "functions and give it a home — a type, a guard, a parsing step at the "
            "boundary — so the rest of the module can stop re-checking it."
        ),
        thresholds=("module_mean_complexity_warning",),
    ),
    prof.BRAIN_CLASS: RuleDoc(
        category=prof.BRAIN_CLASS,
        summary=(
            "A large class with many methods that also contains a Brain Method."
        ),
        why=(
            "The combination is what matters: the class has accumulated both "
            "breadth and a deep well of logic, so it is simultaneously the thing "
            "everything touches and the thing nobody understands."
        ),
        fix=(
            "Deal with the Brain Method first — it is usually the reason the class "
            "grew. Then look at the Low Cohesion signal for where the class wants "
            "to be split."
        ),
        thresholds=("class_max_lines_of_code", "class_max_functions"),
    ),
    prof.LOW_COHESION: RuleDoc(
        category=prof.LOW_COHESION,
        summary=(
            "A class whose methods form several unrelated groups (LCOM4) — they "
            "share no state and do not call each other."
        ),
        why=(
            "Disconnected method groups mean the class is really several classes "
            "sharing a name. Every consumer takes on a dependency on all of them."
        ),
        fix=(
            "Split the class along the component boundaries. The groups the metric "
            "found are usually the classes that wanted to exist."
        ),
        thresholds=("class_max_lcom4",),
    ),
}


def lookup(query: str) -> RuleDoc | None:
    """Find a rule by name, case- and punctuation-insensitively."""

    def normalise(text: str) -> str:
        return "".join(ch for ch in text.lower() if ch.isalnum())

    target = normalise(query)
    for category, doc in RULE_DOCS.items():
        if normalise(category) == target:
            return doc
    # Fall back to a prefix match so `aqs explain bumpy` works.
    for category, doc in RULE_DOCS.items():
        if normalise(category).startswith(target):
            return doc
    return None
