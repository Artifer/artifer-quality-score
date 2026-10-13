//! Where each rule starts to fire, and exactly what it then reports.
//!
//! The behavioural suites show that a rule fires on code that is clearly over the line.
//! These pin the line itself: a value *at* the threshold is fine, one past it is not, and
//! a compound rule needs every one of its conditions.

mod support;

use aqs::models::{FileReport, Finding};
use aqs::profile::{self as prof, Profile};
use support::{detail, py, score, score_with};

fn profile_with(overrides: &[(&str, f64)]) -> Profile {
    let mut profile = Profile::default();
    for (key, value) in overrides {
        profile.thresholds.insert((*key).to_string(), *value);
    }
    profile
}

fn findings<'a>(report: &'a FileReport, category: &str) -> Vec<&'a Finding> {
    let mut found = report.all_findings();
    found.retain(|finding| finding.category == category);
    found
}

/// What each finding of `category` says, and the function it is filed under.
fn told(report: &FileReport, category: &str) -> Vec<(String, String)> {
    findings(report, category)
        .iter()
        .map(|f| (f.details.clone(), f.function.clone().unwrap_or_default()))
        .collect()
}

/// Nine lines and two methods; `decide` is six lines, complexity 3, nesting depth 2.
const ENGINE: &str =
    "# header\nclass Engine:\n    def decide(self, x):\n        total = 0\n        \
                      if x:\n            for item in x:\n                total += item\n        \
                      return total\n\n    def other(self):\n        return 1\n";

/// One below each of `ENGINE`'s measurements.
const JUST_UNDER_ENGINE: [(&str, f64); 5] = [
    ("class_max_lines_of_code", 8.0),
    ("class_max_functions", 1.0),
    ("brain_method_lines_of_code", 5.0),
    ("brain_method_cyclomatic_complexity", 2.0),
    ("brain_method_nesting_depth", 1.0),
];

#[test]
fn a_brain_class_is_past_every_one_of_its_five_limits() {
    let report = score_with(
        &profile_with(&JUST_UNDER_ENGINE),
        ENGINE,
        "engine.py",
        "python",
    );
    let found = findings(&report, prof::BRAIN_CLASS);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].function.as_deref(), Some("Engine"));
    assert_eq!(found[0].highlight_range.start_line, 2);

    // Sitting exactly on any one limit is enough to be let off.
    for (index, (key, limit)) in JUST_UNDER_ENGINE.iter().enumerate() {
        let mut limits = JUST_UNDER_ENGINE;
        limits[index].1 = limit + 1.0;
        let report = score_with(&profile_with(&limits), ENGINE, "engine.py", "python");
        assert!(findings(&report, prof::BRAIN_CLASS).is_empty(), "{key}");
    }
}

fn assertion_wall(count: usize) -> String {
    let wall: String = (0..count)
        .map(|i| format!("    assert result[{i}] == {i}\n"))
        .collect();
    format!("def test_totals():\n    result = totals()\n{wall}")
}

#[test]
fn an_assertion_block_is_large_only_past_the_limit() {
    let profile = profile_with(&[("assertion_block_max_statements", 4.0)]);
    let at_the_limit = score_with(&profile, &assertion_wall(4), "test_totals.py", "python");
    assert_eq!(told(&at_the_limit, prof::LARGE_ASSERTION_BLOCKS), []);

    let past_it = score_with(&profile, &assertion_wall(5), "test_totals.py", "python");
    assert_eq!(
        told(&past_it, prof::LARGE_ASSERTION_BLOCKS),
        [(
            "5 consecutive assertions".to_string(),
            "test_totals".to_string()
        )]
    );
}

#[test]
fn every_repeat_of_an_assertion_block_is_reported_under_its_own_test() {
    // Four statements where three are enough, and three copies where two are.
    let block = "    assert r.status == 200\n    assert r.body == 'ok'\n    assert r.h == {}\n    \
                 assert r.elapsed < 5\n";
    let source: String = ["a", "b", "c"]
        .iter()
        .map(|name| format!("def test_{name}():\n    r = call()\n{block}\n"))
        .collect();
    let report = score(&source, "test_dup.py", "python");
    let repeated = "assertion block repeated 3 times".to_string();
    assert_eq!(
        told(&report, prof::DUPLICATED_ASSERTION_BLOCKS),
        [
            (repeated.clone(), "test_b".to_string()),
            (repeated, "test_c".to_string())
        ]
    );
}

