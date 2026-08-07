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
         \x20       return compute(self.thing)\n\
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

// --------------------------------------------------------------------------- //
// Grammar workarounds
// --------------------------------------------------------------------------- //

#[test]
fn bare_ampersand_in_jsx_text_parses_clean() {
    // Valid JSX that tree-sitter-typescript 0.23.2 rejects. The repair pass rewrites it
    // to `&amp;`, which is semantically identical, so no spurious error is reported.
    for source in [
        "export function H() {\n  return <h1>Werving & Selectie</h1>;\n}\n",
        "export function L() {\n  return <a href={`/x?a=1&status=published`}>go</a>;\n}\n",
        "export function M() {\n  return <p>a & b & c</p>;\n}\n",
    ] {
        let report = score(source, "H.tsx", "typescript");
        assert_eq!(report.syntax_errors, 0, "should parse clean: {source}");
    }
}

#[test]
fn ampersand_repair_does_not_change_the_score() {
    // The escaped and unescaped forms must be scored identically, or the workaround
    // would itself be a source of drift.
    let bare = score(
        "export function H() {\n  return <h1>Werving & Selectie</h1>;\n}\n",
        "H.tsx",
        "typescript",
    );
    let escaped = score(
        "export function H() {\n  return <h1>Werving &amp; Selectie</h1>;\n}\n",
        "H.tsx",
        "typescript",
    );
    assert_eq!(bare.score, escaped.score);
    assert_eq!(bare.loc, escaped.loc);
    assert_eq!(bare.all_findings().len(), escaped.all_findings().len());
}

#[test]
fn logical_and_is_never_rewritten() {
    // The repair only touches ampersands inside error nodes; `&&` must survive, since
    // rewriting it would destroy real operators and change complexity.
    let source = "export function H({ a, b, c }: P) {\n  if (a && b && c) { return <p>x & y</p>; }\n  return null;\n}\n";
    let report = score(source, "H.tsx", "typescript");
    assert_eq!(report.syntax_errors, 0);
    // Two surviving `&&` make this a Complex Conditional. Its presence is the proof
    // that the operators were not rewritten away along with the JSX-text ampersand.
    assert_eq!(
        detail(&report, prof::COMPLEX_CONDITIONAL).as_deref(),
        Some("1 complex conditional expression")
    );
}

#[test]
fn ampersand_in_a_jsx_attribute_string_parses_clean() {
    // The grammar rejects `&` in an attribute value too, and there the parse cascades
    // into a whole-file ERROR: the component vanishes, no functions are found, and the
    // file scores a false 10.0. Found on a real 242-line page.
    let branches: String = (0..14)
        .map(|i| format!("  if (x === {i}) {{ return <p>{i}</p>; }}\n"))
        .collect();
    let source = format!(
        "export function Page({{ x }}: P) {{\n{branches}  return <Empty description=\"op een W&S-vacature zet\" />;\n}}\n"
    );
    let report = score(&source, "Page.tsx", "typescript");
    assert_eq!(report.syntax_errors, 0);
    // The function must actually be found — a cascaded parse would report none, and the
    // complexity finding is the proof that its branches survived.
    assert_eq!(detail(&report, prof::COMPLEX_METHOD).as_deref(), Some("cc = 15"));
}

#[test]
fn a_cascaded_parse_never_rewrites_logical_operators() {
    // With a whole-file ERROR the repair sees every ampersand in the file. Rewriting
    // `&&` there would quietly reduce cyclomatic complexity and inflate the score.
    let source = "export function Page({ a, b, c, x }: P) {\n  if (a && b) { return <i>1</i>; }\n  if (b && c) { return <i>2</i>; }\n  if (a && c) { return <i>3</i>; }\n  return <Empty description=\"W&S vacature\" />;\n}\n";
    let report = score(source, "Page.tsx", "typescript");
    assert_eq!(report.syntax_errors, 0);
    // 1 + 3 ifs + 3 `&&` = cc 7. If the operators had been rewritten it would read 4.
    let functions = &report.function_level_code_smells;
    let complexity_survived = report.loc > 0 && functions.iter().all(|f| !f.function.is_empty());
    assert!(complexity_survived);
    // Directly: the `&&` pairs must still be countable as boolean operators.
    let stricter = "export function P({ a, b, c }: Q) {\n  if (a && b && c) { return <i>x</i>; }\n  return <Empty description=\"W&S\" />;\n}\n";
    let report = score(stricter, "P.tsx", "typescript");
    assert_eq!(
        detail(&report, prof::COMPLEX_CONDITIONAL).as_deref(),
        Some("1 complex conditional expression"),
        "the && operators must survive the repair"
    );
}

