//! End-to-end tests for the command line interface.
//!
//! These invoke the real binary, because the CLI *is* the contract: exit codes gate
//! automation, and the JSON shape is what a model consumes. Everything below was
//! previously verified only by hand, and two regressions slipped through that way — an
//! error message printed whenever `--min-score` was passed rather than only on failure,
//! and `.aqs.json` being ignored for a bare-filename target.
//!
//! No test mutates the process working directory. Where a test needs to run "inside" a
//! directory it sets the *child's* cwd, so the suite stays safe under `cargo test`'s
//! default parallelism.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AQS: &str = env!("CARGO_BIN_EXE_aqs");

const CLEAN: &str = "def add(a: int, b: int) -> int:\n    \"\"\"Add.\"\"\"\n    return a + b\n";
const MESSY: &str = "def f(a, b, c, d, e, f):
    total = 0
    if a:
        for i in range(b):
            while i:
                if i and a or b:
                    for j in range(i):
                        if j:
                            total += j
    return total
";

/// A throwaway directory, removed when the guard drops even if a test panics.
struct Tree(PathBuf);

impl Tree {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!("aqs-cli-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        for (relative, body) in files {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("has a parent")).expect("mkdir");
            std::fs::write(&path, body).expect("write");
        }
        std::fs::create_dir_all(&root).expect("mkdir root");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, body: &str) {
        std::fs::write(self.0.join(relative), body).expect("write");
    }

    /// Turn the tree into a git repository with one commit.
    fn git_init(&self) -> &Self {
        self.git(&["init", "-q", "-b", "develop"]);
        self.git(&["config", "user.email", "t@example.com"]);
        self.git(&["config", "user.name", "T"]);
        self.commit("initial");
        self
    }

    fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .output()
            .expect("git runs");
        assert!(
            status.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", self.stdout))
    }
}

