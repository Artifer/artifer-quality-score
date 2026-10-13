//! End-to-end tests for the command line interface.
//!
//! These invoke the real binary, because the CLI *is* the contract: exit codes gate
//! automation, and the JSON shape is what a model consumes. Everything below was
//! previously verified only by hand, and two regressions slipped through that way — an
//! error message printed whenever `--min-score` was passed rather than only on failure,
//! and `.aqs.json` being ignored for a bare-filename target.

mod common;

use common::*;

#[test]
fn version_and_help_succeed() {
    let version = aqs(&["--version"]);
    assert_eq!(version.code, EXIT_OK);
    assert!(version.stdout.starts_with("aqs "), "{}", version.stdout);

    let help = aqs(&["--help"]);
    assert_eq!(help.code, EXIT_OK);
    for command in ["scan", "delta", "gate", "check", "explain"] {
        assert!(help.stdout.contains(command), "help omits {command}");
    }
}

#[test]
fn a_missing_path_is_an_error_not_a_score() {
    let run = aqs(&["scan", "/definitely/not/here"]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("no such path"), "{}", run.stderr);
}

// --------------------------------------------------------------------------- //
// scan
// --------------------------------------------------------------------------- //

#[test]
fn scan_reports_the_worst_files_first() {
    let tree = Tree::new("scan", &[("clean.py", CLEAN), ("messy.py", MESSY)]);
    let run = tree.scan(&[]);
    assert_outcome(&run, EXIT_OK, &["Artifer Quality Score", "messy.py"]);
}

#[test]
fn a_clean_tree_says_so() {
    let tree = Tree::new("scan-clean", &[("clean.py", CLEAN)]);
    let run = tree.scan(&[]);
    assert_outcome(&run, EXIT_OK, &["No findings"]);
}

#[test]
fn scan_json_is_parseable_and_complete() {
    let tree = Tree::new("scan-json", &[("clean.py", CLEAN), ("messy.py", MESSY)]);
    let run = tree.scan_json();
    assert_eq!(run.code, EXIT_OK);

    let report = run.json();
    for key in ["version", "profile", "score", "loc", "files"] {
        assert!(report.get(key).is_some(), "report is missing {key}");
    }
    let score = report["score"].as_f64().expect("score is a number");
    assert!((1.0..=10.0).contains(&score), "score out of range: {score}");

    let files = report["files"].as_array().expect("files is an array");
    let messy = files
        .iter()
        .find(|f| f["path"] == "messy.py")
        .expect("messy.py is in the report");
    assert!(messy["score"].as_f64().unwrap() < 10.0);

    let smell = &messy["function_level_code_smells"][0]["code_smells"][0];
    for key in [
        "category",
        "details",
        "penalty",
        "highlight_range",
        "value",
        "threshold",
    ] {
        assert!(smell.get(key).is_some(), "finding is missing {key}");
    }
}

#[test]
fn guidance_explains_every_rule_that_fired() {
    // The payload a model receives must explain every rule it is charged for.
    let tree = Tree::new("guidance", &[("messy.py", MESSY)]);
    let run = tree.scan(&["--json", "--guidance"]);
    let report = run.json();

    let reported = reported_categories(&report);
    assert!(!reported.is_empty(), "expected findings to explain");

    let guidance = report["guidance"].as_object().expect("guidance is present");
    for category in reported {
        let entry = guidance
            .get(&category)
            .unwrap_or_else(|| panic!("no guidance for {category}"));
        for key in ["summary", "why", "fix"] {
            assert!(entry.get(key).is_some(), "{category} guidance lacks {key}");
        }
    }
}

#[test]
fn min_score_gates_and_only_complains_when_it_fails() {
    // The message used to print whenever --min-score was passed, including on success.
    let tree = Tree::new("min-score", &[("messy.py", MESSY)]);
    let path = tree.path().to_str().unwrap().to_string();

    let passes = aqs(&["scan", &path, "--min-score", "1.0", "--no-cache"]);
    assert_eq!(passes.code, EXIT_OK);
    assert!(
        passes.stderr.trim().is_empty(),
        "nothing should be written to stderr on success: {}",
        passes.stderr
    );

    let fails = aqs(&["scan", &path, "--min-score", "9.9", "--no-cache"]);
    assert_eq!(fails.code, EXIT_GATE_FAILED);
    assert!(
        fails.stderr.contains("below the required"),
        "{}",
        fails.stderr
    );
}

// --------------------------------------------------------------------------- //
// check
// --------------------------------------------------------------------------- //