/// `load` holds one copy of a three-statement block; `save` holds two that overlap, since
/// the block's first and last statements have the same shape. The window is 24 tokens.
const OVERLAPPING_BLOCKS: &str =
    "def load(a, b):\n    total = compute(a, b)\n    audit.record(a, b)\n    \
     other = compute(b, a)\n    return other\n\n\
     def save(a, b):\n    first = compute(a, b)\n    audit.record(a, b)\n    \
     second = compute(b, a)\n    audit.record(b, a)\n    third = compute(a, a)\n    \
     raise Failure(third)\n";

#[test]
fn a_copy_that_overlaps_one_already_counted_is_not_a_second_copy() {
    let profile = profile_with(&[("duplicated_block_min_tokens", 24.0)]);
    let report = score_with(&profile, OVERLAPPING_BLOCKS, "blocks.py", "python");
    assert_eq!(
        told(&report, prof::DUPLICATED_BLOCK),
        [(
            "3 statements repeated 2 times".to_string(),
            "save".to_string()
        )]
    );
    assert_eq!(
        findings(&report, prof::DUPLICATED_BLOCK)[0]
            .highlight_range
            .start_line,
        8
    );

    // One token short of the floor and the block is too generic to mean anything.
    let stricter = profile_with(&[("duplicated_block_min_tokens", 25.0)]);
    let report = score_with(&stricter, OVERLAPPING_BLOCKS, "blocks.py", "python");
    assert_eq!(told(&report, prof::DUPLICATED_BLOCK), []);
}

#[test]
fn ruby_a_call_on_another_object_is_not_a_call_on_self() {
    // `other.count` names a sibling method, but it is somebody else's `count`.
    let report = score(
        "class Report\n  def add(o)\n    @orders.push(o)\n  end\n  def count\n    @orders.length\n  end\n  \
         def compare(other)\n    other.count\n  end\nend\n",
        "report.rb",
        "ruby",
    );
    let details = detail(&report, prof::LOW_COHESION).expect("compare stands alone");
    assert!(details.contains("LCOM4 = 2"), "{details}");
    assert_eq!(
        findings(&report, prof::LOW_COHESION)[0]
            .highlight_range
            .start_line,
        1
    );
}

#[test]
fn go_only_a_not_equal_to_nil_check_is_an_error_guard() {
    // `err == nil` and `err != other` both return early, and both are real decisions.
    let checks: String = (0..6)
        .map(|i| {
            format!(
                "\tif err == nil {{\n\t\treturn {i}\n\t}}\n\tif err != other{i} {{\n\t\treturn {i}\n\t}}\n"
            )
        })
        .collect();
    let report = score(
        &format!("package p\n\nfunc pick(err error) int {{\n{checks}\treturn 0\n}}\n"),
        "sample.go",
        "go",
    );
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );
}

#[test]
fn java_a_modifier_other_than_static_leaves_a_method_on_the_object() {
    let report = score(
        "class Sample {\n    private List<String> orders;\n    private List<String> emails;\n    \
         public void addOrder(String o) { orders.add(o); }\n    \
         public int countOrders() { return orders.size() + 1; }\n    \
         public void addEmail(String e) { emails.add(e); }\n    \
         public int countEmails() { return emails.size() + 1; }\n}\n",
        "Sample.java",
        "java",
    );
    let details = detail(&report, prof::LOW_COHESION).expect("two unrelated groups");
    assert!(details.contains("LCOM4 = 2"), "{details}");
}

fn on_decide(report: &FileReport, category: &str) -> Vec<String> {
    told(report, category)
        .into_iter()
        .filter(|(_, function)| function == "decide")
        .map(|(details, _)| details)
        .collect()
}

#[test]
fn a_function_rule_fires_only_past_its_limit() {
    for (key, measured, category, said) in [
        (
            "function_cyclomatic_complexity_warning",
            3.0,
            prof::COMPLEX_METHOD,
            "cc = 3",
        ),
        (
            "function_lines_of_code_warning",
            6.0,
            prof::LARGE_METHOD,
            "LoC = 6 lines",
        ),
        (
            "function_nesting_depth_warning",
            2.0,
            prof::DEEP_NESTED_COMPLEXITY,
            "Nesting depth = 2 conditionals",
        ),
    ] {
        let at = score_with(
            &profile_with(&[(key, measured)]),
            ENGINE,
            "engine.py",
            "python",
        );
        assert_eq!(on_decide(&at, category), [""; 0], "{key}");
        let past = score_with(
            &profile_with(&[(key, measured - 1.0)]),
            ENGINE,
            "engine.py",
            "python",
        );
        assert_eq!(on_decide(&past, category), [said], "{key}");
    }
}

