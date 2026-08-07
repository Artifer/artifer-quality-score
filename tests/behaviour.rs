//! Behavioural tests for the detector catalogue and scoring.
//!
//! These encode the design decisions, not just the happy paths. The negative cases
//! matter more than the positive ones: a detector that flags correct code is worse than
//! one that misses a problem, because a false positive teaches a model to make the code
//! worse.
//!
//! Mirrors `tests/test_detectors.py` in the Python reference implementation, so a
//! divergence shows up here as well as in the differential harness.

use aqs::analyze::analyze_source;
use aqs::languages::spec_for_name;
use aqs::models::FileReport;
use aqs::profile::{self as prof, Profile};
use std::collections::HashSet;

fn score(source: &str, path: &str, language: &str) -> FileReport {
    let spec = spec_for_name(language).expect("known language");
    analyze_source(path, source.as_bytes(), spec, &Profile::default())
}

fn py(source: &str) -> FileReport {
    score(source, "sample.py", "python")
}

fn categories(report: &FileReport) -> HashSet<String> {
    report.all_findings().iter().map(|f| f.category.clone()).collect()
}

fn detail(report: &FileReport, category: &str) -> Option<String> {
    report
        .all_findings()
        .iter()
        .find(|f| f.category == category)
        .map(|f| f.details.clone())
}

fn branches(n: usize) -> String {
    (0..n).map(|i| format!("    if x == {i}: return {i}\n")).collect()
}

// --------------------------------------------------------------------------- //
// Function level
// --------------------------------------------------------------------------- //

#[test]
fn complex_method_fires_above_threshold() {
    let report = py(&format!("def f(x):\n{}    return None\n", branches(12)));
    assert_eq!(detail(&report, prof::COMPLEX_METHOD).as_deref(), Some("cc = 13"));
}

#[test]
fn simple_function_is_clean() {
    let report = py("def f(x):\n    if x:\n        return 1\n    return 0\n");
    assert!(report.all_findings().is_empty());
    assert_eq!(report.score, 10.0);
}

#[test]
fn named_nested_function_is_measured_separately() {
    let inner: String =
        (0..14).map(|i| format!("        if x == {i}: return {i}\n")).collect();
    let report = py(&format!("def outer(x):\n    def inner(x):\n{inner}    return inner\n"));
    let flagged: HashSet<String> = report
        .all_findings()
        .iter()
        .filter(|f| f.category == prof::COMPLEX_METHOD)
        .map(|f| f.function.clone().unwrap_or_default())
        .collect();
    assert_eq!(flagged, HashSet::from(["inner".to_string()]));
}

#[test]
fn docstrings_and_comments_are_not_lines_of_code() {
    // Deleting documentation must never be a way to improve the score.
    let comments: String = (0..80).map(|i| format!("    # explanation line {i}\n")).collect();
    let report = py(&format!(
        "def f():\n    \"\"\"A long docstring.\n\n    More prose here.\n    \"\"\"\n{comments}    return 1\n"
    ));
    assert!(!categories(&report).contains(prof::LARGE_METHOD));
}

#[test]
fn deep_nesting_fires_at_depth_four() {
    let report = py(
        "def f(a):\n    if a:\n        for i in a:\n            while i:\n                if i > 1:\n                    return i\n",
    );
    assert_eq!(
        detail(&report, prof::DEEP_NESTED_COMPLEXITY).as_deref(),
        Some("Nesting depth = 4 conditionals")
    );
}

#[test]
fn elif_chain_is_flat() {
    let chain: String = (0..8)
        .map(|i| {
            let keyword = if i == 0 { "if" } else { "elif" };
            format!("    {keyword} x == {i}:\n        return {i}\n")
        })
        .collect();
    let report = py(&format!("def f(x):\n{chain}"));
    assert!(!categories(&report).contains(prof::DEEP_NESTED_COMPLEXITY));
}

