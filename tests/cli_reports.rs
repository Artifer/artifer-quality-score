//! End-to-end tests for what the reports say to a person: the `scan` caveats and
//! observations, `explain` in its listing forms, and how `delta` and `gate` describe a
//! change.
//!
//! The JSON shape is covered in `tests/cli.rs`. These read the text a human gets, because
//! a line that is never printed in a test is a line nobody has looked at.

mod common;

use common::*;
use std::os::unix::fs::PermissionsExt;

/// A Python function whose only smell is its cyclomatic complexity, so the same finding
/// can be made cheaper or dearer by changing `branches`.
fn branchy(name: &str, branches: usize) -> String {
    let body: String = (0..branches)
        .map(|i| format!("    if x == {i}:\n        return {i}\n"))
        .collect();
    format!("def {name}(x):\n{body}    return None\n")
}

/// A function repeated across files, big enough to be a cross-file observation.
fn reporter(stage: &str) -> String {
    format!(
        "function reportFailure(step: string, error: unknown) {{\n  \
           const message = error instanceof Error ? error.message : String(error)\n  \
           console.error('{stage}', step, message)\n  \
           track('{stage}.failure', {{ step, message }})\n  \
           return {{ ok: false, step, message }}\n\
         }}\n"
    )
}

// --------------------------------------------------------------------------- //
// scan
// --------------------------------------------------------------------------- //

#[test]
fn scan_defaults_to_the_working_directory() {
    let tree = Tree::new("report-cwd", &[("messy.py", MESSY)]);
    let run = aqs_in(tree.path(), &["scan", "--no-cache"]);
    assert_outcome(&run, EXIT_OK, &["messy.py"]);
}

#[test]
fn scan_says_when_a_score_is_approximate_or_a_file_was_skipped() {
    let tree = Tree::new(
        "report-caveats",
        &[
            ("broken.py", "def f(:\n    return (\n"),
            ("locked.py", CLEAN),
        ],
    );
    let locked = tree.path().join("locked.py");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    let run = tree.scan(&[]);
    assert_outcome(
        &run,
        EXIT_OK,
        &[
            "1 file(s) had syntax errors; their scores are approximate",
            "1 file(s) skipped",
        ],
    );
}

#[test]
fn scan_lists_open_observations_with_every_site() {
    let tree = Tree::new(
        "report-observations",
        &[("a.ts", &reporter("a")), ("b.ts", &reporter("b"))],
    );
    let run = tree.scan(&[]);
    assert_outcome(
        &run,
        EXIT_OK,
        &[
            "· 1 observations",
            "Observations (1)",
            "Cross-File Duplication",
            "a.ts:1",
            "b.ts:1",
            "reportFailure",
        ],
    );
}

#[test]
fn scan_shows_the_worst_files_and_counts_the_rest() {
    let tree = Tree::new(
        "report-limit",
        &[
            ("mild.py", &branchy("mild", 13)),
            ("bad.py", &branchy("bad", 30)),
            ("worst.py", MESSY),
        ],
    );
    let limited = tree.scan(&["-n", "1"]);
    assert_outcome(
        &limited,
        EXIT_OK,
        &[
            "Lowest scoring files (1 of 3)",
            "2 more file(s) with findings — pass --all to see them",
        ],
    );
    assert!(!limited.stdout.contains("mild.py"), "{}", limited.stdout);

    let all = tree.scan(&["-n", "1", "--all"]);
    assert_outcome(&all, EXIT_OK, &["Lowest scoring files (3 of 3)", "mild.py"]);
    let position = |name: &str| all.stdout.find(name).expect("listed");
    assert!(
        position("bad.py") < position("mild.py"),
        "worst first: {}",
        all.stdout
    );
}

#[test]
fn scan_breaks_a_tie_on_score_by_size_and_then_by_path() {
    let tree = Tree::new(
        "report-ties",
        &[
            ("b.py", &branchy("same", 13)),
            ("a.py", &branchy("same", 13)),
            (
                "small.py",
                &branchy("f", 13).replace("return None", "return 0"),
            ),
        ],
    );
    let run = tree.scan(&[]);
    let position = |name: &str| run.stdout.find(name).expect("listed");
    assert!(position("a.py") < position("b.py"), "{}", run.stdout);
}