#[test]
fn a_brain_method_is_past_all_three_of_its_limits() {
    let just_under = &JUST_UNDER_ENGINE[2..];
    let report = score_with(&profile_with(just_under), ENGINE, "engine.py", "python");
    assert_eq!(
        on_decide(&report, prof::BRAIN_METHOD),
        ["LoC = 6 lines, cc = 3, nesting depth = 2"]
    );
    for index in 0..just_under.len() {
        let mut limits = just_under.to_vec();
        limits[index].1 += 1.0;
        let report = score_with(&profile_with(&limits), ENGINE, "engine.py", "python");
        assert_eq!(
            on_decide(&report, prof::BRAIN_METHOD),
            [""; 0],
            "{}",
            limits[index].0
        );
    }
}

#[test]
fn complex_conditionals_are_reported_once_there_are_enough_of_them() {
    let source = "def allowed(a, b, c):\n    if a and b and c:\n        return 1\n    return 0\n";
    let lenient = profile_with(&[("complex_conditional_min_count", 2.0)]);
    let report = score_with(&lenient, source, "rules.py", "python");
    assert_eq!(told(&report, prof::COMPLEX_CONDITIONAL), []);
    let report = score_with(&Profile::default(), source, "rules.py", "python");
    assert_eq!(
        told(&report, prof::COMPLEX_CONDITIONAL),
        [(
            "1 complex conditional expression".to_string(),
            "allowed".to_string()
        )]
    );
}

#[test]
fn a_definition_ends_the_run_of_statements_before_it() {
    // Two statements, a function, then a third: not the three in a row `load` has.
    let source = "total = compute(a, b)\naudit.record(a, b)\n\n\
                  def load(a, b):\n    total = compute(a, b)\n    audit.record(a, b)\n    \
                  other = compute(b, a)\n    return other\n\n\
                  other = compute(b, a)\n";
    let profile = profile_with(&[("duplicated_block_min_tokens", 24.0)]);
    let report = score_with(&profile, source, "blocks.py", "python");
    assert_eq!(told(&report, prof::DUPLICATED_BLOCK), []);
}

#[test]
fn a_test_file_gets_its_size_limits_multiplied() {
    // Nine lines of class against a limit of 4.5 twice over.
    let limits = [
        ("class_max_lines_of_code", 4.5),
        ("test_file_size_multiplier", 2.0),
    ];
    let limits = [&limits[..], &JUST_UNDER_ENGINE[1..]].concat();
    let in_a_test = score_with(&profile_with(&limits), ENGINE, "test_engine.py", "python");
    assert_eq!(told(&in_a_test, prof::BRAIN_CLASS), []);
    let shipped = score_with(&profile_with(&limits), ENGINE, "engine.py", "python");
    assert_eq!(findings(&shipped, prof::BRAIN_CLASS).len(), 1);
}

#[test]
fn the_multiplier_also_stretches_a_limit_no_test_file_is_exempt_from() {
    // `decide` is six lines: over 2.5, under 2.5 three times over.
    let limits = [
        ("function_lines_of_code_warning", 2.5),
        ("test_file_size_multiplier", 3.0),
    ];
    let in_a_test = score_with(&profile_with(&limits), ENGINE, "test_engine.py", "python");
    assert_eq!(told(&in_a_test, prof::LARGE_METHOD), []);
    let shipped = score_with(&profile_with(&limits), ENGINE, "engine.py", "python");
    assert_eq!(on_decide(&shipped, prof::LARGE_METHOD).len(), 1);
}

#[test]
fn a_bare_star_or_slash_in_a_signature_is_not_an_argument() {
    // Four parameters and two markers for how they may be passed.
    let report = py("def place(a, /, b, *, c, d):\n    return a\n");
    assert_eq!(told(&report, prof::EXCESS_ARGUMENTS), []);

    let report = py("def place(a, /, b, *, c, d, e):\n    return a\n");
    let said: Vec<String> = told(&report, prof::EXCESS_ARGUMENTS)
        .into_iter()
        .map(|(details, _)| details)
        .collect();
    assert_eq!(said.len(), 1);
    assert!(said[0].contains('5'), "{said:?}");
}

/// Two functions called `run`, each of complexity 3, with `marker` on the line above the second.
fn two_runs(first_line_suffix: &str, marker: &str) -> String {
    let body =
        "        if x:\n            for item in x:\n                x = item\n        return x\n";
    format!(
        "class Load:\n    def run(self, x):{first_line_suffix}\n{body}\n\nclass Save:\n{marker}    def run(self, x):\n{body}"
    )
}