#[test]
fn else_if_chain_is_flat_in_typescript() {
    let chain: Vec<String> =
        (0..8).map(|i| format!("if (x === {i}) {{ return {i}; }}")).collect();
    let report = score(
        &format!("function f(x: number) {{ {} return -1; }}", chain.join(" else ")),
        "sample.ts",
        "typescript",
    );
    assert!(!categories(&report).contains(prof::DEEP_NESTED_COMPLEXITY));
}

#[test]
fn bumpy_road_fires_on_two_nested_chunks() {
    let report = py(
        "def f(a, b):\n    if a:\n        for i in a:\n            print(i)\n    if b:\n        for j in b:\n            print(j)\n",
    );
    assert_eq!(detail(&report, prof::BUMPY_ROAD).as_deref(), Some("bumps = 2"));
}

#[test]
fn single_chunk_is_not_a_bumpy_road() {
    let report = py("def f(a):\n    if a:\n        for i in a:\n            print(i)\n");
    assert!(!categories(&report).contains(prof::BUMPY_ROAD));
}

#[test]
fn complex_conditional_needs_two_operators() {
    let two = py("def f(a, b, c):\n    if a and b and c:\n        return 1\n");
    assert_eq!(
        detail(&two, prof::COMPLEX_CONDITIONAL).as_deref(),
        Some("1 complex conditional expression")
    );
    let one = py("def f(a, b):\n    if a and b:\n        return 1\n");
    assert!(!categories(&one).contains(prof::COMPLEX_CONDITIONAL));
}

#[test]
fn arithmetic_is_not_a_logical_operator() {
    let report = score(
        "function f(a: number) { if (a + 1 > a * 2) { return 1 } return 0 }",
        "sample.ts",
        "typescript",
    );
    assert!(!categories(&report).contains(prof::COMPLEX_CONDITIONAL));
}

#[test]
fn self_is_not_an_argument() {
    let report = py("class C:\n    def f(self, a, b, c, d):\n        return a\n");
    assert!(!categories(&report).contains(prof::EXCESS_ARGUMENTS));
}

#[test]
fn constructor_reported_once_as_over_injection() {
    let report =
        py("class C:\n    def __init__(self, a, b, c, d, e, f):\n        self.a = a\n");
    let found = categories(&report);
    assert!(found.contains(prof::CONSTRUCTOR_OVER_INJECTION));
    assert!(!found.contains(prof::EXCESS_ARGUMENTS));
}

#[test]
fn destructured_props_are_one_argument() {
    // React props are a single argument, however many keys they hold.
    let report = score(
        "export function T({ a, b, c, d, e, f, g }: Props) { return null }",
        "T.tsx",
        "typescript",
    );
    assert!(!categories(&report).contains(prof::EXCESS_ARGUMENTS));
}

#[test]
fn brain_method_requires_all_three_conditions() {
    // Long but flat and simple is not a Brain Method.
    let body: String = (0..60).map(|i| format!("    x{i} = {i}\n")).collect();
    let flat = py(&format!("def f():\n{body}"));
    assert!(!categories(&flat).contains(prof::BRAIN_METHOD));

    let chunks: String = (0..12)
        .map(|i| {
            format!(
                "    if a == {i}:\n        for j in range({i}):\n            if j and a:\n                for k in range(j):\n                    total += k\n"
            )
        })
        .collect();
    let brain = py(&format!("def f(a):\n    total = 0\n{chunks}    return total\n"));
    let details = detail(&brain, prof::BRAIN_METHOD).expect("brain method fires");
    assert!(details.contains("cc =") && details.contains("nesting depth ="));
}

#[test]
fn unannotated_parameters_are_not_guessed() {
    // Being untyped is absence of evidence, not evidence of bad design.
    let report = py("def f(a, b, c, d):\n    return a\n");
    assert!(!categories(&report).contains(prof::PRIMITIVE_OBSESSION));
}