#[test]
fn check_emits_parseable_lint_lines() {
    let tree = Tree::new("check", &[("messy.py", MESSY)]);
    let run = aqs(&["check", tree.path().to_str().unwrap()]);
    assert_eq!(run.code, EXIT_OK);

    let lines: Vec<&str> = run
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert!(!lines.is_empty(), "expected findings");
    for line in lines {
        // path:line:col: Rule: details
        let (location, rest) = line.split_once(": ").unwrap_or_else(|| panic!("{line}"));
        let parts: Vec<&str> = location.rsplitn(3, ':').collect();
        assert_eq!(parts.len(), 3, "{line}");
        assert!(
            parts[0].parse::<u32>().is_ok(),
            "column not numeric: {line}"
        );
        assert!(parts[1].parse::<u32>().is_ok(), "line not numeric: {line}");
        assert!(!rest.is_empty(), "{line}");
    }
}

#[test]
fn check_can_fail_on_findings() {
    let dirty = Tree::new("check-dirty", &[("messy.py", MESSY)]);
    let run = aqs(&[
        "check",
        dirty.path().to_str().unwrap(),
        "--fail-on-findings",
    ]);
    assert_eq!(run.code, EXIT_GATE_FAILED);

    let clean = Tree::new("check-clean", &[("clean.py", CLEAN)]);
    let run = aqs(&[
        "check",
        clean.path().to_str().unwrap(),
        "--fail-on-findings",
    ]);
    assert_eq!(run.code, EXIT_OK);
}

// --------------------------------------------------------------------------- //
// explain
// --------------------------------------------------------------------------- //

#[test]
fn explain_resolves_exact_and_fuzzy_names() {
    let exact = aqs(&["explain", "Bumpy Road Ahead"]);
    assert_eq!(exact.code, EXIT_OK);
    assert!(exact.stdout.contains("How to fix it"), "{}", exact.stdout);

    let fuzzy = aqs(&["explain", "bumpy"]);
    assert_eq!(fuzzy.code, EXIT_OK);
    assert!(
        fuzzy.stdout.contains("Bumpy Road Ahead"),
        "{}",
        fuzzy.stdout
    );
}

#[test]
fn an_unknown_rule_is_an_error() {
    let run = aqs(&["explain", "Not A Rule"]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("unknown rule"), "{}", run.stderr);
}

#[test]
fn explain_list_covers_every_rule_with_guidance() {
    let run = aqs(&["explain", "--list", "--json"]);
    assert_eq!(run.code, EXIT_OK);
    let rules = run.json();
    let rules = rules.as_object().expect("an object keyed by rule");
    assert_eq!(rules.len(), 21, "expected the full catalogue");

    for (name, entry) in rules {
        for key in ["summary", "why", "fix", "scored", "thresholds"] {
            assert!(entry.get(key).is_some(), "{name} lacks {key}");
        }
        // Only scored rules carry a weight. An observation that grew one would be
        // priceable, which is the distinction the channel exists to hold.
        let scored = entry["scored"].as_bool().expect("scored is a bool");
        assert_eq!(entry.get("weight").is_some(), scored, "{name}");
    }

    let scored = rules
        .values()
        .filter(|entry| entry["scored"] == true)
        .count();
    assert_eq!(scored, 20, "every scored rule is listed");
}

#[test]
fn explain_thresholds_lists_configurable_names() {
    let run = aqs(&["explain", "--thresholds", "--json"]);
    assert_eq!(run.code, EXIT_OK);
    let thresholds = run.json();
    let thresholds = thresholds.as_object().expect("an object");
    assert!(thresholds.contains_key("function_cyclomatic_complexity_warning"));
    assert!(thresholds.values().all(|v| v.is_number()));
}

// --------------------------------------------------------------------------- //
// delta
// --------------------------------------------------------------------------- //

#[test]
fn delta_fails_on_a_regression_and_names_it() {
    let tree = Tree::new("delta-regress", &[("app.py", CLEAN)]);
    tree.git_init();
    tree.write("app.py", MESSY);

    let run = aqs(&["delta", "--path", tree.path().to_str().unwrap()]);
    // "new" is the label for an introduced finding.
    assert_outcome(&run, EXIT_GATE_FAILED, &["new", "Regression"]);
}

#[test]
fn delta_tolerates_a_drop_within_max_drop() {
    let tree = Tree::new("delta-tolerance", &[("app.py", CLEAN)]);
    tree.git_init();
    tree.write("app.py", MESSY);

    let run = aqs(&[
        "delta",
        "--path",
        tree.path().to_str().unwrap(),
        "--max-drop",
        "10",
    ]);
    assert_eq!(run.code, EXIT_OK);
    assert!(!run.stdout.contains("Regression"), "{}", run.stdout);
}

#[test]
fn delta_does_not_call_an_unchanged_score_a_regression() {
    let tree = Tree::new("delta-level", &[("app.py", CLEAN)]);
    tree.git_init();
    tree.write("app.py", &format!("{CLEAN}\nLIMIT = 3\n"));

    let run = aqs(&["delta", "--path", tree.path().to_str().unwrap()]);
    assert_outcome(&run, EXIT_OK, &["app.py"]);
    assert!(!run.stdout.contains("Regression"), "{}", run.stdout);
}