const QUIET: &str = "# @aqs(disable:\"Complex Method\") measured on purpose";

#[test]
fn a_directive_governs_the_function_below_it_and_not_its_namesake() {
    let profile = profile_with(&[("function_cyclomatic_complexity_warning", 2.0)]);
    let source = two_runs("", &format!("    {QUIET}\n"));
    let report = score_with(&profile, &source, "runs.py", "python");
    let left: Vec<usize> = findings(&report, prof::COMPLEX_METHOD)
        .iter()
        .map(|f| f.highlight_range.start_line)
        .collect();
    assert_eq!(left, [2]);
    assert_eq!(report.suppressed.len(), 1);
}

#[test]
fn a_directive_on_a_function_s_own_line_is_about_the_next_one() {
    let profile = profile_with(&[("function_cyclomatic_complexity_warning", 2.0)]);
    let source = two_runs(&format!("  {QUIET}"), "");
    let report = score_with(&profile, &source, "runs.py", "python");
    let left: Vec<usize> = findings(&report, prof::COMPLEX_METHOD)
        .iter()
        .map(|f| f.highlight_range.start_line)
        .collect();
    assert_eq!(left, [2]);
    assert_eq!(report.suppressed.len(), 1);
}

fn function_count(source: &str) -> Option<String> {
    let profile = profile_with(&[("module_max_functions", 2.0)]);
    detail(
        &score_with(&profile, source, "lib.rs", "rust"),
        prof::FUNCTION_COUNT,
    )
}

#[test]
fn rust_only_a_cfg_test_module_is_an_inline_test() {
    // The function above the module mentions both words; it is still not an attribute.
    let helpers = "fn cfg_test_helper() {}\nmod helpers {\n    fn a() {}\n    fn b() {}\n}\n";
    assert_eq!(function_count(helpers).as_deref(), Some("3 functions"));
    let gated =
        "#[cfg(feature = \"x\")]\nmod helpers {\n    fn a() {}\n    fn b() {}\n    fn c() {}\n}\n";
    assert_eq!(function_count(gated).as_deref(), Some("3 functions"));
}

#[test]
fn rust_what_surrounds_a_test_module_is_not_part_of_it() {
    let source = "fn a() {}\nfn b() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn c() {}\n";
    assert_eq!(function_count(source).as_deref(), Some("3 functions"));
}

#[test]
fn definitions_in_a_row_are_not_a_repeated_block() {
    let trio = |tag: &str| {
        format!(
            "def load_{tag}(a, b):\n    return compute(a, b)\n\n\
             def save_{tag}(a, b):\n    audit.record(a, b)\n    return a\n\n\
             class Holder_{tag}:\n    limit = compute(1, 2)\n\n"
        )
    };
    let profile = profile_with(&[("duplicated_block_min_tokens", 1.0)]);
    let source = format!("{}{}", trio("one"), trio("two"));
    let report = score_with(&profile, &source, "views.py", "python");
    assert_eq!(told(&report, prof::DUPLICATED_BLOCK), []);
}

#[test]
fn a_string_heavy_signature_is_one_past_the_limit() {
    let three = score(
        "def send(to: str, subject: str, body: str):\n    return to\n",
        "mail.py",
        "python",
    );
    assert_eq!(told(&three, prof::STRING_HEAVY_ARGUMENTS), []);
    let four = score(
        "def send(to: str, cc: str, subject: str, body: str):\n    return to\n",
        "mail.py",
        "python",
    );
    assert_eq!(
        told(&four, prof::STRING_HEAVY_ARGUMENTS),
        [("4 string arguments".to_string(), "send".to_string())]
    );
}

/// A function of `6 + 3 * lines` tokens, each line the same shape.
fn copies(name: &str, lines: usize) -> String {
    let body: String = (0..lines).map(|i| format!("    v{i} = source\n")).collect();
    format!("def {name}(source):\n{body}\n")
}

fn duplication(similarity: f64, source: &str) -> Vec<(String, String)> {
    let profile = profile_with(&[
        ("function_duplication_min_lines_of_code_for_check", 1.0),
        ("function_duplication_min_similarity_percentage", similarity),
    ]);
    told(
        &score_with(&profile, source, "copies.py", "python"),
        prof::CODE_DUPLICATION,
    )
}