#[test]
fn domain_types_are_clean() {
    let report = py(
        "def f(order: Order, customer: Customer, money: Money, id: TenderId):\n    return order\n",
    );
    assert!(!categories(&report).contains(prof::PRIMITIVE_OBSESSION));
}

#[test]
fn primitive_obsession_fires_on_all_primitive_signature() {
    let report = py("def f(a: int, b: int, c: str, d: bool) -> None:\n    return None\n");
    let details = detail(&report, prof::PRIMITIVE_OBSESSION).expect("fires");
    assert!(details.contains("100%"), "{details}");
}

// --------------------------------------------------------------------------- //
// File / module level
// --------------------------------------------------------------------------- //

#[test]
fn detects_structural_clones_ignoring_names() {
    let template = |name: &str, arg: &str, var: &str, key: &str| {
        format!(
            "def {name}({arg}):\n    {var} = {arg}.copy()\n    if not {var}.get('{key}'):\n        raise ValueError('{key}')\n    record = Model.objects.create(**{var})\n    log.info('%s', record.id)\n    notify(record)\n    audit(record)\n    cache.delete('list')\n    metrics.increment('x')\n    record.refresh_from_db()\n    enqueue(record.id)\n    touch(record)\n    reindex(record)\n    publish(record)\n    return serialize(record)\n"
        )
    };
    let source = format!(
        "{}\n\n{}",
        template("create", "payload", "data", "name"),
        template("update", "values", "body", "title")
    );
    let details = detail(&py(&source), prof::CODE_DUPLICATION).expect("duplication fires");
    assert!(details.contains("similar to"), "{details}");
}

#[test]
fn short_functions_are_not_compared_for_duplication() {
    let report = py("def a(x):\n    return x + 1\n\n\ndef b(y):\n    return y + 1\n");
    assert!(!categories(&report).contains(prof::CODE_DUPLICATION));
}

#[test]
fn assertion_rules_only_apply_to_test_files() {
    let wall: String =
        (0..12).map(|i| format!("    assert value_{i} == {i}\n")).collect();
    let source = format!("def test_x():\n{wall}");

    let test_file = score(&source, "test_thing.py", "python");
    assert_eq!(
        detail(&test_file, prof::LARGE_ASSERTION_BLOCKS).as_deref(),
        Some("12 consecutive assertions")
    );

    let production = score(&source, "production.py", "python");
    assert!(!categories(&production).contains(prof::LARGE_ASSERTION_BLOCKS));
}

#[test]
fn duplicated_assertion_blocks_are_detected() {
    let block = "    assert r.status == 200\n    assert r.body == 'ok'\n    assert r.h == {}\n";
    let source =
        format!("def test_a():\n    r = call()\n{block}\ndef test_b():\n    r = call()\n{block}");
    let report = score(&source, "test_dup.py", "python");
    assert!(categories(&report).contains(prof::DUPLICATED_ASSERTION_BLOCKS));
}

#[test]
fn typescript_expect_style_assertions() {
    let calls: String = (0..12).map(|i| format!("  expect(a{i}).toBe({i});\n")).collect();
    let report = score(
        &format!("it('checks', () => {{\n{calls}}});"),
        "thing.spec.ts",
        "typescript",
    );
    assert!(categories(&report).contains(prof::LARGE_ASSERTION_BLOCKS));
}

#[test]
fn overall_complexity_needs_three_functions() {
    let body = branches(10);
    let report = py(&format!("def a(x):\n{body}\n\ndef b(x):\n{body}"));
    assert!(!categories(&report).contains(prof::OVERALL_COMPLEXITY));
}

#[test]
fn file_and_function_count_rules_fire() {
    let big: String = (0..600).map(|i| format!("x{i} = {i}\n")).collect();
    assert!(categories(&py(&big)).contains(prof::FILE_SIZE));

    let many: String = (0..25).map(|i| format!("def f{i}():\n    return {i}\n\n")).collect();
    assert!(categories(&py(&many)).contains(prof::FUNCTION_COUNT));
}