#[test]
fn scan_fits_its_columns_to_the_terminal() {
    let tree = Tree::new("report-narrow", &[("messy.py", MESSY)]);
    let output = std::process::Command::new(AQS)
        .args(["scan", tree.path().to_str().unwrap(), "--no-cache"])
        .env("COLUMNS", "50")
        .output()
        .expect("aqs runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let finding = stdout
        .lines()
        .find(|line| line.contains("Nesting"))
        .expect("a finding row");
    assert!(finding.chars().count() <= 50, "{finding:?}");
    assert!(
        finding.trim_end().ends_with(char::is_numeric),
        "the penalty survives: {finding:?}"
    );
}

#[test]
fn scan_lists_the_most_widely_copied_observation_first() {
    let retry = |stage: &str| {
        reporter(stage)
            .replace("reportFailure", "retryLater")
            .replace(
                "return { ok: false, step, message }",
                "return schedule(step, message, 3)",
            )
    };
    let notify = |stage: &str| {
        retry(stage)
            .replace("retryLater", "notifyOwner")
            .replace("console.error(", "console.warn(owner, ")
    };
    let tree = Tree::new(
        "report-observation-order",
        &[
            ("a.ts", &retry("a")),
            ("b.ts", &retry("b")),
            ("c.ts", &reporter("c")),
            ("d.ts", &reporter("d")),
            ("e.ts", &reporter("e")),
            ("y.ts", &notify("y")),
            ("z.ts", &notify("z")),
        ],
    );
    let run = tree.scan(&[]);
    assert_outcome(&run, EXIT_OK, &["Observations (3)"]);

    // Most copies first, then source order between the two that tie.
    let position = |text: &str| run.stdout.find(text).expect("listed");
    assert!(position("3 copies across 3 files") < position("a.ts:1"));
    assert!(position("a.ts:1") < position("y.ts:1"), "{}", run.stdout);
}

#[test]
fn check_reports_an_open_observation_at_each_site() {
    let tree = Tree::new(
        "report-check-observation",
        &[("a.ts", &reporter("a")), ("b.ts", &reporter("b"))],
    );
    let run = aqs(&["check", tree.path().to_str().unwrap()]);
    assert_outcome(
        &run,
        EXIT_OK,
        &[
            "a.ts:1:1: Cross-File Duplication",
            "b.ts:1:1: Cross-File Duplication",
            "[reportFailure] (observation)",
        ],
    );
}

// --------------------------------------------------------------------------- //
// explain
// --------------------------------------------------------------------------- //

#[test]
fn explain_lists_every_rule_and_sets_observations_apart() {
    for args in [&["explain", "--list"][..], &["explain"][..]] {
        let run = aqs(args);
        assert_outcome(
            &run,
            EXIT_OK,
            &[
                "Rule",
                "Weight",
                "Complex Method",
                "Observation",
                "not scored",
                "Cross-File Duplication",
                "aqs explain <rule>",
            ],
        );
    }
}

#[test]
fn explain_thresholds_lists_each_name_with_its_value() {
    let run = aqs(&["explain", "--thresholds"]);
    assert_outcome(
        &run,
        EXIT_OK,
        &[
            "Threshold",
            "Value",
            "function_cyclomatic_complexity_warning",
        ],
    );
}

#[test]
fn explain_says_an_observation_is_not_scored() {
    let run = aqs(&["explain", "Cross-File Duplication"]);
    assert_outcome(
        &run,
        EXIT_OK,
        &["Configuration", "not scored — reported as an observation"],
    );
}

#[test]
fn explain_one_rule_as_json_carries_its_guidance_and_thresholds() {
    let run = aqs(&["explain", "Complex Method", "--json"]);
    assert_eq!(run.code, EXIT_OK);
    let rule = run.json();
    assert_eq!(rule["category"], "Complex Method");
    for key in ["summary", "why", "fix", "thresholds"] {
        assert!(rule.get(key).is_some(), "the rule lacks {key}");
    }
}

// --------------------------------------------------------------------------- //
// delta
// --------------------------------------------------------------------------- //

/// A repository where one finding got dearer and another got cheaper.
fn repository_with_a_finding_each_way(name: &str) -> Tree {
    let tree = Tree::new(
        name,
        &[
            ("grows.py", &branchy("grows", 13)),
            ("shrinks.py", &branchy("shrinks", 25)),
        ],
    );
    tree.git_init();
    tree.write("grows.py", &branchy("grows", 25));
    tree.write("shrinks.py", &branchy("shrinks", 13));
    tree
}

#[test]
fn delta_tells_a_finding_that_got_worse_from_one_that_got_better() {
    let tree = repository_with_a_finding_each_way("report-delta-degraded");
    let run = aqs(&["delta", "--path", tree.path().to_str().unwrap(), "--json"]);
    let report = run.json();
    let files = report["files"].as_array().expect("files");

    // Worst change first.
    assert_eq!(files[0]["path"], "grows.py", "{}", run.stdout);
    assert_eq!(files[0]["degraded"][0]["category"], "Complex Method");
    assert_eq!(files[1]["path"], "shrinks.py", "{}", run.stdout);
    assert_eq!(files[1]["improved"][0]["category"], "Complex Method");
    for file in files {
        assert_eq!(file["introduced"].as_array().unwrap().len(), 0, "{file}");
        assert_eq!(file["fixed"].as_array().unwrap().len(), 0, "{file}");
    }
}

#[test]
fn delta_labels_a_file_that_appeared_and_one_that_went() {
    let big: String = (0..700).map(|i| format!("x{i} = {i}\n")).collect();
    let tree = Tree::new(
        "report-delta-labels",
        &[("old.py", MESSY), ("kept.py", CLEAN)],
    );
    tree.git_init();
    std::fs::remove_file(tree.path().join("old.py")).expect("remove");
    tree.write("fresh.py", &big);
    tree.write("kept.py", &format!("{CLEAN}\n"));
    tree.write("also.py", CLEAN);

    let path = tree.path().to_str().unwrap().to_string();
    let human = aqs(&["delta", "--path", &path, "--max-drop", "10"]);
    assert_outcome(&human, EXIT_OK, &["new →", "→ deleted"]);

    // A file-level finding has no function to be keyed on, and files whose score did
    // not move keep path order.
    let report = aqs(&["delta", "--path", &path, "--json"]).json();
    let files = report["files"].as_array().expect("files");
    let fresh = files
        .iter()
        .find(|f| f["path"] == "fresh.py")
        .expect("fresh.py");
    assert!(fresh["introduced"][0]["function"].is_null(), "{fresh}");
    let unmoved: Vec<&str> = files
        .iter()
        .filter(|f| f["score_change"] == 0.0)
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(unmoved, vec!["also.py", "kept.py"]);
}

#[test]
fn delta_scores_a_file_git_has_never_seen() {
    let tree = Tree::new("report-delta-untracked", &[("app.py", CLEAN)]);
    tree.git_init();
    tree.write("fresh.py", MESSY);

    // A single file is as good a way to name the repository as its directory.
    let target = tree.path().join("app.py");
    let run = aqs(&["delta", "--path", target.to_str().unwrap(), "--json"]);
    let report = run.json();
    assert_eq!(report["files"][0]["path"], "fresh.py", "{}", run.stdout);
    assert!(report["files"][0]["old_score"].is_null(), "{}", run.stdout);
}

#[test]
fn delta_skips_a_changed_path_with_nothing_to_read() {
    let tree = Tree::new("report-delta-ghost", &[("app.py", CLEAN)]);
    tree.git_init();
    std::os::unix::fs::symlink("/nowhere/at/all.py", tree.path().join("ghost.py"))
        .expect("symlink");

    let run = aqs(&["delta", "--path", tree.path().to_str().unwrap(), "--json"]);
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);
    assert_eq!(run.json()["files"].as_array().unwrap().len(), 0);
}

