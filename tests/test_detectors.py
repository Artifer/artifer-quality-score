"""One test per rule: it fires when it should, and stays quiet when it should not.

The negative cases matter more than the positive ones. A detector that flags
correct code is worse than one that misses a problem, because the whole point is
to be a signal a model can follow — and a false positive teaches it to make the
code worse.
"""

from __future__ import annotations

import pytest

from aqs import profile as prof


class TestComplexMethod:
    def test_fires_above_threshold(self, score, find):
        body = "\n".join(f"    if x == {i}: return {i}" for i in range(12))
        report = score(f"def f(x):\n{body}\n    return None\n")
        finding = find(report, prof.COMPLEX_METHOD)
        assert finding is not None
        assert finding.details == "cc = 13"
        assert finding.function == "f"

    def test_quiet_for_simple_function(self, score, categories):
        report = score("def f(x):\n    if x:\n        return 1\n    return 0\n")
        assert prof.COMPLEX_METHOD not in categories(report)

    def test_boolean_operators_count(self, score, find):
        report = score("def f(a, b):\n    return a and b\n")
        assert find(report, prof.COMPLEX_METHOD) is None  # cc = 2, well under

    def test_named_nested_function_is_measured_separately(self, score):
        """A nested helper's complexity belongs to the helper, not the parent."""
        branches = "\n".join(f"        if x == {i}: return {i}" for i in range(14))
        report = score(f"def outer(x):\n    def inner(x):\n{branches}\n    return inner\n")
        flagged = {
            fn.function
            for fn in report.function_level_code_smells
            for smell in fn.code_smells
            if smell.category == prof.COMPLEX_METHOD
        }
        assert flagged == {"inner"}

    def test_anonymous_callback_counts_toward_parent(self, score, find):
        """An inline lambda's branching lands on whoever has to read it."""
        report = score(
            "def outer(items):\n"
            "    return sorted(items, key=lambda r: (r.a and r.b) or (r.c and r.d))\n"
        )
        finding = find(report, prof.COMPLEX_METHOD)
        assert finding is None  # cc = 5, under threshold, but attributed here
        functions = {fn.function for fn in report.function_level_code_smells}
        assert "<anonymous>" not in functions


class TestLargeMethod:
    def test_fires_on_long_function(self, score, find):
        body = "\n".join(f"    x{i} = {i}" for i in range(70))
        report = score(f"def f():\n{body}\n")
        finding = find(report, prof.LARGE_METHOD)
        assert finding is not None
        assert "lines" in finding.details

    def test_docstrings_and_comments_are_not_lines_of_code(self, score, categories):
        """Deleting documentation must never be a way to improve the score."""
        comments = "\n".join(f"    # explanation line {i}" for i in range(80))
        report = score(
            'def f():\n    """A long docstring.\n\n    More prose here.\n    """\n'
            f"{comments}\n    return 1\n"
        )
        assert prof.LARGE_METHOD not in categories(report)


class TestDeepNestedComplexity:
    def test_fires_at_depth_four(self, score, find):
        report = score(
            "def f(a):\n"
            "    if a:\n"
            "        for i in a:\n"
            "            while i:\n"
            "                if i > 1:\n"
            "                    return i\n"
        )
        finding = find(report, prof.DEEP_NESTED_COMPLEXITY)
        assert finding is not None
        assert finding.details == "Nesting depth = 4 conditionals"

    def test_elif_chain_is_flat(self, score, categories):
        """`elif` is a sibling branch, not another level of nesting."""
        chain = "\n".join(
            f"    {'if' if i == 0 else 'elif'} x == {i}:\n        return {i}"
            for i in range(8)
        )
        report = score(f"def f(x):\n{chain}\n")
        assert prof.DEEP_NESTED_COMPLEXITY not in categories(report)

    def test_else_if_chain_is_flat_in_typescript(self, score, categories):
        chain = " else ".join(f"if (x === {i}) {{ return {i}; }}" for i in range(8))
        report = score(
            f"function f(x: number) {{ {chain} return -1; }}",
            path="sample.ts",
            language="typescript",
        )
        assert prof.DEEP_NESTED_COMPLEXITY not in categories(report)


class TestBumpyRoad:
    def test_fires_on_two_nested_chunks(self, score, find):
        report = score(
            "def f(a, b):\n"
            "    if a:\n"
            "        for i in a:\n"
            "            print(i)\n"
            "    if b:\n"
            "        for j in b:\n"
            "            print(j)\n"
        )
        finding = find(report, prof.BUMPY_ROAD)
        assert finding is not None
        assert finding.details == "bumps = 2"

    def test_quiet_for_single_chunk(self, score, categories):
        report = score(
            "def f(a):\n    if a:\n        for i in a:\n            print(i)\n"
        )
        assert prof.BUMPY_ROAD not in categories(report)