#[test]
fn declaration_size_is_typescript_only() {
    let source: String = (0..90)
        .map(|i| format!("export interface I{i} {{\n  a: string;\n  b: number;\n}}\n"))
        .collect();
    let report = score(&source, "types.ts", "typescript");
    assert!(categories(&report).contains(prof::DECLARATION_SIZE));
}

// --------------------------------------------------------------------------- //
// Low Cohesion — the rule with the most false-positive traps
// --------------------------------------------------------------------------- //

#[test]
fn low_cohesion_fires_on_two_unrelated_groups() {
    let report = py(
        "class Mixed:\n    def __init__(self):\n        self.orders = []\n        self.emails = []\n    def add_order(self, o):\n        self.orders.append(o)\n    def count_orders(self):\n        return len(self.orders)\n    def add_email(self, e):\n        self.emails.append(e)\n    def count_emails(self):\n        return len(self.emails)\n",
    );
    let details = detail(&report, prof::LOW_COHESION).expect("fires");
    assert!(details.contains("LCOM4 = 2"), "{details}");
}

#[test]
fn cohesive_class_is_clean() {
    let report = py(
        "class Money:\n    def __init__(self, amount):\n        self.amount = amount\n    def doubled(self):\n        return self.amount * 2\n    def halved(self):\n        return self.amount / 2\n",
    );
    assert!(!categories(&report).contains(prof::LOW_COHESION));
}

#[test]
fn interfaces_are_exempt_from_cohesion() {
    // An interface has no shared state by definition; that is not a defect.
    let cases = [
        "from abc import ABC, abstractmethod\nclass Base(ABC):\n    @abstractmethod\n    def a(self, x: str) -> str: ...\n    @abstractmethod\n    def b(self, y: str) -> str: ...\n",
        "from typing import Protocol\nclass P(Protocol):\n    def a(self) -> str: ...\n    def b(self) -> str: ...\n",
        "class Stub:\n    def a(self):\n        raise NotImplementedError\n    def b(self):\n        raise NotImplementedError\n",
    ];
    for source in cases {
        assert!(
            !categories(&py(source)).contains(prof::LOW_COHESION),
            "should be exempt: {source}"
        );
    }
}

#[test]
fn super_only_overrides_are_exempt_from_cohesion() {
    // A DRF/Django view whose extra methods exist purely to hang a decorator on and
    // forward to super(). They hold no state by design, so counting each as its own
    // responsibility condemns a perfectly ordinary class.
    let report = py(
        "class MyProfileView(generics.RetrieveUpdateAPIView):\n\
         \x20   def get_object(self):\n\
         \x20       return Profile.objects.get(user=self.request.user)\n\
         \x20   def perform_update(self, serializer):\n\
         \x20       log(user=self.request.user)\n\
         \x20   @extend_schema(summary='Get')\n\
         \x20   def get(self, request, *args, **kwargs):\n\
         \x20       return super().get(request, *args, **kwargs)\n\
         \x20   @extend_schema(summary='Put')\n\
         \x20   def put(self, request, *args, **kwargs):\n\
         \x20       return super().put(request, *args, **kwargs)\n\
         \x20   @extend_schema(summary='Patch')\n\
         \x20   def patch(self, request, *args, **kwargs):\n\
         \x20       return super().patch(request, *args, **kwargs)\n",
    );
    assert!(
        !categories(&report).contains(prof::LOW_COHESION),
        "super()-only overrides should not count as responsibilities: {:?}",
        report.all_findings().iter().map(|f| &f.details).collect::<Vec<_>>()
    );
}

