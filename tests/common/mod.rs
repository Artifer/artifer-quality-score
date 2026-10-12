//! What every end-to-end suite shares: a throwaway tree, a way to run the binary, and
//! the exit codes automation binds to.
//!
//! No test mutates the process working directory. Where a test needs to run "inside" a
//! directory it sets the *child's* cwd, so the suites stay safe under `cargo test`'s
//! default parallelism.

// Each suite is its own crate and uses a different subset of these helpers.
#![allow(dead_code)]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const AQS: &str = env!("CARGO_BIN_EXE_aqs");

pub const CLEAN: &str = "def add(a: int, b: int) -> int:\n    \"\"\"Add.\"\"\"\n    return a + b\n";
pub const MESSY: &str = "def f(a, b, c, d, e, f):
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
pub struct Tree(PathBuf);

impl Tree {
    pub fn new(name: &str, files: &[(&str, &str)]) -> Self {
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

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Scan the tree uncached, with any extra `flags`.
    pub fn scan(&self, flags: &[&str]) -> Run {
        let mut args = vec!["scan", self.0.to_str().expect("utf-8 path"), "--no-cache"];
        args.extend_from_slice(flags);
        aqs(&args)
    }

    /// Scan the tree uncached and ask for the JSON report.
    pub fn scan_json(&self) -> Run {
        self.scan(&["--json"])
    }

    pub fn write(&self, relative: &str, body: &str) {
        std::fs::write(self.0.join(relative), body).expect("write");
    }

    /// Turn the tree into a git repository with one commit.
    pub fn git_init(&self) -> &Self {
        self.git(&["init", "-q", "-b", "develop"]);
        self.git(&["config", "user.email", "t@example.com"]);
        self.git(&["config", "user.name", "T"]);
        self.commit("initial");
        self
    }

    pub fn git(&self, args: &[&str]) {
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

    pub fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", self.stdout))
    }
}

pub fn from(output: Output) -> Run {
    Run {
        code: output.status.code().expect("process exited normally"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Run `aqs` with the given arguments.
pub fn aqs(args: &[&str]) -> Run {
    from(Command::new(AQS).args(args).output().expect("aqs runs"))
}

/// Run `aqs` as though the shell were sitting in `cwd`.
pub fn aqs_in(cwd: &Path, args: &[&str]) -> Run {
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

pub const EXIT_OK: i32 = 0;
pub const EXIT_GATE_FAILED: i32 = 1;
pub const EXIT_ERROR: i32 = 2;

/// Assert an exit code and that stdout mentions each of `expected`.
///
/// A named matcher rather than the same three asserts in every gate test: when it fails
/// you learn which phrase was missing, and there is one place to change.
pub fn assert_outcome(run: &Run, code: i32, expected: &[&str]) {
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

/// Every rule name that appears anywhere in a scan report.
pub fn reported_categories(report: &Value) -> Vec<String> {
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
