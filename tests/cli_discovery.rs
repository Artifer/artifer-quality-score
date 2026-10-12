//! End-to-end tests for what a scan picks up: linked worktrees, single-file
//! components, and each supported language.

mod common;

use common::*;
use serde_json::Value;

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
    let run = tree.scan_json();
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
    let run = tree.scan(&["--include-worktrees", "--json"]);
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
    let run = tree.scan_json();
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
    let run = tree.scan_json();
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
    let run = tree.scan(&[]);
    assert_eq!(run.code, EXIT_ERROR);
    assert!(run.stderr.contains("scan_worktrees"), "{}", run.stderr);
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

    let run = tree.scan_json();
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

    let run = tree.scan_json();

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

    let run = tree.scan_json();

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

    let run = tree.scan_json();
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
    let handlers: String = (0..methods)
        .map(|i| format!("        go{i}() {{}},\n"))
        .collect();

    format!(
        "<template>\n    <p/>\n</template>\n\n<script>\nexport default {{\n    computed: {{\n{computed}    }},\n\n    methods: {{\n{handlers}    }},\n}}\n</script>\n"
    )
}

#[test]
fn a_component_is_counted_on_what_it_does_not_on_what_it_declares() {
    // Thirty getters forwarding to the store is a wiring layer, not a god
    // component: the reader holds three procedures, not thirty-three.
    let tree = Tree::new("vue-count", &[("Wiring.vue", &wiring_component(30, 3))]);

    let run = tree.scan_json();

    assert_eq!(
        run.json()["files"][0]["file_level_code_smells"],
        serde_json::json!([])
    );
}

#[test]
fn a_component_with_too_much_behaviour_is_still_flagged() {
    let tree = Tree::new("vue-count-bad", &[("God.vue", &wiring_component(3, 30))]);

    let run = tree.scan_json();

    let smell = &run.json()["files"][0]["file_level_code_smells"][0];
    assert_eq!(smell["category"], "Number of Functions in a Single Module");
    // The wording names what was counted, so the number can be reproduced by
    // counting the same things.
    assert_eq!(smell["details"], "30 methods and handlers");
}

#[test]
fn a_plain_module_is_still_counted_on_every_function() {
    let functions: String = (0..25)
        .map(|i| format!("export const f{i} = () => {i}\n"))
        .collect();
    let tree = Tree::new("js-count", &[("helpers.js", &functions)]);

    let run = tree.scan_json();

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
        &[("tally.go", deep), ("vendor/github.com/x/dep.go", deep)],
    );

    let run = tree.scan_json();
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let files = run.json()["files"].as_array().unwrap().clone();
    assert_eq!(
        files.len(),
        1,
        "vendored code is not the project's: {}",
        run.stdout
    );
    assert_eq!(files[0]["path"], "tally.go");
    assert_eq!(files[0]["language"], "go");
    assert!(
        !files[0]["function_level_code_smells"]
            .as_array()
            .unwrap()
            .is_empty(),
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

    let run = tree.scan_json();
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let files = run.json()["files"].as_array().unwrap().clone();
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec!["app/models/tally.rb", "spec/tally_spec.rb"]);
    assert_eq!(files[0]["language"], "ruby");
    assert!(
        !files[0]["function_level_code_smells"]
            .as_array()
            .unwrap()
            .is_empty(),
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

    let run = tree.scan_json();
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let file = &run.json()["files"][0];
    assert_eq!(file["path"], "Counter.svelte");
    // `lang="ts"` decides the language, not the extension.
    assert_eq!(file["language"], "typescript");
    // Markup and style are not code this tool can explain a finding about.
    assert!(
        file["loc"].as_u64().unwrap() < 20,
        "only the script counts: {}",
        run.stdout
    );

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
    let line = file["function_level_code_smells"][0]["range"]["start_line"]
        .as_u64()
        .unwrap();
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

    let run = tree.scan_json();
    assert_eq!(run.code, EXIT_OK, "{}", run.stderr);

    let files = run.json()["files"].as_array().unwrap().clone();
    let found: Vec<(&str, &str)> = files
        .iter()
        .map(|f| (f["path"].as_str().unwrap(), f["language"].as_str().unwrap()))
        .collect();
    assert_eq!(
        found,
        vec![
            ("src/Tally.php", "php"),
            ("src/main/java/Tally.java", "java")
        ],
        "build output and vendored code are not the project's: {}",
        run.stdout
    );
}