#[test]
fn functions_too_unequal_in_length_are_not_compared() {
    // 12 tokens against 24 is exactly the half that 50% asks for, and the short one is
    // wholly contained in the long one.
    let half = format!("{}{}", copies("long", 6), copies("short", 2));
    assert_eq!(
        duplication(50.0, &half),
        [("67% similar to long".to_string(), "short".to_string())]
    );
    // 12 against 27 is under half, so it is not looked at -- though 62% of it matches.
    let under = format!("{}{}", copies("long", 7), copies("short", 2));
    assert_eq!(duplication(50.0, &under), []);
    // The same holds whichever of the two comes first.
    let under = format!("{}{}", copies("short", 2), copies("long", 7));
    assert_eq!(duplication(50.0, &under), []);
}

#[test]
fn a_similarity_exactly_at_the_limit_is_a_duplicate() {
    let twins = format!("{}{}", copies("first", 3), copies("second", 3));
    assert_eq!(
        duplication(100.0, &twins),
        [("100% similar to first".to_string(), "second".to_string())]
    );
    // What was measured and what it was held against are both percentages.
    let profile = profile_with(&[
        ("function_duplication_min_lines_of_code_for_check", 1.0),
        ("function_duplication_min_similarity_percentage", 90.0),
    ]);
    let report = score_with(&profile, &twins, "copies.py", "python");
    let found = findings(&report, prof::CODE_DUPLICATION)[0];
    assert_eq!((found.value, found.threshold), (Some(100.0), Some(90.0)));
}

#[test]
fn a_block_repeated_back_to_back_stops_where_its_copy_starts() {
    let block = "    total = compute(a, b)\n    audit.record(a, b)\n    a += 1\n";
    let source = format!("def churn(a, b):\n{block}{block}    total = compute(a, b)\n");
    let profile = profile_with(&[("duplicated_block_min_tokens", 1.0)]);
    let report = score_with(&profile, &source, "churn.py", "python");
    assert_eq!(
        told(&report, prof::DUPLICATED_BLOCK),
        [(
            "3 statements repeated 2 times".to_string(),
            "churn".to_string()
        )]
    );
    assert_eq!(
        findings(&report, prof::DUPLICATED_BLOCK)[0]
            .highlight_range
            .start_line,
        5
    );
}

/// What the file-level rule `category` says about `source` under one changed `limit`.
fn file_rule(limit: (&str, f64), category: &str, source: &str, path: &str) -> Vec<String> {
    let language = if path.ends_with(".ts") {
        "typescript"
    } else {
        "python"
    };
    let report = score_with(&profile_with(&[limit]), source, path, language);
    told(&report, category)
        .into_iter()
        .map(|(details, _)| details)
        .collect()
}

const THREE_FUNCTIONS: &str =
    "def one(x):\n    if x:\n        for item in x:\n            x = item\n    return x\n\n\
                               def two(x):\n    return x\n\n\
                               def three(x):\n    if x:\n        return 1\n    return x\n";

#[test]
fn a_file_level_rule_fires_only_past_its_limit() {
    let shapes = "interface Shape {\n  sides: number;\n  name: string;\n}\n";
    for (key, measured, category, source, path, said) in [
        (
            "file_lines_of_code_warning",
            9.0,
            prof::FILE_SIZE,
            ENGINE,
            "engine.py",
            "LoC = 9 lines",
        ),
        (
            "file_declaration_lines_warning",
            4.0,
            prof::DECLARATION_SIZE,
            shapes,
            "shapes.ts",
            "LoC = 4 lines of declarations",
        ),
        (
            "module_max_functions",
            3.0,
            prof::FUNCTION_COUNT,
            THREE_FUNCTIONS,
            "three.py",
            "3 functions",
        ),
        (
            "module_mean_complexity_warning",
            2.0,
            prof::OVERALL_COMPLEXITY,
            THREE_FUNCTIONS,
            "three.py",
            "mean cc = 2.0 across 3 functions",
        ),
    ] {
        assert_eq!(
            file_rule((key, measured), category, source, path),
            [""; 0],
            "{key}"
        );
        assert_eq!(
            file_rule((key, measured - 0.5), category, source, path),
            [said],
            "{key}"
        );
    }
}

#[test]
fn mean_complexity_is_not_judged_on_fewer_than_three_functions() {
    let two = THREE_FUNCTIONS
        .split("def three")
        .next()
        .expect("the first two");
    let found = file_rule(
        ("module_mean_complexity_warning", 0.0),
        prof::OVERALL_COMPLEXITY,
        two,
        "two.py",
    );
    assert_eq!(found, [""; 0]);
}