fn from(output: Output) -> Run {
    Run {
        code: output.status.code().expect("process exited normally"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Run `aqs` with the given arguments.
fn aqs(args: &[&str]) -> Run {
    from(Command::new(AQS).args(args).output().expect("aqs runs"))
}

/// Run `aqs` as though the shell were sitting in `cwd`.
fn aqs_in(cwd: &Path, args: &[&str]) -> Run {
    from(
        Command::new(AQS)
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("aqs runs"),
    )
}

// --------------------------------------------------------------------------- //
// Exit codes: the contract automation binds to
// --------------------------------------------------------------------------- //

const EXIT_OK: i32 = 0;
const EXIT_GATE_FAILED: i32 = 1;
const EXIT_ERROR: i32 = 2;

/// Assert an exit code and that stdout mentions each of `expected`.
///
/// A named matcher rather than the same three asserts in every gate test: when it fails
/// you learn which phrase was missing, and there is one place to change.
fn assert_outcome(run: &Run, code: i32, expected: &[&str]) {
    assert_eq!(
        run.code, code,
        "unexpected exit code. stdout:\n{}",
        run.stdout
    );
    for phrase in expected {
        assert!(
            run.stdout.contains(phrase),
            "stdout lacks {phrase:?}:\n{}",
            run.stdout
        );
    }
}

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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
    assert_outcome(&run, EXIT_OK, &["Artifer Quality Score", "messy.py"]);
}

#[test]
fn a_clean_tree_says_so() {
    let tree = Tree::new("scan-clean", &[("clean.py", CLEAN)]);
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
    assert_outcome(&run, EXIT_OK, &["No findings"]);
}

#[test]
fn scan_json_is_parseable_and_complete() {
    let tree = Tree::new("scan-json", &[("clean.py", CLEAN), ("messy.py", MESSY)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
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

/// Every rule name that appears anywhere in a scan report.
fn reported_categories(report: &Value) -> Vec<String> {
    let mut names = Vec::new();
    for file in report["files"].as_array().expect("files is an array") {
        let file_level = file["file_level_code_smells"].as_array().unwrap().iter();
        let function_level = file["function_level_code_smells"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| group["code_smells"].as_array().unwrap().iter());
        for smell in file_level.chain(function_level) {
            names.push(smell["category"].as_str().unwrap().to_string());
        }
    }
    names
}

#[test]
fn guidance_explains_every_rule_that_fired() {
    // The payload a model receives must explain every rule it is charged for.
    let tree = Tree::new("guidance", &[("messy.py", MESSY)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--guidance",
        "--no-cache",
    ]);
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
fn an_unknown_rule_name_in_config_is_fatal() {
    let tree = Tree::new(
        "config-bad-rule",
        &[
            (".aqs.json", r#"{"rules":{"Complex Methd":0.0}}"#),
            ("a.py", CLEAN),
        ],
    );
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
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
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
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
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
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
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
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
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
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

    let with_broken = aqs(&[
        "scan",
        broken.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    let without = aqs(&[
        "scan",
        none.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
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
        let run = aqs(&[
            "scan",
            tree.path().to_str().unwrap(),
            "--json",
            "--no-cache",
        ]);
        assert!(
            reported_categories(&run.json()).is_empty(),
            "{name} directive ignored: {}",
            run.stdout
        );
    }
}

// --------------------------------------------------------------------------- //
// Observations: the unscored channel
// --------------------------------------------------------------------------- //

/// A reporter big enough to clear `cross_file_duplication_min_lines_of_code`.
///
/// The `stage` string differs per copy so the files are not byte-identical — the match
/// has to come from normalised structure, which is the whole point of the rule.
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

fn observations(report: &Value) -> &Vec<Value> {
    report["observations"]
        .as_array()
        .expect("an observations array")
}

/// The regression this whole channel exists for.
///
/// Splitting a file that held two copies of a function used to *raise* the score: the
/// intra-file duplication finding disappeared and nothing replaced it. Both files score a
/// clean 10.00 here, which is correct — the duplication is not a complexity problem — but
/// it must not therefore be invisible.
#[test]
fn splitting_a_file_does_not_launder_its_duplication() {
    let tree = Tree::new(
        "observation-launder",
        &[
            ("useClip.ts", &format!("export function useClip(s: string) {{\n  return {{ s }}\n}}\n\n{}", reporter("clip"))),
            ("useClipCut.ts", &format!("export function useClipCut(s: string, at: number) {{\n  return {{ s, at }}\n}}\n\n{}", reporter("cut"))),
        ],
    );
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(run.code, EXIT_OK, "an observation must not fail the scan");

    let report = run.json();
    assert_eq!(
        report["score"], 10.0,
        "duplication is not a complexity penalty"
    );
    assert_eq!(report["files"].as_array().unwrap().len(), 2);

    let found = observations(&report);
    assert_eq!(
        found.len(),
        1,
        "expected one cross-file observation: {}",
        run.stdout
    );
    assert_eq!(found[0]["category"], "Cross-File Duplication");
    assert!(found[0]["dismissed"].is_null(), "nothing dismissed it");

    let sites = found[0]["sites"].as_array().expect("sites");
    let paths: Vec<&str> = sites.iter().map(|s| s["path"].as_str().unwrap()).collect();
    assert_eq!(
        paths,
        vec!["useClip.ts", "useClipCut.ts"],
        "both copies, in path order"
    );
    for site in sites {
        assert_eq!(site["function"], "reportFailure");
    }
}

/// The score and the channel must not both charge for the same repetition.
#[test]
fn duplication_inside_one_file_stays_a_scored_finding() {
    let body: String = (0..18)
        .map(|i| format!("  const v{i} = input[{i}] ?? {i}\n"))
        .collect();
    let twice = format!(
        "export function first(input: number[]) {{\n{body}  return 1\n}}\n\n\
         export function second(input: number[]) {{\n{body}  return 1\n}}\n"
    );
    let tree = Tree::new("observation-intra-file", &[("pair.ts", &twice)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    let report = run.json();

    assert!(
        reported_categories(&report).contains(&"Code Duplication".to_string()),
        "the intra-file rule should still score this: {}",
        run.stdout
    );
    assert!(
        observations(&report).is_empty(),
        "one file is the scored rule's territory, not the channel's: {}",
        run.stdout
    );
}

#[test]
fn a_directive_dismisses_an_observation_and_keeps_the_reason() {
    let dismissal =
        "// @aqs(disable:\"Cross-File Duplication\") the hooks must not import each other\n";
    let tree = Tree::new(
        "observation-dismiss",
        &[
            ("a.ts", &reporter("a")),
            ("b.ts", &format!("{dismissal}{}", reporter("b"))),
        ],
    );
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(run.code, EXIT_OK);

    let report = run.json();
    let found = observations(&report);
    assert_eq!(
        found.len(),
        1,
        "dismissed, not deleted — a decision stays visible"
    );
    assert_eq!(
        found[0]["dismissed"], "the hooks must not import each other",
        "the reason travels with it"
    );

    // The lint output is what an editor and an agent read; a settled question is quiet.
    let check = aqs(&["check", tree.path().to_str().unwrap()]);
    assert!(
        !check.stdout.contains("Cross-File Duplication"),
        "a dismissed observation should not keep nagging: {}",
        check.stdout
    );
}

/// The one that would break silently.
///
/// Fingerprints ride along in the content-keyed cache, so a cache hit has to contribute
/// to the cross-file join without being re-analysed. If it did not, the observation would
/// simply vanish on the second run — and a scan that finds nothing looks exactly like a
/// scan that found nothing wrong.
#[test]
fn a_warm_cache_still_reports_observations() {
    let tree = Tree::new(
        "observation-cache",
        &[("a.ts", &reporter("a")), ("b.ts", &reporter("b"))],
    );
    let path = tree.path().to_str().unwrap().to_string();

    let cold = aqs(&["scan", &path, "--json"]);
    let warm = aqs(&["scan", &path, "--json"]);
    let uncached = aqs(&["scan", &path, "--json", "--no-cache"]);

    assert_eq!(observations(&cold.json()).len(), 1, "{}", cold.stdout);
    assert_eq!(cold.json(), warm.json(), "a warm cache changed the report");
    assert_eq!(cold.json(), uncached.json(), "caching changed the report");
}

#[test]
fn test_files_are_not_matched_against_each_other() {
    let tree = Tree::new(
        "observation-tests",
        &[("a.spec.ts", &reporter("a")), ("b.spec.ts", &reporter("b"))],
    );
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert!(
        observations(&run.json()).is_empty(),
        "suites repeat their setup on purpose: {}",
        run.stdout
    );
}

#[test]
fn functions_below_the_floor_are_not_matched() {
    let small = "export function close(state: State) {\n  state.open = false\n}\n";
    let tree = Tree::new("observation-floor", &[("a.ts", small), ("b.ts", small)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert!(
        observations(&run.json()).is_empty(),
        "a two-line setter repeated is not news: {}",
        run.stdout
    );
}

/// Fingerprints are how the join works, not something a consumer acts on.
#[test]
fn the_published_report_does_not_leak_fingerprints() {
    let tree = Tree::new(
        "observation-no-fingerprints",
        &[("a.ts", &reporter("a")), ("b.ts", &reporter("b"))],
    );
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    for file in run.json()["files"].as_array().expect("files") {
        assert!(
            file.get("fingerprints").is_none(),
            "internal detail in the report: {file}"
        );
    }
}

#[test]
fn guidance_explains_an_observation_that_fired() {
    let tree = Tree::new(
        "observation-guidance",
        &[("a.ts", &reporter("a")), ("b.ts", &reporter("b"))],
    );
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--guidance",
        "--no-cache",
    ]);
    let report = run.json();
    let guidance = &report["guidance"]["Cross-File Duplication"];
    assert!(
        guidance.is_object(),
        "an observation needs a fix to be actionable: {}",
        run.stdout
    );
    assert!(guidance["fix"]
        .as_str()
        .expect("fix")
        .contains("shared module"));
}

// --------------------------------------------------------------------------- //
// Linked git worktrees
// --------------------------------------------------------------------------- //

/// The `.git` file a linked worktree carries at its root.
const WORKTREE_MARKER: &str = "gitdir: /elsewhere/repo/.git/worktrees/feature\n";
/// The same file form, but a submodule — part of the project, so it must be scanned.
const SUBMODULE_MARKER: &str = "gitdir: /elsewhere/repo/.git/modules/vendored\n";

fn scanned_paths(report: &Value) -> Vec<String> {
    report["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|f| f["path"].as_str().expect("path").to_string())
        .collect()
}

/// A worktree is a second checkout of the same repository, so scanning one scores every
/// file twice — skewing the LoC-weighted aggregate and making every function collide
/// with its own original in the cross-file join.
#[test]
fn linked_worktrees_are_skipped_by_default() {
    let tree = Tree::new(
        "worktree-default",
        &[
            ("app.py", CLEAN),
            ("trees/feature/.git", WORKTREE_MARKER),
            ("trees/feature/app.py", CLEAN),
        ],
    );
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(scanned_paths(&run.json()), vec!["app.py"], "{}", run.stdout);
}

#[test]
fn include_worktrees_brings_them_back() {
    let tree = Tree::new(
        "worktree-opt-in",
        &[
            ("app.py", CLEAN),
            ("trees/feature/.git", WORKTREE_MARKER),
            ("trees/feature/app.py", CLEAN),
        ],
    );
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--include-worktrees",
        "--json",
        "--no-cache",
    ]);
    let paths = scanned_paths(&run.json());
    assert_eq!(paths.len(), 2, "{paths:?}");
    assert!(
        paths.contains(&"trees/feature/app.py".to_string()),
        "{paths:?}"
    );
}

#[test]
fn scan_worktrees_can_be_turned_on_in_config() {
    let tree = Tree::new(
        "worktree-config",
        &[
            (".aqs.json", r#"{"scan_worktrees": true}"#),
            ("app.py", CLEAN),
            ("trees/feature/.git", WORKTREE_MARKER),
            ("trees/feature/app.py", CLEAN),
        ],
    );
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(scanned_paths(&run.json()).len(), 2, "{}", run.stdout);
}

/// Pointing at a worktree directly is the escape hatch: the scan root is never pruned.
#[test]
fn a_worktree_named_as_the_target_is_still_scanned() {
    let tree = Tree::new(
        "worktree-explicit",
        &[("wt/.git", WORKTREE_MARKER), ("wt/app.py", CLEAN)],
    );
    let target = tree.path().join("wt");
    let run = aqs(&["scan", target.to_str().unwrap(), "--json", "--no-cache"]);
    assert_eq!(scanned_paths(&run.json()), vec!["app.py"], "{}", run.stdout);
}

/// Detection reads the `.git` file rather than matching a directory name, so a project
/// with real source in a directory called `trees/` is unaffected — and a submodule,
/// which uses the same file form, is part of the project and stays in scope.
#[test]
fn only_actual_worktrees_are_skipped() {
    let tree = Tree::new(
        "worktree-lookalikes",
        &[
            ("trees/binary_tree.py", CLEAN),
            ("vendored/.git", SUBMODULE_MARKER),
            ("vendored/lib.py", CLEAN),
            ("checkout/.git/config", "[core]\n"),
            ("checkout/app.py", CLEAN),
        ],
    );
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    let mut paths = scanned_paths(&run.json());
    paths.sort();
    assert_eq!(
        paths,
        vec!["checkout/app.py", "trees/binary_tree.py", "vendored/lib.py"],
        "a name, a submodule and a plain repo are all still source: {}",
        run.stdout
    );
}

#[test]
fn a_non_boolean_scan_worktrees_is_rejected() {
    let tree = Tree::new(
        "worktree-bad-config",
        &[
            (".aqs.json", r#"{"scan_worktrees": "yes"}"#),
            ("app.py", CLEAN),
        ],
    );
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("scan_worktrees"), "{}", run.stderr);
}

// --------------------------------------------------------------------------- //
// Duplicated Block
// --------------------------------------------------------------------------- //

/// A failure-handling block substantial enough to clear the token floor.
fn reporter_block(tag: &str) -> String {
    format!(
        "  if (!result.ok) {{\n    \
             logger.error('{tag} failed', {{ id, status: result.status }})\n    \
             metrics.increment('{tag}.failure', {{ status: result.status }})\n    \
             return null\n  \
           }}\n"
    )
}

/// The case the whole-function rules structurally cannot see.
///
/// `Code Duplication` compares whole functions with a 15-line floor and Cross-File
/// Duplication needs an entire function to match. A three-line block pasted into
/// functions that otherwise differ is invisible to both.
#[test]
fn a_block_repeated_across_different_functions_is_found() {
    let source = format!(
        "export async function loadClip(id: string) {{\n  \
           const result = await api.get(`/clips/${{id}}`)\n{}  \
           return result.data\n}}\n\n\
         export async function deleteClip(id: string) {{\n  \
           const result = await api.delete(`/clips/${{id}}`)\n{}  \
           return true\n}}\n",
        reporter_block("load"),
        reporter_block("load")
    );
    let tree = Tree::new("block-across-functions", &[("svc.ts", &source)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);

    let categories = reported_categories(&run.json());
    assert!(
        categories.contains(&"Duplicated Block".to_string()),
        "the surrounding functions differ, so only the block rule can see this: {}",
        run.stdout
    );
    assert!(
        !categories.contains(&"Code Duplication".to_string()),
        "the functions themselves are not duplicates: {}",
        run.stdout
    );
}

/// One repetition is one finding, however many windows fit inside it.
#[test]
fn a_long_repeated_block_is_one_finding_not_many() {
    // Deliberately *varied* statements. Eight identical ones would be a list, which the
    // rule declines to report — see `a_repeating_list_is_not_a_duplicated_block`.
    let body: String = (0..8)
        .map(|i| match i % 4 {
            0 => format!("    const step{i} = resolve(input[{i}], options)\n"),
            1 => format!("    if (!step{}) {{ return null }}\n", i - 1),
            2 => format!("    accumulator.push(transform(step{}, context))\n", i - 2),
            _ => format!("    logger.debug('step {i}', {{ size: accumulator.length }})\n"),
        })
        .collect();
    let source = format!(
        "export function first(input: string[]) {{\n  if (input.length) {{\n{body}  }}\n  return 1\n}}\n\n\
         export function second(input: string[]) {{\n  if (input.length) {{\n{body}  }}\n  return 2\n}}\n"
    );
    let tree = Tree::new("block-one-finding", &[("long.ts", &source)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);

    let report = run.json();
    let blocks: Vec<&Value> = report["files"][0]["function_level_code_smells"]
        .as_array()
        .expect("groups")
        .iter()
        .flat_map(|g| g["code_smells"].as_array().expect("smells"))
        .filter(|f| f["category"] == "Duplicated Block")
        .collect();

    assert_eq!(
        blocks.len(),
        1,
        "eight statements is one repetition: {}",
        run.stdout
    );
    assert!(
        blocks[0]["details"]
            .as_str()
            .unwrap()
            .starts_with("8 statements"),
        "the finding should describe the whole extent: {}",
        blocks[0]["details"]
    );
}

/// Repeated *definitions* belong to the function-level rules, not this one.
#[test]
fn consecutive_similar_functions_are_not_a_duplicated_block() {
    // Three Django-style views: decorator, decorator, def — the exact shape that made
    // an early version report a dozen findings per views.py.
    let view = |name: &str| {
        format!(
            "@api_view([\"GET\"])\n@permission_classes([IsAuthenticated])\ndef {name}(request):\n    \
               payload = build(request)\n    return Response(payload)\n\n"
        )
    };
    let source = format!("{}{}{}", view("alpha"), view("beta"), view("gamma"));
    let tree = Tree::new("block-definitions", &[("views.py", &source)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert!(
        !reported_categories(&run.json()).contains(&"Duplicated Block".to_string()),
        "runs of definitions are Code Duplication's territory: {}",
        run.stdout
    );
}

/// A Rust file must not score better for having no unit tests.
#[test]
fn rust_cfg_test_modules_are_exempt() {
    let case = |name: &str| {
        format!(
            "    #[test]\n    fn {name}() {{\n        \
                 let profile = Profile::default();\n        \
                 let result = analyse(&profile, SOURCE, LANGUAGE);\n        \
                 assert_eq!(result.score, 10.0);\n    }}\n"
        )
    };
    let source = format!(
        "pub fn add(a: i64, b: i64) -> i64 {{\n    a + b\n}}\n\n\
         #[cfg(test)]\nmod tests {{\n    use super::*;\n{}{}{}}}\n",
        case("one"),
        case("two"),
        case("three")
    );
    let tree = Tree::new("block-cfg-test", &[("lib.rs", &source)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert!(
        !reported_categories(&run.json()).contains(&"Duplicated Block".to_string()),
        "unit tests repeat their shape on purpose: {}",
        run.stdout
    );
}

#[test]
fn a_trivial_repeated_shape_is_below_the_token_floor() {
    let source = "export function f() {\n  const a = 1\n  const b = 2\n  const c = 3\n  \
                  return a\n}\n\nexport function g() {\n  const a = 1\n  const b = 2\n  \
                  const c = 3\n  return b\n}\n";
    let tree = Tree::new("block-trivial", &[("tiny.ts", source)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert!(
        !reported_categories(&run.json()).contains(&"Duplicated Block".to_string()),
        "three assignments is a shape, not an idea: {}",
        run.stdout
    );
}

#[test]
fn explain_covers_the_block_rule() {
    let run = aqs(&["explain", "Duplicated Block"]);
    assert_eq!(run.code, EXIT_OK);
    assert!(
        run.stdout.contains("duplicated_block_min_tokens"),
        "{}",
        run.stdout
    );
}

/// A dispatch list is periodic, not duplicated.
///
/// Nine calls of the same shape in a row means every window matches every other, so an
/// unguarded matcher reports the second half of the list as a copy of the first. This
/// fired on `detectors::detect`, which is a list of detector calls by design.
#[test]
fn a_repeating_list_is_not_a_duplicated_block() {
    let calls: String = (0..9)
        .map(|i| format!("  rule{i}(context, &mut findings, options)\n"))
        .collect();
    let source = format!(
        "export function detect(context: Ctx, options: Opts) {{\n{calls}  return findings\n}}\n"
    );
    let tree = Tree::new("block-dispatch-list", &[("detect.ts", &source)]);
    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert!(
        !reported_categories(&run.json()).contains(&"Duplicated Block".to_string()),
        "a list of same-shaped calls is not duplication: {}",
        run.stdout
    );
}

/// A branch that copies a function into a second file moves no score at all.
///
/// That is the entire reason the gate prints observations: the run on which this would
/// ever be noticed is a *passing* one, so gating on the score alone lets it through
/// silently while every file still reads 10.00.
#[test]
fn the_gate_reports_observations_a_branch_introduced() {
    let tree = Tree::new("gate-observations", &[("a.ts", &reporter("a"))]);
    tree.git_init();

    tree.write("b.ts", &reporter("b"));
    tree.commit("split the reporter out");

    let run = aqs_in(tree.path(), &["gate", "--against", "HEAD~1", "--json"]);
    assert_eq!(
        run.code, EXIT_OK,
        "an observation must not fail the gate: {}",
        run.stdout
    );

    let report = run.json();
    assert_eq!(
        report["score_change"], 0.0,
        "duplication is not a score change"
    );
    let introduced = report["observations_introduced"]
        .as_array()
        .expect("observations_introduced");
    assert_eq!(introduced.len(), 1, "{}", run.stdout);
    assert_eq!(introduced[0]["category"], "Cross-File Duplication");

    // And it has to be visible in the human output, on a pass.
    let human = aqs_in(tree.path(), &["gate", "--against", "HEAD~1"]);
    assert_outcome(
        &human,
        EXIT_OK,
        &["PASS", "Observations introduced", "Cross-File Duplication"],
    );
}

/// A pre-existing observation is not "introduced" just because lines moved.
#[test]
fn the_gate_ignores_observations_the_baseline_already_had() {
    let tree = Tree::new(
        "gate-observations-existing",
        &[("a.ts", &reporter("a")), ("b.ts", &reporter("b"))],
    );
    tree.git_init();

    // An unrelated edit above the duplicated function shifts its line numbers.
    tree.write(
        "a.ts",
        &format!("const VERSION = 1\nconst NAME = 'a'\n\n{}", reporter("a")),
    );
    tree.commit("add constants");

    let run = aqs_in(tree.path(), &["gate", "--against", "HEAD~1", "--json"]);
    let introduced = run.json()["observations_introduced"]
        .as_array()
        .expect("observations_introduced")
        .len();
    assert_eq!(
        introduced, 0,
        "a line shift is not a new observation: {}",
        run.stdout
    );
}

/// Dismissing an observation resolves it, exactly as fixing it would.
#[test]
fn dismissing_an_observation_is_not_introducing_one() {
    let tree = Tree::new(
        "gate-observations-dismissed",
        &[("a.ts", &reporter("a")), ("b.ts", &reporter("b"))],
    );
    tree.git_init();

    tree.write(
        "b.ts",
        &format!(
            "// @aqs(disable:\"Cross-File Duplication\") deliberate, the modules stay independent\n{}",
            reporter("b")
        ),
    );
    tree.commit("dismiss the duplication");

    let run = aqs_in(tree.path(), &["gate", "--against", "HEAD~1", "--json"]);
    assert_eq!(run.code, EXIT_OK);
    let introduced = run.json()["observations_introduced"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(introduced, 0, "{}", run.stdout);
}

// --------------------------------------------------------------------------- //
// Vue single-file components
// --------------------------------------------------------------------------- //

/// An SFC whose script holds one deeply nested function, preceded by a template
/// long enough that a mis-mapped line number is unmistakable.
fn sfc(lang: &str) -> String {
    let filler: String = (0..20).map(|i| format!("    <p>row {i}</p>\n")).collect();
    format!(
        "<template>\n{filler}</template>\n\n<script{lang}>\nexport default {{\n    methods: {{\n        tally(a, b, c) {{\n            let total = 0\n            if (a) {{\n                for (const i of b) {{\n                    while (i) {{\n                        if (i && a || c) {{\n                            for (const j of b) {{\n                                if (j) total += j\n                            }}\n                        }}\n                    }}\n                }}\n            }}\n            return total\n        }},\n    }},\n}}\n</script>\n\n<style scoped>\np {{ color: red; }}\n</style>\n"
    )
}

#[test]
fn scan_scores_the_script_of_a_single_file_component() {
    let tree = Tree::new("vue-scan", &[("Player.vue", &sfc(""))]);

    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let file = &run.json()["files"][0];
    assert_eq!(file["path"], "Player.vue");
    assert_eq!(file["language"], "javascript");
    assert!(
        !file["function_level_code_smells"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the nested method should be flagged: {}",
        run.stdout
    );
}

#[test]
fn a_finding_points_at_the_line_of_the_vue_file() {
    let tree = Tree::new("vue-lines", &[("Player.vue", &sfc(""))]);

    let run = aqs(&["check", tree.path().to_str().unwrap()]);
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    // Every line is where the .vue file has it: the method is declared on line 27
    // (22 template lines, a blank, the script tag, and two lines of object), and
    // the conditional it is flagged for is five lines further down. A finding that
    // pointed at the script's own line 4 would be useless to whoever opens the file.
    let reported = run.stdout.trim();
    assert!(
        reported.contains("Player.vue:27:9: Deep, Nested Complexity"),
        "a function-level finding belongs on the function's line: {reported}"
    );
    assert!(
        reported.contains("Player.vue:32:28: Complex Conditional"),
        "an expression-level finding belongs on the expression's line: {reported}"
    );
}

#[test]
fn a_typescript_block_is_scored_as_typescript() {
    let tree = Tree::new("vue-ts", &[("Player.vue", &sfc(" setup lang=\"ts\""))]);

    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);

    assert_eq!(
        run.json()["files"][0]["language"],
        "typescript",
        "{}",
        run.stdout
    );
}

#[test]
fn only_the_script_counts_as_lines_of_code() {
    let script = "<script>\nexport const a = 1\n</script>\n";
    let padded = format!(
        "<template>\n{}</template>\n{script}<style>\n{}</style>\n",
        (0..400)
            .map(|i| format!("    <p>row {i}</p>\n"))
            .collect::<String>(),
        (0..400)
            .map(|i| format!("p {{ margin: {i}px; }}\n"))
            .collect::<String>(),
    );
    let tree = Tree::new("vue-loc", &[("Big.vue", &padded)]);

    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);

    // 800 lines of markup and css, one line of code — and no "file too long".
    assert_eq!(run.json()["files"][0]["loc"], 1, "{}", run.stdout);
    assert_eq!(run.json()["score"], 10.0);
}

/// A component with nothing to score is an empty file, not a broken one — and is
/// treated exactly as an empty `.js` is, rather than as a case of its own.
#[test]
fn a_component_without_a_script_is_scored_as_empty() {
    let tree = Tree::new(
        "vue-empty",
        &[
            ("Static.vue", "<template>\n    <p>hi</p>\n</template>\n"),
            ("empty.js", ""),
        ],
    );

    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let files = run.json();
    let sfc = &files["files"][0];
    let empty = &files["files"][1];
    assert_eq!(sfc["path"], "Static.vue");
    assert!(
        sfc["error"].is_null(),
        "a template-only component is not an error: {}",
        run.stdout
    );
    assert_eq!(sfc["loc"], empty["loc"]);
    assert_eq!(sfc["score"], empty["score"]);
}

/// An SFC whose script declares a lot and does little — the wiring container
/// shape: one computed getter per value it forwards from the store.
fn wiring_component(getters: usize, methods: usize) -> String {
    let computed: String = (0..getters)
        .map(|i| format!("        value{i}() {{ return this.$store.getters['p/value{i}'] }},\n"))
        .collect();
    let handlers: String = (0..methods).map(|i| format!("        go{i}() {{}},\n")).collect();

    format!(
        "<template>\n    <p/>\n</template>\n\n<script>\nexport default {{\n    computed: {{\n{computed}    }},\n\n    methods: {{\n{handlers}    }},\n}}\n</script>\n"
    )
}

#[test]
fn a_component_is_counted_on_what_it_does_not_on_what_it_declares() {
    // Thirty getters forwarding to the store is a wiring layer, not a god
    // component: the reader holds three procedures, not thirty-three.
    let tree = Tree::new("vue-count", &[("Wiring.vue", &wiring_component(30, 3))]);

    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--json", "--no-cache"]);

    assert_eq!(run.json()["files"][0]["file_level_code_smells"], serde_json::json!([]));
}

#[test]
fn a_component_with_too_much_behaviour_is_still_flagged() {
    let tree = Tree::new("vue-count-bad", &[("God.vue", &wiring_component(3, 30))]);

    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--json", "--no-cache"]);

    let smell = &run.json()["files"][0]["file_level_code_smells"][0];
    assert_eq!(smell["category"], "Number of Functions in a Single Module");
    // The wording names what was counted, so the number can be reproduced by
    // counting the same things.
    assert_eq!(smell["details"], "30 methods and handlers");
}

#[test]
fn a_plain_module_is_still_counted_on_every_function() {
    let functions: String = (0..25).map(|i| format!("export const f{i} = () => {i}\n")).collect();
    let tree = Tree::new("js-count", &[("helpers.js", &functions)]);

    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--json", "--no-cache"]);

    assert_eq!(
        run.json()["files"][0]["file_level_code_smells"][0]["details"],
        "25 functions"
    );
}

#[test]
fn scan_discovers_go_files_and_skips_vendored_ones() {
    let deep = "package p\n\nfunc tally(a bool, b []int, c bool) int {\n\ttotal := 0\n\tif a {\n\t\tfor _, i := range b {\n\t\t\tfor i > 0 {\n\t\t\t\tif i > 1 && a || c {\n\t\t\t\t\tfor _, j := range b {\n\t\t\t\t\t\ttotal += j\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t\ti--\n\t\t\t}\n\t\t}\n\t}\n\treturn total\n}\n";
    let tree = Tree::new(
        "go-scan",
        &[
            ("tally.go", deep),
            ("vendor/github.com/x/dep.go", deep),
        ],
    );

    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let files = run.json()["files"].as_array().unwrap().clone();
    assert_eq!(files.len(), 1, "vendored code is not the project's: {}", run.stdout);
    assert_eq!(files[0]["path"], "tally.go");
    assert_eq!(files[0]["language"], "go");
    assert!(
        !files[0]["function_level_code_smells"].as_array().unwrap().is_empty(),
        "the nested function should be flagged: {}",
        run.stdout
    );
}

#[test]
fn scan_discovers_ruby_files_and_skips_generated_ones() {
    let deep = "class Tally\n  def run(a, xs, c)\n    total = 0\n    if a\n      xs.each do |x|\n        while x > 0\n          case x\n          when 1\n            if c\n              total += x\n            end\n          end\n          x -= 1\n        end\n      end\n    end\n    total\n  end\nend\n";
    let tree = Tree::new(
        "ruby-scan",
        &[
            ("app/models/tally.rb", deep),
            ("db/schema.rb", deep),
            ("spec/tally_spec.rb", deep),
        ],
    );

    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let files = run.json()["files"].as_array().unwrap().clone();
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec!["app/models/tally.rb", "spec/tally_spec.rb"]);
    assert_eq!(files[0]["language"], "ruby");
    assert!(
        !files[0]["function_level_code_smells"].as_array().unwrap().is_empty(),
        "the nested method should be flagged: {}",
        run.stdout
    );
}

#[test]
fn scan_scores_the_script_of_a_svelte_component() {
    // The same mechanism as a Vue SFC, and for the same reason: the script is the part
    // written in a language this tool parses.
    let filler: String = (0..20).map(|i| format!("  <p>row {i}</p>\n")).collect();
    let component = format!(
        "<script context=\"module\" lang=\"ts\">\n  export const ssr = true\n</script>\n\n<script lang=\"ts\">\n  export function tally(a: boolean, b: number[], c: boolean) {{\n    let sum = 0\n    if (a) {{\n      for (const i of b) {{\n        while (i > 0) {{\n          if (i && a || c) {{\n            for (const j of b) {{\n              if (j) sum += j\n            }}\n          }}\n        }}\n      }}\n    }}\n    return sum\n  }}\n</script>\n\n{filler}\n<style>\n  p {{ color: red; }}\n</style>\n"
    );
    let tree = Tree::new("svelte-scan", &[("Counter.svelte", &component)]);

    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let file = &run.json()["files"][0];
    assert_eq!(file["path"], "Counter.svelte");
    // `lang="ts"` decides the language, not the extension.
    assert_eq!(file["language"], "typescript");
    // Markup and style are not code this tool can explain a finding about.
    assert!(file["loc"].as_u64().unwrap() < 20, "only the script counts: {}", run.stdout);

    let flagged: Vec<&str> = file["function_level_code_smells"][0]["code_smells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["category"].as_str().unwrap())
        .collect();
    assert!(
        flagged.contains(&"Deep, Nested Complexity"),
        "the nested function should be flagged: {}",
        run.stdout
    );
    // The line number points at the line a reader will open, not at the script offset.
    let line = file["function_level_code_smells"][0]["range"]["start_line"].as_u64().unwrap();
    assert_eq!(line, 6, "{}", run.stdout);
}

#[test]
fn scan_discovers_java_and_php_files() {
    let java = "class Tally {\n    int run(boolean a, int[] xs, boolean c) {\n        int total = 0;\n        if (a) {\n            for (int x : xs) {\n                while (x > 0) {\n                    if (c) { total += x; }\n                    x--;\n                }\n            }\n        }\n        return total;\n    }\n}\n";
    let php = "<?php\nclass Tally\n{\n    function run($a, $xs, $c)\n    {\n        $total = 0;\n        if ($a) {\n            foreach ($xs as $x) {\n                while ($x > 0) {\n                    if ($c) { $total += $x; }\n                    $x--;\n                }\n            }\n        }\n        return $total;\n    }\n}\n";
    let tree = Tree::new(
        "jvm-php-scan",
        &[
            ("src/main/java/Tally.java", java),
            ("target/generated/Gen.java", java),
            ("src/Tally.php", php),
            ("vendor/composer/ClassLoader.php", php),
        ],
    );

    let run = aqs(&[
        "scan",
        tree.path().to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let files = run.json()["files"].as_array().unwrap().clone();
    let found: Vec<(&str, &str)> = files
        .iter()
        .map(|f| (f["path"].as_str().unwrap(), f["language"].as_str().unwrap()))
        .collect();
    assert_eq!(
        found,
        vec![("src/Tally.php", "php"), ("src/main/java/Tally.java", "java")],
        "build output and vendored code are not the project's: {}",
        run.stdout
    );
}