#[test]
fn delta_names_the_merge_base_it_compared_against() {
    let tree = Tree::new("report-delta-merge-base", &[("app.py", CLEAN)]);
    tree.git_init();
    tree.git(&["checkout", "-q", "-b", "feature"]);
    tree.write("app.py", MESSY);
    tree.commit("worse");

    let path = tree.path().to_str().unwrap().to_string();
    let run = aqs(&[
        "delta",
        "--path",
        &path,
        "--baseline",
        "develop",
        "--merge-base",
        "--staged",
        "--json",
    ]);
    let baseline = run.json()["baseline"]
        .as_str()
        .expect("baseline")
        .to_string();
    assert!(
        baseline.starts_with("develop (merge-base ") && baseline.ends_with("(staged)"),
        "{baseline}"
    );
}

#[test]
fn delta_against_an_unknown_ref_is_an_error() {
    let tree = Tree::new("report-delta-unknown-ref", &[("app.py", CLEAN)]);
    tree.git_init();
    let path = tree.path().to_str().unwrap().to_string();
    let run = aqs(&["delta", "--path", &path, "--baseline", "no-such-branch"]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("git error"), "{}", run.stderr);
}

#[test]
fn delta_reports_unrelated_histories_as_an_error() {
    let tree = Tree::new("report-delta-orphan", &[("app.py", CLEAN)]);
    tree.git_init();
    tree.git(&["checkout", "-q", "--orphan", "island"]);
    tree.commit("unrelated");

    let path = tree.path().to_str().unwrap().to_string();
    let run = aqs(&[
        "delta",
        "--path",
        &path,
        "--baseline",
        "develop",
        "--merge-base",
    ]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("merge-base"), "{}", run.stderr);
}

// --------------------------------------------------------------------------- //
// gate
// --------------------------------------------------------------------------- //

#[test]
fn gate_says_so_when_the_repository_improved() {
    let tree = Tree::new(
        "report-gate-improved",
        &[("a.py", MESSY), ("b.py", &branchy("b", 30))],
    );
    tree.git_init();
    tree.git(&["checkout", "-q", "-b", "feature"]);
    tree.write("a.py", CLEAN);
    tree.write("b.py", &branchy("b", 13));
    tree.commit("better");

    let path = tree.path().to_str().unwrap().to_string();
    let run = aqs(&[
        "gate",
        "--path",
        &path,
        "--against",
        "develop",
        "--no-cache",
    ]);
    assert_outcome(&run, EXIT_OK, &["PASS", "repository score improved by"]);

    // Two files that moved by the same amount are listed in path order.
    tree.write("d.py", &branchy("same", 13));
    tree.write("c.py", &branchy("same", 13));
    tree.commit("two equal regressions");
    let json = aqs(&[
        "gate",
        "--path",
        &path,
        "--against",
        "HEAD~1",
        "--no-merge-base",
        "--no-cache",
        "--json",
    ]);
    let listed: Vec<String> = json.json()["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(listed, vec!["c.py", "d.py"], "{}", json.stdout);
}