class TestComplexConditional:
    def test_fires_on_two_operators(self, score, find):
        report = score("def f(a, b, c):\n    if a and b and c:\n        return 1\n")
        finding = find(report, prof.COMPLEX_CONDITIONAL)
        assert finding is not None
        assert finding.details == "1 complex conditional expression"

    def test_quiet_on_single_operator(self, score, categories):
        report = score("def f(a, b):\n    if a and b:\n        return 1\n")
        assert prof.COMPLEX_CONDITIONAL not in categories(report)

    def test_typescript_logical_operators(self, score, find):
        report = score(
            "function f(a: boolean, b: boolean, c: boolean) {"
            " if (a && b || c) { return 1 } return 0 }",
            path="sample.ts",
            language="typescript",
        )
        assert find(report, prof.COMPLEX_CONDITIONAL) is not None

    def test_arithmetic_is_not_a_logical_operator(self, score, categories):
        report = score(
            "function f(a: number) { if (a + 1 > a * 2) { return 1 } return 0 }",
            path="sample.ts",
            language="typescript",
        )
        assert prof.COMPLEX_CONDITIONAL not in categories(report)


class TestArguments:
    def test_excess_arguments_fires(self, score, find):
        report = score("def f(a, b, c, d, e):\n    return a\n")
        finding = find(report, prof.EXCESS_ARGUMENTS)
        assert finding is not None
        assert finding.details == "Arguments = 5"

    def test_self_is_not_an_argument(self, score, categories):
        report = score(
            "class C:\n    def f(self, a, b, c, d):\n        return a\n"
        )
        assert prof.EXCESS_ARGUMENTS not in categories(report)

    def test_constructor_reported_once_as_over_injection(self, score, categories):
        report = score(
            "class C:\n"
            "    def __init__(self, a, b, c, d, e, f):\n"
            "        self.a = a\n"
        )
        found = categories(report)
        assert prof.CONSTRUCTOR_OVER_INJECTION in found
        assert prof.EXCESS_ARGUMENTS not in found

    def test_destructured_object_is_one_parameter(self, score, categories):
        """React props are a single argument, however many keys they hold."""
        report = score(
            "export function T({ a, b, c, d, e, f, g }: Props) { return null }",
            path="T.tsx",
            language="typescript",
        )
        assert prof.EXCESS_ARGUMENTS not in categories(report)


class TestBrainMethod:
    def test_requires_all_three_conditions(self, score, categories):
        """Long but flat and simple is not a Brain Method."""
        body = "\n".join(f"    x{i} = {i}" for i in range(60))
        assert prof.BRAIN_METHOD not in categories(score(f"def f():\n{body}\n"))

    def test_fires_when_long_complex_and_nested(self, score, find):
        chunks = "\n".join(
            f"    if a == {i}:\n"
            f"        for j in range({i}):\n"
            f"            if j and a:\n"
            f"                for k in range(j):\n"
            f"                    total += k\n"
            for i in range(12)
        )
        report = score(f"def f(a):\n    total = 0\n{chunks}\n    return total\n")
        finding = find(report, prof.BRAIN_METHOD)
        assert finding is not None
        assert "cc =" in finding.details and "nesting depth =" in finding.details


class TestTypeSmells:
    def test_string_heavy_arguments(self, score, find):
        report = score(
            "def f(a: str, b: str, c: str, d: str) -> None:\n    return None\n"
        )
        assert find(report, prof.STRING_HEAVY_ARGUMENTS) is not None

    def test_primitive_obsession(self, score, find):
        report = score(
            "def f(a: int, b: int, c: str, d: bool) -> None:\n    return None\n"
        )
        finding = find(report, prof.PRIMITIVE_OBSESSION)
        assert finding is not None
        assert "100%" in finding.details

    def test_unannotated_parameters_are_not_guessed(self, score, categories):
        """Being untyped is not evidence of bad design; it is absence of evidence."""
        report = score("def f(a, b, c, d):\n    return a\n")
        assert prof.PRIMITIVE_OBSESSION not in categories(report)

    def test_domain_types_are_clean(self, score, categories):
        report = score(
            "def f(order: Order, customer: Customer, money: Money, id: TenderId):\n"
            "    return order\n"
        )
        assert prof.PRIMITIVE_OBSESSION not in categories(report)


class TestCodeDuplication:
    def test_detects_structural_clones(self, score, find):
        template = (
            "def {name}({arg}):\n"
            "    {v} = {arg}.copy()\n"
            "    if not {v}.get('{key}'):\n"
            "        raise ValueError('{key}')\n"
            "    record = Model.objects.create(**{v})\n"
            "    log.info('%s', record.id)\n"
            "    notify(record)\n"
            "    audit(record)\n"
            "    cache.delete('list')\n"
            "    metrics.increment('x')\n"
            "    record.refresh_from_db()\n"
            "    enqueue(record.id)\n"
            "    touch(record)\n"
            "    reindex(record)\n"
            "    publish(record)\n"
            "    return serialize(record)\n"
        )
        source = template.format(name="create", arg="payload", v="data", key="name")
        source += "\n\n" + template.format(
            name="update", arg="values", v="body", key="title"
        )
        finding = find(score(source), prof.CODE_DUPLICATION)
        assert finding is not None
        assert "similar to" in finding.details

    def test_short_functions_are_not_compared(self, score, categories):
        source = "def a(x):\n    return x + 1\n\n\ndef b(y):\n    return y + 1\n"
        assert prof.CODE_DUPLICATION not in categories(score(source))


