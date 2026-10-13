//! End-to-end tests for the flags and phrases that only matter at an edge: a switch that
//! is off, a count that is zero, a change of exactly nothing.
//!
//! Each of these is a line that reads the same whether or not it works, until the one
//! case where it does not.

mod common;

use common::*;

fn lacks(run: &Run, phrases: &[&str]) {
    for phrase in phrases {
        assert!(
            !run.stdout.contains(phrase),
            "stdout has {phrase:?}:\n{}",
            run.stdout
        );
    }
}

/// A repository on a `feature` branch cut from `develop`.
fn branched(name: &str, files: &[(&str, &str)]) -> Tree {
    let tree = Tree::new(name, files);
    tree.git_init();
    tree.git(&["checkout", "-q", "-b", "feature"]);
    tree
}

fn gate(tree: &Tree, flags: &[&str]) -> Run {
    let mut args = vec!["gate", "--path", tree.path().to_str().unwrap()];
    args.extend_from_slice(&["--against", "develop"]);
    args.extend_from_slice(flags);
    aqs(&args)
}

// --------------------------------------------------------------------------- //
// The cache is written unless it is switched off
// --------------------------------------------------------------------------- //

#[test]
fn scan_writes_a_cache_only_when_allowed_to() {
    let tree = Tree::new("edge-scan-cache", &[("app.py", CLEAN)]);
    let cache = tree.path().join(".aqs");

    assert_eq!(tree.scan(&[]).code, EXIT_OK);
    assert!(!cache.exists(), "--no-cache left a cache behind");

    let path = tree.path().to_str().unwrap();
    assert_eq!(aqs(&["scan", path]).code, EXIT_OK);
    assert!(cache.join("cache.sqlite3").exists());
}

#[test]
fn gate_writes_a_cache_only_when_allowed_to() {
    let tree = branched("edge-gate-cache", &[("app.py", CLEAN)]);
    let cache = tree.path().join(".aqs");

    assert_eq!(gate(&tree, &["--no-cache"]).code, EXIT_OK);
    assert!(!cache.exists(), "--no-cache left a cache behind");

    assert_eq!(gate(&tree, &[]).code, EXIT_OK);
    assert!(cache.join("cache.sqlite3").exists());
}

// --------------------------------------------------------------------------- //
// gate
// --------------------------------------------------------------------------- //

#[test]
fn gate_compares_against_the_branch_tip_when_told_not_to_use_the_merge_base() {
    let tree = branched("edge-gate-tip", &[("mine.py", CLEAN)]);
    tree.git(&["checkout", "-q", "develop"]);
    tree.write("theirs.py", MESSY);
    tree.commit("their mess");
    tree.git(&["checkout", "-q", "feature"]);

    let from_the_fork = gate(&tree, &["--no-cache", "--json"]).json();
    assert_eq!(from_the_fork["baseline_score"], 10.0);
    assert_eq!(from_the_fork["baseline_file_count"], 1);

    let from_the_tip = gate(&tree, &["--no-cache", "--json", "--no-merge-base"]).json();
    assert!(from_the_tip["baseline_score"].as_f64().unwrap() < 10.0);
    assert_eq!(from_the_tip["baseline_file_count"], 2);
}

#[test]
fn gate_scores_only_what_a_scan_would() {
    // A vendored dependency and a file in no supported language are both in the tree.
    let tree = branched(
        "edge-gate-tree",
        &[
            ("app.py", CLEAN),
            ("node_modules/lib/index.py", MESSY),
            ("README.md", "# notes\n"),
        ],
    );
    let report = gate(&tree, &["--no-cache", "--json"]).json();
    assert_eq!(report["baseline_file_count"], 1);
    assert_eq!(report["current_file_count"], 1);
    assert_eq!(report["current_score"], 10.0);
}

#[test]
fn gate_mentions_held_scores_only_when_a_changed_file_held_its_score() {
    let tree = branched("edge-gate-held", &[("a.py", CLEAN), ("b.py", CLEAN)]);
    tree.write("a.py", MESSY);
    tree.commit("worse");
    let run = gate(&tree, &["--no-cache"]);
    assert_outcome(
        &run,
        EXIT_GATE_FAILED,
        &[
            "merge gate",
            "baseline",
            "Changed files that moved the score",
        ],
    );
    lacks(&run, &["held their score"]);

    tree.write("b.py", &format!("{CLEAN}\nLIMIT = 3\n"));
    tree.commit("and a change that costs nothing");
    let run = gate(&tree, &["--no-cache"]);
    assert_outcome(
        &run,
        EXIT_GATE_FAILED,
        &["1 other changed file(s) held their score"],
    );
}

#[test]
fn gate_does_not_call_an_unchanged_score_an_improvement() {
    let tree = branched("edge-gate-level", &[("a.py", CLEAN)]);
    let run = gate(&tree, &["--no-cache"]);
    assert_outcome(&run, EXIT_OK, &["PASS", "repository score did not go down"]);
    lacks(&run, &["improved"]);
}

// --------------------------------------------------------------------------- //
// scan
// --------------------------------------------------------------------------- //

#[test]
fn scan_keeps_quiet_about_what_did_not_happen() {
    let tree = Tree::new("edge-scan-quiet", &[("app.py", CLEAN)]);
    let run = tree.scan(&[]);
    assert_outcome(&run, EXIT_OK, &["10.00"]);
    lacks(&run, &["syntax errors", "skipped", "@aqs directives"]);
}

fn scan_at_width(tree: &Tree, columns: Option<&str>) -> String {
    let mut command = std::process::Command::new(AQS);
    command.args(["scan", tree.path().to_str().unwrap(), "--no-cache"]);
    match columns {
        Some(value) => command.env("COLUMNS", value),
        None => command.env_remove("COLUMNS"),
    };
    let output = command.output().expect("aqs runs");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn scan_assumes_a_hundred_columns_when_the_terminal_does_not_say() {
    let tree = Tree::new("edge-scan-width", &[("messy.py", MESSY)]);
    let assumed = scan_at_width(&tree, None);
    assert_eq!(assumed, scan_at_width(&tree, Some("100")));
    assert_eq!(assumed, scan_at_width(&tree, Some("wide")));
    assert_ne!(assumed, scan_at_width(&tree, Some("50")));
}

#[test]
fn scan_does_not_promise_more_files_when_the_limit_fits_them_all() {
    let tree = Tree::new("edge-scan-limit", &[("messy.py", MESSY), ("app.py", CLEAN)]);
    let run = tree.scan(&["-n", "1"]);
    assert_outcome(&run, EXIT_OK, &["Lowest scoring files (1 of 1)"]);
    lacks(&run, &["more file(s)"]);
}
