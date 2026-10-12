//! End-to-end tests for duplication: the unscored cross-file observations, the
//! `Duplicated Block` rule, and how the gate reports what a branch introduced.

mod common;

use common::*;
use serde_json::Value;

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
    let run = tree.scan_json();
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
    let run = tree.scan_json();
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
    let run = tree.scan_json();
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
    let run = tree.scan_json();
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
    let run = tree.scan_json();
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
    let run = tree.scan_json();
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
    let run = tree.scan(&["--json", "--guidance"]);
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

/// Scan `source` as a single file and assert the block rule stayed quiet about it.
///
/// `why` is the reason this shape is not duplication, shown when the assertion fails.
fn assert_no_duplicated_block(file: &str, source: &str, why: &str) {
    let tree = Tree::new(&format!("block-quiet-{file}"), &[(file, source)]);
    let run = tree.scan_json();
    assert!(
        !reported_categories(&run.json()).contains(&"Duplicated Block".to_string()),
        "{why}: {}",
        run.stdout
    );
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
    let run = tree.scan_json();

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
    let run = tree.scan_json();

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
    assert_no_duplicated_block(
        "views.py",
        &source,
        "runs of definitions are Code Duplication's territory",
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
    assert_no_duplicated_block(
        "lib.rs",
        &source,
        "unit tests repeat their shape on purpose",
    );
}

#[test]
fn a_trivial_repeated_shape_is_below_the_token_floor() {
    let source = "export function f() {\n  const a = 1\n  const b = 2\n  const c = 3\n  \
                  return a\n}\n\nexport function g() {\n  const a = 1\n  const b = 2\n  \
                  const c = 3\n  return b\n}\n";
    assert_no_duplicated_block(
        "tiny.ts",
        source,
        "three assignments is a shape, not an idea",
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
    assert_no_duplicated_block(
        "detect.ts",
        &source,
        "a list of same-shaped calls is not duplication",
    );
}

/// A committed repository holding the same reporter in two files.
fn duplicated_pair_repository(name: &str) -> Tree {
    let tree = Tree::new(name, &[("a.ts", &reporter("a")), ("b.ts", &reporter("b"))]);
    tree.git_init();
    tree
}

/// Gate the last commit against the one before it, as JSON.
fn gate_last_commit(tree: &Tree) -> Run {
    aqs_in(tree.path(), &["gate", "--against", "HEAD~1", "--json"])
}

/// The observations a gate run says the branch introduced.
fn introduced(run: &Run) -> Vec<Value> {
    run.json()["observations_introduced"]
        .as_array()
        .expect("observations_introduced")
        .clone()
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

    let run = gate_last_commit(&tree);
    assert_eq!(
        run.code, EXIT_OK,
        "an observation must not fail the gate: {}",
        run.stdout
    );
    assert_eq!(
        run.json()["score_change"],
        0.0,
        "duplication is not a score change"
    );
    let introduced = introduced(&run);
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
    let tree = duplicated_pair_repository("gate-observations-existing");

    // An unrelated edit above the duplicated function shifts its line numbers.
    tree.write(
        "a.ts",
        &format!("const VERSION = 1\nconst NAME = 'a'\n\n{}", reporter("a")),
    );
    tree.commit("add constants");

    let run = gate_last_commit(&tree);
    assert_eq!(
        introduced(&run).len(),
        0,
        "a line shift is not a new observation: {}",
        run.stdout
    );
}

/// Dismissing an observation resolves it, exactly as fixing it would.
#[test]
fn dismissing_an_observation_is_not_introducing_one() {
    let tree = duplicated_pair_repository("gate-observations-dismissed");

    tree.write(
        "b.ts",
        &format!(
            "// @aqs(disable:\"Cross-File Duplication\") deliberate, the modules stay independent\n{}",
            reporter("b")
        ),
    );
    tree.commit("dismiss the duplication");

    let run = gate_last_commit(&tree);
    assert_eq!(run.code, EXIT_OK);
    assert_eq!(introduced(&run).len(), 0, "{}", run.stdout);
}