#[test]
fn delta_with_no_changes_passes() {
    let tree = Tree::new("delta-clean", &[("app.py", CLEAN)]);
    tree.git_init();

    let run = aqs(&["delta", "--path", tree.path().to_str().unwrap()]);
    assert_outcome(&run, EXIT_OK, &["No changed files"]);
}

#[test]
fn delta_json_reports_the_change() {
    let tree = Tree::new("delta-json", &[("app.py", CLEAN)]);
    tree.git_init();
    tree.write("app.py", MESSY);

    let run = aqs(&["delta", "--path", tree.path().to_str().unwrap(), "--json"]);
    let report = run.json();
    for key in [
        "baseline",
        "old_score",
        "new_score",
        "score_change",
        "files",
    ] {
        assert!(report.get(key).is_some(), "delta report lacks {key}");
    }
    assert!(report["score_change"].as_f64().unwrap() < 0.0);
}

#[test]
fn delta_scores_the_index_with_staged() {
    let tree = Tree::new("delta-staged", &[("app.py", CLEAN)]);
    tree.git_init();
    tree.write("app.py", MESSY);
    tree.git(&["add", "app.py"]);
    // A further unstaged edit that must not be scored.
    tree.write("app.py", &format!("{MESSY}\nx = 1\n"));

    let run = aqs(&[
        "delta",
        "--path",
        tree.path().to_str().unwrap(),
        "--staged",
        "--json",
    ]);
    let report = run.json();
    assert_eq!(report["files"].as_array().unwrap().len(), 1);
    assert!(report["baseline"].as_str().unwrap().contains("staged"));
}

#[test]
fn delta_outside_a_repository_is_an_error() {
    let tree = Tree::new("delta-nogit", &[("app.py", CLEAN)]);
    let run = aqs(&["delta", "--path", tree.path().to_str().unwrap()]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("git error"), "{}", run.stderr);
}

// --------------------------------------------------------------------------- //
// gate
// --------------------------------------------------------------------------- //

#[test]
fn gate_fails_when_the_repository_score_drops() {
    let tree = Tree::new("gate-regress", &[("a.py", CLEAN), ("b.py", CLEAN)]);
    tree.git_init();
    tree.git(&["checkout", "-q", "-b", "feature"]);
    tree.write("a.py", MESSY);
    tree.commit("worse");

    let run = aqs(&[
        "gate",
        "--path",
        tree.path().to_str().unwrap(),
        "--against",
        "develop",
        "--no-cache",
    ]);
    assert_outcome(&run, EXIT_GATE_FAILED, &["FAIL", "a.py"]);
}

#[test]
fn gate_passes_an_unchanged_repository() {
    let tree = Tree::new("gate-clean", &[("a.py", CLEAN)]);
    tree.git_init();

    let run = aqs(&[
        "gate",
        "--path",
        tree.path().to_str().unwrap(),
        "--against",
        "develop",
        "--no-cache",
    ]);
    assert_outcome(&run, EXIT_OK, &["PASS"]);
}