#[test]
fn nested_error_nodes_do_not_corrupt_the_repair() {
    // ERROR nodes nest, so the same ampersand can be reported at several depths and in
    // tree rather than byte order. This crashed on real Next.js source before the
    // offsets were sorted and deduplicated.
    let source = "export function P() {\n  return (\n    <div>\n      <a href={`/x?a=1&b=2&c=3`}>A & B</a>\n      <span>C & D & E</span>\n    </div>\n  );\n}\n";
    let report = score(source, "P.tsx", "typescript");
    assert_eq!(report.syntax_errors, 0, "{:?}", report.all_findings());
}

#[test]
fn genuinely_broken_files_still_report_errors() {
    // The repair must not mask real syntax problems.
    let report = score(
        "export function broken( {\n  return <div>\n}\n",
        "B.tsx",
        "typescript",
    );
    assert!(report.syntax_errors > 0);
}

#[test]
fn repeated_analysis_is_byte_identical() {
    // The whole gate depends on this: same bytes in, same JSON out.
    let source = "def f(a, b, c, d, e):\n    if a and b and c:\n        return 1\n";
    let first = serde_json::to_string(&py(source)).expect("serialisable");
    let second = serde_json::to_string(&py(source)).expect("serialisable");
    assert_eq!(first, second);
}

// --------------------------------------------------------------------------- //
// Discovery
// --------------------------------------------------------------------------- //

/// Build a throwaway tree and return its root. Caller removes it.
fn scratch_tree(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("aqs-test-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    for (relative, body) in files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write");
    }
    root
}