class TestAssertionRules:
    WALL = "def test_x():\n" + "\n".join(
        f"    assert value_{i} == {i}" for i in range(12)
    )

    def test_large_assertion_block_in_test_file(self, score, find):
        report = score(self.WALL, path="test_thing.py")
        finding = find(report, prof.LARGE_ASSERTION_BLOCKS)
        assert finding is not None
        assert finding.details == "12 consecutive assertions"

    def test_not_applied_outside_test_files(self, score, categories):
        report = score(self.WALL, path="production.py")
        assert prof.LARGE_ASSERTION_BLOCKS not in categories(report)

    def test_duplicated_assertion_blocks(self, score, find):
        block = "    assert r.status == 200\n    assert r.body == 'ok'\n    assert r.h == {}\n"
        source = f"def test_a():\n    r = call()\n{block}\ndef test_b():\n    r = call()\n{block}"
        report = score(source, path="test_dup.py")
        assert find(report, prof.DUPLICATED_ASSERTION_BLOCKS) is not None

    def test_typescript_expect_style(self, score, find):
        calls = "\n".join(f"  expect(a{i}).toBe({i});" for i in range(12))
        report = score(
            f"it('checks', () => {{\n{calls}\n}});",
            path="thing.spec.ts",
            language="typescript",
        )
        assert find(report, prof.LARGE_ASSERTION_BLOCKS) is not None


class TestModuleRules:
    def test_file_size(self, score, find):
        source = "\n".join(f"x{i} = {i}" for i in range(600))
        finding = find(score(source), prof.FILE_SIZE)
        assert finding is not None
        assert "lines" in finding.details

    def test_function_count(self, score, find):
        source = "\n\n".join(f"def f{i}():\n    return {i}" for i in range(25))
        assert find(score(source), prof.FUNCTION_COUNT) is not None

    def test_overall_complexity_needs_three_functions(self, score, categories):
        branches = "\n".join(f"    if x == {i}: return {i}" for i in range(10))
        source = f"def a(x):\n{branches}\n\n\ndef b(x):\n{branches}\n"
        assert prof.OVERALL_COMPLEXITY not in categories(score(source))

    def test_declaration_size_typescript_only(self, score, find):
        source = "\n".join(
            f"export interface I{i} {{\n  a: string;\n  b: number;\n}}" for i in range(90)
        )
        report = score(source, path="types.ts", language="typescript")
        assert find(report, prof.DECLARATION_SIZE) is not None


class TestLowCohesion:
    def test_fires_on_two_unrelated_groups(self, score, find):
        report = score(
            "class Mixed:\n"
            "    def __init__(self):\n"
            "        self.orders = []\n"
            "        self.emails = []\n"
            "    def add_order(self, o):\n"
            "        self.orders.append(o)\n"
            "    def count_orders(self):\n"
            "        return len(self.orders)\n"
            "    def add_email(self, e):\n"
            "        self.emails.append(e)\n"
            "    def count_emails(self):\n"
            "        return len(self.emails)\n"
        )
        finding = find(report, prof.LOW_COHESION)
        assert finding is not None
        assert "LCOM4 = 2" in finding.details

    def test_quiet_on_cohesive_class(self, score, categories):
        report = score(
            "class Money:\n"
            "    def __init__(self, amount):\n"
            "        self.amount = amount\n"
            "    def doubled(self):\n"
            "        return self.amount * 2\n"
            "    def halved(self):\n"
            "        return self.amount / 2\n"
        )
        assert prof.LOW_COHESION not in categories(report)

    @pytest.mark.parametrize(
        "source",
        [
            pytest.param(
                "from abc import ABC, abstractmethod\n"
                "class Base(ABC):\n"
                "    @abstractmethod\n"
                "    def a(self, x: str) -> str: ...\n"
                "    @abstractmethod\n"
                "    def b(self, y: str) -> str: ...\n",
                id="abstract-base-class",
            ),
            pytest.param(
                "from typing import Protocol\n"
                "class P(Protocol):\n"
                "    def a(self) -> str: ...\n"
                "    def b(self) -> str: ...\n",
                id="protocol",
            ),
            pytest.param(
                "class Stub:\n"
                "    def a(self):\n"
                "        raise NotImplementedError\n"
                "    def b(self):\n"
                "        raise NotImplementedError\n",
                id="all-stubs",
            ),
        ],
    )
    def test_interfaces_are_exempt(self, score, categories, source):
        """An interface has no shared state by definition; that is not a defect."""
        assert prof.LOW_COHESION not in categories(score(source))

    def test_test_classes_are_exempt(self, score, categories):
        """Independent test methods are the goal, not a cohesion failure."""
        methods = "\n".join(
            f"    def test_{i}(self):\n        assert thing_{i}() == {i}"
            for i in range(6)
        )
        report = score(f"class TestThings:\n{methods}\n", path="test_things.py")
        assert prof.LOW_COHESION not in categories(report)
