//! End-to-end tests for `@aqs` directives: what they suppress, and how a bad one fails.

mod common;

use common::*;

// --------------------------------------------------------------------------- //
// @aqs directives
// --------------------------------------------------------------------------- //

/// A function that trips both Complex Method (cc 15) and Large Method (> 60 lines), so a
/// directive naming one rule can be shown not to silence the other.
fn offender(name: &str) -> String {
    let branches: String = (0..14)
        .map(|i| format!("    if x == {i}:\n        y{i} = {i}\n        return {i}\n"))
        .collect();
    let padding: String = (0..20).map(|i| format!("    z{i} = {i}\n")).collect();
    format!("def {name}(x):\n{branches}{padding}    return None\n")
}

#[test]
fn a_directive_suppresses_a_finding_and_says_so() {
    let body = offender("f");
    let tree = Tree::new(
        "directive-suppress",
        &[(
            "a.py",
            &format!("# @aqs(disable:\"Complex Method\") dispatch table, splitting it hides intent\n{body}"),
        )],
    );
    let run = tree.scan_json();
    assert_eq!(run.code, EXIT_OK);

    let file = &run.json()["files"][0];
    let categories = reported_categories(&run.json());
    assert!(
        !categories.contains(&"Complex Method".to_string()),
        "Complex Method should be suppressed: {categories:?}"
    );
    // Other rules still fire — a directive names what it disables and nothing more.
    assert!(
        categories.contains(&"Large Method".to_string()),
        "{categories:?}"
    );

    let suppressed = file["suppressed"].as_array().expect("a suppressed list");
    assert_eq!(suppressed.len(), 1);
    assert_eq!(suppressed[0]["category"], "Complex Method");
    assert_eq!(suppressed[0]["function"], "f");
    assert!(suppressed[0]["reason"]
        .as_str()
        .unwrap()
        .contains("dispatch table"));
}

#[test]
fn suppression_is_counted_in_the_human_report() {
    // A branch must not be able to zero its findings invisibly.
    let body = offender("f");
    let tree = Tree::new(
        "directive-visible",
        &[(
            "a.py",
            &format!("# @aqs(disable-all) rewritten next sprint\n{body}"),
        )],
    );
    let run = tree.scan(&[]);
    assert_outcome(&run, EXIT_OK, &["suppressed by @aqs directives"]);
}

#[test]
fn disable_all_covers_every_rule_for_that_function() {
    let body = offender("f");
    let tree = Tree::new(
        "directive-all",
        &[(
            "a.py",
            &format!("# @aqs(disable-all) generated, do not edit\n{body}"),
        )],
    );
    let run = tree.scan_json();
    assert!(
        reported_categories(&run.json()).is_empty(),
        "{}",
        run.stdout
    );
}

#[test]
fn a_directive_governs_only_the_function_it_precedes() {
    let tree = Tree::new(
        "directive-scope",
        &[(
            "a.py",
            &format!(
                "# @aqs(disable:\"Complex Method\") only this one\n{}\n\n{}",
                offender("suppressed"),
                offender("still_reported")
            ),
        )],
    );
    let run = tree.scan_json();
    let file = &run.json()["files"][0];

    let complex: Vec<String> = file["function_level_code_smells"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| {
            let name = g["function"].as_str().unwrap().to_string();
            g["code_smells"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["category"] == "Complex Method")
                .map(move |_| name.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(complex, vec!["still_reported"], "the directive leaked");
}

#[test]
fn a_file_directive_suppresses_file_level_rules() {
    // File-level rules anchor on line 1 and have no function to annotate.
    let big: String = (0..600).map(|i| format!("x{i} = {i}\n")).collect();
    let tree = Tree::new(
        "directive-file",
        &[(
            "a.py",
            &format!("# @aqs-file(disable:\"Lines of Code in a Single File\") generated fixture data\n{big}"),
        )],
    );
    let run = tree.scan_json();
    let categories = reported_categories(&run.json());
    assert!(
        !categories.contains(&"Lines of Code in a Single File".to_string()),
        "{categories:?}"
    );
}

#[test]
fn a_directive_without_a_reason_fails_the_scan() {
    // An unexplained suppression is invisible debt, so it is rejected rather than obeyed.
    let body = offender("f");
    let tree = Tree::new(
        "directive-no-reason",
        &[(
            "a.py",
            &format!("# @aqs(disable:\"Complex Method\")\n{body}"),
        )],
    );
    let run = tree.scan(&[]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stdout.contains("must say why"), "{}", run.stdout);
}

#[test]
fn a_misspelled_rule_in_a_directive_fails_the_scan() {
    // A directive that quietly does nothing is worse than one that fails loudly.
    let body = offender("f");
    let tree = Tree::new(
        "directive-typo",
        &[(
            "a.py",
            &format!("# @aqs(disable:\"Complex Methd\") typo\n{body}"),
        )],
    );
    let run = tree.scan(&[]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stdout.contains("unknown rule"), "{}", run.stdout);
    assert!(
        run.stdout.contains("a.py:1"),
        "should locate it: {}",
        run.stdout
    );
}

#[test]
fn a_broken_directive_cannot_lower_a_score() {
    // Fail-safe: the file is scored as though the directive were absent.
    let body = offender("f");
    let broken = Tree::new(
        "directive-failsafe",
        &[(
            "a.py",
            &format!("# @aqs(disable:\"Complex Methd\") typo\n{body}"),
        )],
    );
    let none = Tree::new(
        "directive-none",
        &[("a.py", &format!("# ordinary\n{body}"))],
    );

    let with_broken = broken.scan_json();
    let without = none.scan_json();
    assert_eq!(
        with_broken.json()["score"],
        without.json()["score"],
        "a broken directive changed the score"
    );
}

#[test]
fn check_also_fails_on_a_broken_directive() {
    let body = offender("f");
    let tree = Tree::new(
        "directive-check",
        &[("a.py", &format!("# @aqs(disable:\"Nope\") bad\n{body}"))],
    );
    let run = aqs(&["check", tree.path().to_str().unwrap()]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("directive error"), "{}", run.stderr);
}

#[test]
fn directives_work_in_every_comment_style() {
    let cases = [
        ("a.py", format!("# @aqs(disable-all) python\n{}", offender("f"))),
        (
            "a.ts",
            format!(
                "// @aqs(disable-all) typescript\nexport function f(x: number) {{\n{}  return 0;\n}}\n",
                (0..14).map(|i| format!("  if (x === {i}) {{ return {i}; }}\n")).collect::<String>()
            ),
        ),
    ];
    for (name, source) in cases {
        let tree = Tree::new(&format!("directive-style-{name}"), &[(name, &source)]);
        let run = tree.scan_json();
        assert!(
            reported_categories(&run.json()).is_empty(),
            "{name} directive ignored: {}",
            run.stdout
        );
    }
}