#[test]
fn a_method_that_does_more_than_delegate_still_counts() {
    // The exemption must be narrow: real logic alongside the super() call counts.
    let report = py(
        "class C(Base):\n\
         \x20   def a(self):\n\
         \x20       return self.thing\n\
         \x20   def b(self, request):\n\
         \x20       audit(request)\n\
         \x20       return super().b(request)\n",
    );
    assert!(categories(&report).contains(prof::LOW_COHESION));
}

#[test]
fn django_app_tests_py_is_recognised_as_a_test_file() {
    // `manage.py startapp` puts an app's tests in a single `app/tests.py`.
    let methods: String = (0..4)
        .map(|i| format!("    def test_{i}(self):\n        assert thing_{i}() == {i}\n"))
        .collect();
    let source = format!("class TestVacancy:\n{methods}");
    for path in ["tests.py", "vacancies/tests.py"] {
        let report = score(&source, path, "python");
        assert!(
            !categories(&report).contains(prof::LOW_COHESION),
            "{path} should be treated as a test file"
        );
    }
}

#[test]
fn test_classes_are_exempt_from_cohesion() {
    // Independent test methods are the goal, not a cohesion failure.
    let methods: String = (0..6)
        .map(|i| format!("    def test_{i}(self):\n        assert thing_{i}() == {i}\n"))
        .collect();
    let report = score(
        &format!("class TestThings:\n{methods}"),
        "test_things.py",
        "python",
    );
    assert!(!categories(&report).contains(prof::LOW_COHESION));
}

// --------------------------------------------------------------------------- //
// Scoring
// --------------------------------------------------------------------------- //

#[test]
fn penalties_sum_to_the_deduction() {
    let report = py(&format!("def f(x):\n{}", branches(14)));
    let total: f64 = report.all_findings().iter().map(|f| f.penalty).sum();
    assert_eq!(report.score, aqs::models::round4(10.0 - total));
}

#[test]
fn score_floors_at_one() {
    let chunk = "    if a:\n        for b in a:\n            while b:\n                if b and a or b:\n                    for c in b:\n                        if c:\n                            a += c\n";
    let source: String = (0..20)
        .map(|i| format!("def f{i}(a, b, c, d, e, f):\n{}\n", chunk.repeat(4)))
        .collect();
    assert_eq!(py(&source).score, 1.0);
}

#[test]
fn per_file_cap_limits_one_rule() {
    let body = branches(12);
    let source: String =
        (0..40).map(|i| format!("def f{i}(x):\n{body}\n")).collect();
    let report = py(&source);
    let spent: f64 = report
        .all_findings()
        .iter()
        .filter(|f| f.category == prof::COMPLEX_METHOD)
        .map(|f| f.penalty)
        .sum();
    let cap = Profile::default().rule(prof::COMPLEX_METHOD).file_cap;
    assert!(spent <= cap + 1e-6, "spent {spent} exceeds cap {cap}");
}

#[test]
fn worst_offender_keeps_full_penalty_when_cap_binds() {
    // Fixing the worst function must always move the score.
    let mild = branches(12);
    let severe = branches(120);
    let mut source: String =
        (0..30).map(|i| format!("def mild{i}(x):\n{mild}\n")).collect();
    source.push_str(&format!("def severe(x):\n{severe}\n"));

    let report = py(&source);
    let severe_penalty = report
        .all_findings()
        .iter()
        .find(|f| {
            f.category == prof::COMPLEX_METHOD && f.function.as_deref() == Some("severe")
        })
        .map(|f| f.penalty)
        .expect("severe function is flagged");
    assert_eq!(severe_penalty, Profile::default().rule(prof::COMPLEX_METHOD).cap);
}

#[test]
fn repeated_analysis_is_byte_identical() {
    // The whole gate depends on this: same bytes in, same JSON out.
    let source = "def f(a, b, c, d, e):\n    if a and b and c:\n        return 1\n";
    let first = serde_json::to_string(&py(source)).expect("serialisable");
    let second = serde_json::to_string(&py(source)).expect("serialisable");
    assert_eq!(first, second);
}
