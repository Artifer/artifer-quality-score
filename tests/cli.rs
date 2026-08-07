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
        assert!(status.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&status.stderr));
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
    assert_eq!(run.code, code, "unexpected exit code. stdout:\n{}", run.stdout);
    for phrase in expected {
        assert!(run.stdout.contains(phrase), "stdout lacks {phrase:?}:\n{}", run.stdout);
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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--json", "--no-cache"]);
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
    for key in ["category", "details", "penalty", "highlight_range", "value", "threshold"] {
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
    assert!(fails.stderr.contains("below the required"), "{}", fails.stderr);
}

// --------------------------------------------------------------------------- //
// check
// --------------------------------------------------------------------------- //

#[test]
fn check_emits_parseable_lint_lines() {
    let tree = Tree::new("check", &[("messy.py", MESSY)]);
    let run = aqs(&["check", tree.path().to_str().unwrap()]);
    assert_eq!(run.code, EXIT_OK);

    let lines: Vec<&str> = run.stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!lines.is_empty(), "expected findings");
    for line in lines {
        // path:line:col: Rule: details
        let (location, rest) = line.split_once(": ").unwrap_or_else(|| panic!("{line}"));
        let parts: Vec<&str> = location.rsplitn(3, ':').collect();
        assert_eq!(parts.len(), 3, "{line}");
        assert!(parts[0].parse::<u32>().is_ok(), "column not numeric: {line}");
        assert!(parts[1].parse::<u32>().is_ok(), "line not numeric: {line}");
        assert!(!rest.is_empty(), "{line}");
    }
}

#[test]
fn check_can_fail_on_findings() {
    let dirty = Tree::new("check-dirty", &[("messy.py", MESSY)]);
    let run = aqs(&["check", dirty.path().to_str().unwrap(), "--fail-on-findings"]);
    assert_eq!(run.code, EXIT_GATE_FAILED);

    let clean = Tree::new("check-clean", &[("clean.py", CLEAN)]);
    let run = aqs(&["check", clean.path().to_str().unwrap(), "--fail-on-findings"]);
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
    assert!(fuzzy.stdout.contains("Bumpy Road Ahead"), "{}", fuzzy.stdout);
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
    assert_eq!(rules.len(), 19, "expected the full catalogue");
    for (name, entry) in rules {
        for key in ["summary", "why", "fix", "weight", "thresholds"] {
            assert!(entry.get(key).is_some(), "{name} lacks {key}");
        }
    }
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
    for key in ["baseline", "old_score", "new_score", "score_change", "files"] {
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
    let strict = aqs(&["gate", "--path", &path, "--against", "develop", "--no-cache"]);
    let lenient = aqs(&[
        "gate", "--path", &path, "--against", "develop", "--tolerance", "10", "--no-cache",
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
            (".aqs.json", r#"{"name":"quiet","rules":{"Deep, Nested Complexity":0.0,"Complex Method":0.0,"Bumpy Road Ahead":0.0,"Complex Conditional":0.0,"Excess Number of Function Arguments":0.0}}"#),
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
        &[(".aqs.json", r#"{"rules":{"Complex Methd":0.0}}"#), ("a.py", CLEAN)],
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
        &[(".aqs.json", r#"{"rule":{"Complex Method":0.0}}"#), ("a.py", CLEAN)],
    );
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("unknown config section"), "{}", run.stderr);
}

#[test]
fn invalid_config_json_is_reported_with_its_path() {
    let tree = Tree::new("config-bad-json", &[(".aqs.json", "{not json"), ("a.py", CLEAN)]);
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
    assert!(tree.path().join(".aqs").exists(), "expected a cache directory");
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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--json", "--no-cache"]);
    assert_eq!(run.code, EXIT_OK);

    let file = &run.json()["files"][0];
    let categories = reported_categories(&run.json());
    assert!(
        !categories.contains(&"Complex Method".to_string()),
        "Complex Method should be suppressed: {categories:?}"
    );
    // Other rules still fire — a directive names what it disables and nothing more.
    assert!(categories.contains(&"Large Method".to_string()), "{categories:?}");

    let suppressed = file["suppressed"].as_array().expect("a suppressed list");
    assert_eq!(suppressed.len(), 1);
    assert_eq!(suppressed[0]["category"], "Complex Method");
    assert_eq!(suppressed[0]["function"], "f");
    assert!(suppressed[0]["reason"].as_str().unwrap().contains("dispatch table"));
}

#[test]
fn suppression_is_counted_in_the_human_report() {
    // A branch must not be able to zero its findings invisibly.
    let body = offender("f");
    let tree = Tree::new(
        "directive-visible",
        &[("a.py", &format!("# @aqs(disable-all) rewritten next sprint\n{body}"))],
    );
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
    assert_outcome(&run, EXIT_OK, &["suppressed by @aqs directives"]);
}

#[test]
fn disable_all_covers_every_rule_for_that_function() {
    let body = offender("f");
    let tree = Tree::new(
        "directive-all",
        &[("a.py", &format!("# @aqs(disable-all) generated, do not edit\n{body}"))],
    );
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--json", "--no-cache"]);
    assert!(reported_categories(&run.json()).is_empty(), "{}", run.stdout);
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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--json", "--no-cache"]);
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
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--json", "--no-cache"]);
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
        &[("a.py", &format!("# @aqs(disable:\"Complex Method\")\n{body}"))],
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
        &[("a.py", &format!("# @aqs(disable:\"Complex Methd\") typo\n{body}"))],
    );
    let run = aqs(&["scan", tree.path().to_str().unwrap(), "--no-cache"]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stdout.contains("unknown rule"), "{}", run.stdout);
    assert!(run.stdout.contains("a.py:1"), "should locate it: {}", run.stdout);
}

#[test]
fn a_broken_directive_cannot_lower_a_score() {
    // Fail-safe: the file is scored as though the directive were absent.
    let body = offender("f");
    let broken = Tree::new(
        "directive-failsafe",
        &[("a.py", &format!("# @aqs(disable:\"Complex Methd\") typo\n{body}"))],
    );
    let none = Tree::new("directive-none", &[("a.py", &format!("# ordinary\n{body}"))]);

    let with_broken = aqs(&["scan", broken.path().to_str().unwrap(), "--json", "--no-cache"]);
    let without = aqs(&["scan", none.path().to_str().unwrap(), "--json", "--no-cache"]);
    assert_eq!(
        with_broken.json()["score"], without.json()["score"],
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
        let run = aqs(&["scan", tree.path().to_str().unwrap(), "--json", "--no-cache"]);
        assert!(
            reported_categories(&run.json()).is_empty(),
            "{name} directive ignored: {}",
            run.stdout
        );
    }
}