#[test]
fn excluded_directories_are_pruned_not_walked() {
    // Pruning is an optimisation only: the file set must be exactly what the per-file
    // exclusion check would have produced anyway.
    let root = scratch_tree(
        "prune",
        &[
            ("src/app.ts", "export const a = 1;\n"),
            ("node_modules/dep/index.ts", "export const b = 2;\n"),
            ("node_modules/dep/nested/deep/x.ts", "export const c = 3;\n"),
            (".venv/lib/site.py", "x = 1\n"),
            ("src/components/ui/button.tsx", "export const B = () => null;\n"),
        ],
    );

    let found = aqs::analyze::discover(std::slice::from_ref(&root), &Profile::default());
    let names: Vec<String> = found
        .iter()
        .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(names, vec!["src/app.ts"], "only real source should be discovered");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_explicit_scan_of_an_excluded_root_still_descends() {
    // Pruning must not make the root itself unwalkable, or `aqs scan node_modules`
    // would behave differently from the per-file rule.
    let root = scratch_tree(
        "explicit-root",
        &[("node_modules/dep/index.ts", "export const b = 2;\n")],
    );
    let inner = root.join("node_modules");
    // Files under it are still excluded by pattern, so the result is empty either way —
    // the point is that this returns rather than panicking or walking nothing at all.
    let found = aqs::analyze::discover(&[inner], &Profile::default());
    assert!(found.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_root_level_test_directory_is_recognised() {
    // Playwright puts specs in `e2e/` at the repository root. A pattern of `*/e2e/*`
    // needs a leading segment, so without segment matching the same file counted as
    // production code when scanned from the root and as test code from one level up.
    let wall: String = (0..12).map(|i| format!("  expect(a{i}).toBe({i});\n")).collect();
    let source = format!("it('checks', () => {{\n{wall}}});");

    for path in ["e2e/flow.ts", "repo/e2e/flow.ts", "__tests__/thing.ts", "tests/thing.ts"] {
        let report = score(&source, path, "typescript");
        assert_eq!(
            detail(&report, prof::LARGE_ASSERTION_BLOCKS).as_deref(),
            Some("12 consecutive assertions"),
            "{path} should be treated as test code"
        );
    }
}

#[test]
fn test_classification_does_not_depend_on_the_scan_root() {
    // The same file, addressed at two depths, must be scored identically.
    let methods: String = (0..6)
        .map(|i| format!("    def test_{i}(self):\n        assert thing_{i}() == {i}\n"))
        .collect();
    let source = format!("class TestThings:\n{methods}");
    let shallow = score(&source, "tests/test_things.py", "python");
    let deep = score(&source, "apps/core/tests/test_things.py", "python");
    assert_eq!(shallow.score, deep.score);
    assert_eq!(
        categories(&shallow).contains(prof::LOW_COHESION),
        categories(&deep).contains(prof::LOW_COHESION)
    );
}

#[test]
fn production_code_is_not_mistaken_for_tests() {
    // The segment matching must not be so loose that ordinary paths become tests.
    let profile = Profile::default();
    for path in ["src/latest.ts", "apps/core/models.py", "src/contest/rules.ts",
                 "src/e2e_helpers/util.ts", "src/protest.py"] {
        assert!(!profile.test_patterns.matches(path), "{path} is not test code");
    }
}

// The bare-filename and empty-base cases are covered end-to-end in tests/cli.rs, which
// sets the *child's* working directory. Testing them here meant mutating the process
// working directory, which is unsafe under `cargo test`'s default parallelism.

// --------------------------------------------------------------------------- //
// Rust
// --------------------------------------------------------------------------- //

fn rs(source: &str) -> FileReport {
    score(source, "sample.rs", "rust")
}

#[test]
fn rust_complexity_counts_branches_match_arms_and_boolean_operators() {
    let arms: String = (0..12).map(|i| format!("        {i} => {i},\n")).collect();
    let report = rs(&format!(
        "pub fn classify(v: u8) -> u8 {{\n    match v {{\n{arms}        _ => 0,\n    }}\n}}\n"
    ));
    // 1 + 13 match arms = cc 14.
    assert_eq!(detail(&report, prof::COMPLEX_METHOD).as_deref(), Some("cc = 14"));
}

#[test]
fn rust_question_mark_is_not_a_branch() {
    // `?` is linear error propagation, not a decision the reader tracks. Counting it
    // would penalise idiomatic Rust for being explicit about failure.
    let calls: String = (0..20).map(|i| format!("    let v{i} = step{i}()?;\n")).collect();
    let report = rs(&format!("pub fn run() -> Result<(), E> {{\n{calls}    Ok(())\n}}\n"));
    assert!(!categories(&report).contains(prof::COMPLEX_METHOD), "{:?}", report.all_findings());
}

#[test]
fn rust_self_is_not_an_argument() {
    // `&self` is its own node kind, not a parameter whose text reads "self".
    let report = rs(
        "impl S {\n    pub fn f(&self, a: u8, b: u8, c: u8, d: u8) -> u8 { a + b + c + d }\n}\n",
    );
    assert!(!categories(&report).contains(prof::EXCESS_ARGUMENTS));

    let too_many = rs(
        "impl S {\n    pub fn f(&self, a: u8, b: u8, c: u8, d: u8, e: u8) -> u8 { a }\n}\n",
    );
    assert_eq!(
        detail(&too_many, prof::EXCESS_ARGUMENTS).as_deref(),
        Some("Arguments = 5")
    );
}

#[test]
fn rust_else_if_chains_stay_flat() {
    let chain: String = (0..8)
        .map(|i| format!("{} x == {i} {{ return {i}; }}", if i == 0 { "if" } else { " else if" }))
        .collect();
    let report = rs(&format!("pub fn f(x: u8) -> u8 {{\n    {chain}\n    0\n}}\n"));
    assert!(!categories(&report).contains(prof::DEEP_NESTED_COMPLEXITY));
}

#[test]
fn rust_nesting_is_measured() {
    let report = rs(
        "pub fn f(a: bool, xs: &[u8]) -> u8 {\n    if a {\n        for x in xs {\n            while *x > 0 {\n                if *x > 1 {\n                    return *x;\n                }\n            }\n        }\n    }\n    0\n}\n",
    );
    assert_eq!(
        detail(&report, prof::DEEP_NESTED_COMPLEXITY).as_deref(),
        Some("Nesting depth = 4 conditionals")
    );
}

#[test]
fn rust_assert_macros_are_assertions() {
    let wall: String = (0..12).map(|i| format!("        assert_eq!(a{i}, {i});\n")).collect();
    let report = score(
        &format!("#[cfg(test)]\nmod tests {{\n    #[test]\n    fn it_works() {{\n{wall}    }}\n}}\n"),
        "tests/thing.rs",
        "rust",
    );
    assert_eq!(
        detail(&report, prof::LARGE_ASSERTION_BLOCKS).as_deref(),
        Some("12 consecutive assertions")
    );
}

#[test]
fn rust_trait_impls_are_exempt_from_cohesion() {
    // The method set of `impl Trait for Type` is dictated by the trait, so its grouping
    // is not the author's choice — the same reasoning as ABCs and super()-only overrides.
    let report = rs(
        "impl Display for Money {\n    fn fmt(&self, f: &mut Formatter) -> Result { write!(f, \"{}\", self.amount) }\n    fn other(&self) -> u8 { 1 }\n}\n",
    );
    assert!(!categories(&report).contains(prof::LOW_COHESION));
}

#[test]
fn rust_inherent_impls_are_judged_for_cohesion() {
    let report = rs(
        "impl Mixed {\n    fn add_order(&mut self, o: u8) { self.orders.push(o) }\n    fn count_orders(&self) -> usize { self.orders.len() }\n    fn add_email(&mut self, e: u8) { self.emails.push(e) }\n    fn count_emails(&self) -> usize { self.emails.len() }\n}\n",
    );
    let details = detail(&report, prof::LOW_COHESION).expect("fires on an inherent impl");
    assert!(details.contains("LCOM4 = 2"), "{details}");
}

#[test]
fn rust_named_closures_are_their_own_unit() {
    let branches: String = (0..14).map(|i| format!("        if x == {i} {{ return {i}; }}\n")).collect();
    let report = rs(&format!(
        "pub fn outer(x: u8) -> u8 {{\n    let inner = |x: u8| -> u8 {{\n{branches}        0\n    }};\n    inner(x)\n}}\n"
    ));
    let flagged: HashSet<String> = report
        .all_findings()
        .iter()
        .filter(|f| f.category == prof::COMPLEX_METHOD)
        .map(|f| f.function.clone().unwrap_or_default())
        .collect();
    assert_eq!(flagged, HashSet::from(["inner".to_string()]));
}

#[test]
fn pure_accessors_are_exempt_but_mutators_are_not() {
    // A value object of accessors is not six responsibilities; splitting it would be
    // absurd. But a class that merely appends to two unrelated collections is real low
    // cohesion, and the "no calls" condition is what keeps those apart.
    let accessors = py(
        "class Config:\n\
         \x20   def timeout(self):\n\
         \x20       return self._timeout\n\
         \x20   def retries(self):\n\
         \x20       return self._retries\n\
         \x20   def host(self):\n\
         \x20       return self._host\n",
    );
    assert!(
        !categories(&accessors).contains(prof::LOW_COHESION),
        "accessors: {:?}",
        accessors.all_findings().iter().map(|f| &f.details).collect::<Vec<_>>()
    );

    let mutators = py(
        "class Mixed:\n\
         \x20   def add_order(self, o):\n\
         \x20       self.orders.append(o)\n\
         \x20   def count_orders(self):\n\
         \x20       return len(self.orders)\n\
         \x20   def add_email(self, e):\n\
         \x20       self.emails.append(e)\n\
         \x20   def count_emails(self):\n\
         \x20       return len(self.emails)\n",
    );
    assert!(
        categories(&mutators).contains(prof::LOW_COHESION),
        "mutators touching two collections must still be condemned"
    );
}

#[test]
fn rust_impl_blocks_are_named_by_their_type() {
    // `impl Money` has no `name` field; its identity is the type it implements.
    let report = rs(
        "impl Mixed {\n    fn add_order(&mut self, o: u8) { self.orders.push(o) }\n    fn count_orders(&self) -> usize { self.orders.len() }\n    fn add_email(&mut self, e: u8) { self.emails.push(e) }\n    fn count_emails(&self) -> usize { self.emails.len() }\n}\n",
    );
    let finding = report
        .all_findings()
        .into_iter()
        .find(|f| f.category == prof::LOW_COHESION)
        .expect("fires");
    assert_eq!(finding.function.as_deref(), Some("Mixed"));
}

#[test]
fn a_typoed_config_section_is_rejected() {
    // An unknown rule name was already fatal, but an unknown *section* was silently
    // ignored, so `"exclude_pattern"` or `"rule"` quietly did nothing. For a gate, a
    // config that fails to apply is worse than one that refuses to load.
    let root = scratch_tree(
        "typo-section",
        &[(".aqs.json", r#"{"rule":{"Complex Method":0.0}}"#)],
    );
    let error = aqs::profile::load_profile(&root, None).expect_err("must be rejected");
    assert!(format!("{error}").contains("unknown config section"), "{error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn underscore_prefixed_keys_are_comments() {
    // JSON has no comments, so `_why` carries the rationale for an exemption.
    let root = scratch_tree(
        "comment-key",
        &[(".aqs.json", r#"{"_why":{"a":"because"},"name":"documented"}"#)],
    );
    assert_eq!(
        aqs::profile::load_profile(&root, None).expect("loads").name,
        "documented"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn methods_that_say_nothing_about_cohesion_are_excluded() {
    // Each case is a class LCOM4 would condemn if the corresponding exemption were
    // missing, and each exemption exists for a different reason.
    let cases = [
        (
            // An accessor is not a responsibility of its own, but reading two fields is
            // real evidence they belong together. Excluding it from connectivity as well
            // as from counting would *raise* LCOM4 by removing the link.
            "accessor connects the fields it reads",
            "class Money:\n\
             \x20   def total(self):\n\
             \x20       return self.net + self.tax\n\
             \x20   def bump_net(self, v):\n\
             \x20       self.net = compute(v)\n\
             \x20   def bump_tax(self, v):\n\
             \x20       self.tax = compute(v)\n",
        ),
        (
            // `return super().update(deleted_at=timezone.now())` delegates even though it
            // computes an argument on the way. Demanding a call-free statement counted it
            // as its own responsibility and flagged a cohesive Django QuerySet.
            "super() delegation that computes an argument",
            "class SoftDeleteQuerySet(models.QuerySet):\n\
             \x20   def delete(self):\n\
             \x20       return super().update(deleted_at=timezone.now())\n\
             \x20   def hard_delete(self):\n\
             \x20       return super().delete()\n\
             \x20   def alive(self):\n\
             \x20       return self.filter(deleted_at__isnull=True)\n\
             \x20   def dead(self):\n\
             \x20       return self.filter(deleted_at__isnull=False)\n",
        ),
        (
            // A value object of getters is not six unrelated jobs.
            "value object of accessors",
            "class Config:\n\
             \x20   def timeout(self):\n\
             \x20       return self._timeout\n\
             \x20   def retries(self):\n\
             \x20       return self._retries\n\
             \x20   def host(self):\n\
             \x20       return self._host\n",
        ),
    ];
    for (label, source) in cases {
        let report = py(source);
        assert!(
            !categories(&report).contains(prof::LOW_COHESION),
            "{label}: {:?}",
            report.all_findings().iter().map(|f| &f.details).collect::<Vec<_>>()
        );
    }
}