#[test]
fn gate_json_records_a_concrete_baseline_revision() {
    let tree = Tree::new("gate-json", &[("a.py", CLEAN)]);
    tree.git_init();

    let run = aqs(&[
        "gate",
        "--path",
        tree.path().to_str().unwrap(),
        "--against",
        "develop",
        "--json",
        "--no-cache",
    ]);
    let report = run.json();
    for key in [
        "against",
        "baseline_revision",
        "baseline_score",
        "current_score",
        "score_change",
        "tolerance",
    ] {
        assert!(report.get(key).is_some(), "gate report lacks {key}");
    }
    // A report that records "develop" rather than a SHA is not reproducible tomorrow.
    let revision = report["baseline_revision"].as_str().unwrap();
    assert_eq!(revision.len(), 40, "expected a full SHA, got {revision}");
    assert!(revision.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn gate_tolerance_permits_a_small_drop() {
    let tree = Tree::new("gate-tolerance", &[("a.py", CLEAN)]);
    tree.git_init();
    tree.git(&["checkout", "-q", "-b", "feature"]);
    tree.write("a.py", MESSY);
    tree.commit("worse");

    let path = tree.path().to_str().unwrap().to_string();
    let strict = aqs(&[
        "gate",
        "--path",
        &path,
        "--against",
        "develop",
        "--no-cache",
    ]);
    let lenient = aqs(&[
        "gate",
        "--path",
        &path,
        "--against",
        "develop",
        "--tolerance",
        "10",
        "--no-cache",
    ]);
    assert_eq!(strict.code, EXIT_GATE_FAILED);
    assert_eq!(lenient.code, EXIT_OK);
}

#[test]
fn gate_judges_only_your_own_changes() {
    // The merge base is the default baseline, so someone else's regression on the target
    // branch must not fail your branch.
    let tree = Tree::new("gate-mergebase", &[("mine.py", CLEAN)]);
    tree.git_init();
    tree.git(&["checkout", "-q", "-b", "feature"]);
    tree.write("mine.py", CLEAN);
    tree.write("also-mine.py", CLEAN);
    tree.commit("my clean addition");

    tree.git(&["checkout", "-q", "develop"]);
    tree.write("theirs.py", MESSY);
    tree.commit("their mess");
    tree.git(&["checkout", "-q", "feature"]);

    let run = aqs(&[
        "gate",
        "--path",
        tree.path().to_str().unwrap(),
        "--against",
        "develop",
        "--no-cache",
    ]);
    assert_eq!(run.code, EXIT_OK, "{}", run.stdout);
}

// --------------------------------------------------------------------------- //
// Configuration
// --------------------------------------------------------------------------- //

#[test]
fn config_applies_to_a_bare_filename_target() {
    // `Path::new("thing.py").parent()` is Some(""), which made config discovery skip
    // `.aqs.json` entirely on exactly the single-file call an editor or agent loop uses.
    let tree = Tree::new(
        "config-bare",
        &[
            (
                ".aqs.json",
                r#"{"name":"quiet","rules":{"Deep, Nested Complexity":0.0,"Complex Method":0.0,"Bumpy Road Ahead":0.0,"Complex Conditional":0.0,"Excess Number of Function Arguments":0.0}}"#,
            ),
            ("messy.py", MESSY),
        ],
    );
    // Addressed as a bare filename, with the child's cwd inside the tree.
    let run = aqs_in(tree.path(), &["check", "messy.py"]);
    assert_eq!(run.code, EXIT_OK);
    assert!(
        run.stdout.trim().is_empty(),
        "config should have silenced every rule: {}",
        run.stdout
    );
    // And the path is reported relative, not absolute.
    let scan = aqs_in(tree.path(), &["scan", "messy.py", "--json", "--no-cache"]);
    assert_eq!(scan.json()["files"][0]["path"], "messy.py");
    assert_eq!(scan.json()["profile"], "quiet");
}

#[test]
fn a_file_in_a_subdirectory_hangs_off_that_directory() {
    let tree = Tree::new(
        "config-nested",
        &[
            ("sub/.aqs.json", r#"{"name":"nested"}"#),
            ("sub/messy.py", MESSY),
        ],
    );
    let scan = aqs_in(
        tree.path(),
        &["scan", "sub/messy.py", "--json", "--no-cache"],
    );
    assert_eq!(scan.json()["files"][0]["path"], "messy.py");
    assert_eq!(scan.json()["profile"], "nested");
}

#[test]
fn an_unknown_rule_name_in_config_is_fatal() {
    let tree = Tree::new(
        "config-bad-rule",
        &[
            (".aqs.json", r#"{"rules":{"Complex Methd":0.0}}"#),
            ("a.py", CLEAN),
        ],
    );
    let run = tree.scan(&[]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("unknown rule"), "{}", run.stderr);
}

#[test]
fn an_unknown_config_section_is_fatal() {
    // A config that quietly fails to apply is worse than one that refuses to load.
    let tree = Tree::new(
        "config-bad-section",
        &[
            (".aqs.json", r#"{"rule":{"Complex Method":0.0}}"#),
            ("a.py", CLEAN),
        ],
    );
    let run = tree.scan(&[]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(
        run.stderr.contains("unknown config section"),
        "{}",
        run.stderr
    );
}

#[test]
fn invalid_config_json_is_reported_with_its_path() {
    let tree = Tree::new(
        "config-bad-json",
        &[(".aqs.json", "{not json"), ("a.py", CLEAN)],
    );
    let run = tree.scan(&[]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains(".aqs.json"), "{}", run.stderr);
}

// --------------------------------------------------------------------------- //
// Cache
// --------------------------------------------------------------------------- //

#[test]
fn the_cache_never_changes_a_score() {
    let tree = Tree::new("cache-parity", &[("messy.py", MESSY), ("clean.py", CLEAN)]);
    let path = tree.path().to_str().unwrap().to_string();

    let cold = aqs(&["scan", &path, "--json"]);
    let warm = aqs(&["scan", &path, "--json"]);
    let uncached = aqs(&["scan", &path, "--json", "--no-cache"]);

    assert_eq!(cold.json(), warm.json(), "a warm cache changed the report");
    assert_eq!(cold.json(), uncached.json(), "caching changed the report");
    assert!(
        tree.path().join(".aqs").exists(),
        "expected a cache directory"
    );
}
