//! Git primitives, and scoring a whole tree at an arbitrary revision.
//!
//! Scoring a past revision never checks anything out. `git ls-tree` lists the paths
//! and `git cat-file --batch` streams the blobs through a single process, so a
//! baseline scan costs no temp directory, no worktree, and no second clone.
//!
//! Blobs are read and analysed in chunks so peak memory stays bounded by the chunk
//! rather than by the size of the repository.

use crate::analyze::{analyze_jobs, AnalysisJob};
use crate::cache::Cache;
use crate::languages::spec_for_path;
use crate::models::{aggregate_score, FileReport, ScanReport};
use crate::profile::Profile;
use crate::VERSION;
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Paths per batched `cat-file` round trip. Bounds peak memory on large repos while
/// staying large enough that most repositories need only one pass.
pub const BLOB_CHUNK_SIZE: usize = 2048;

pub fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .context("git is not installed or not on PATH")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn repo_root(start: &Path) -> Result<PathBuf> {
    let directory = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent().unwrap_or(start).to_path_buf()
    };
    let output = git(&directory, &["rev-parse", "--show-toplevel"])?;
    let trimmed = output.trim();
    if trimmed.is_empty() {
        bail!("{} is not inside a git repository", directory.display());
    }
    Ok(PathBuf::from(trimmed))
}

/// The commit to compare against.
///
/// For a merge gate this must be the *merge base*, not the branch tip. Comparing
/// against the tip of `develop` would charge your branch for every change other people
/// merged while you were working; the common ancestor isolates your diff.
pub fn resolve_baseline(repo: &Path, reference: &str, use_merge_base: bool) -> Result<String> {
    if !use_merge_base {
        // Always resolve to a concrete SHA: a report that records "HEAD~5" as its
        // baseline is not reproducible tomorrow.
        let resolved = git(repo, &["rev-parse", "--verify", &format!("{reference}^{{commit}}")])?;
        let trimmed = resolved.trim().to_string();
        if trimmed.is_empty() {
            bail!("cannot resolve {reference}");
        }
        return Ok(trimmed);
    }
    let merge_base = git(repo, &["merge-base", "HEAD", reference])?;
    let trimmed = merge_base.trim().to_string();
    if trimmed.is_empty() {
        bail!("no common ancestor between HEAD and {reference}");
    }
    Ok(trimmed)
}

/// Repo-relative paths that differ from the baseline.
///
/// Deletions are included deliberately: removing a bad file is a real improvement, and
/// a gate that cannot see it would under-report progress. Untracked files are included
/// in working-tree mode, since freshly generated code is exactly what this exists to
/// check and it has no baseline at all.
pub fn changed_files(repo: &Path, baseline: &str, staged: bool) -> Result<Vec<String>> {
    const FILTER: &str = "--diff-filter=ACMRD";
    let mut paths: Vec<String> = if staged {
        git(repo, &["diff", "--name-only", "--cached", FILTER, baseline])?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(String::from)
            .collect()
    } else {
        let tracked = git(repo, &["diff", "--name-only", FILTER, baseline])?;
        let untracked = git(repo, &["ls-files", "--others", "--exclude-standard"])?;
        tracked
            .lines()
            .chain(untracked.lines())
            .filter(|line| !line.trim().is_empty())
            .map(String::from)
            .collect()
    };
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Read many git blobs through a single `git cat-file --batch` process.
///
/// Returns None for revisions that do not exist, which is how a newly added file is
/// represented.
pub fn read_blobs(repo: &Path, revisions: &[String]) -> Result<Vec<Option<Vec<u8>>>> {
    if revisions.is_empty() {
        return Ok(Vec::new());
    }

    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("git is not installed or not on PATH")?;

    // Write the request from a separate thread. Writing inline would deadlock as soon
    // as git's output fills the pipe buffer while we are still feeding it input.
    let request: Vec<u8> = revisions
        .iter()
        .flat_map(|revision| {
            let mut line = revision.clone().into_bytes();
            line.push(b'\n');
            line
        })
        .collect();
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&request);
        let _ = stdin.flush();
    });

    let output = child.wait_with_output().context("git cat-file failed")?;
    let _ = writer.join();

    let stream = output.stdout;
    let mut blobs: Vec<Option<Vec<u8>>> = Vec::with_capacity(revisions.len());
    let mut offset = 0usize;

    for _ in revisions {
        let Some(newline) = stream[offset..].iter().position(|&b| b == b'\n') else {
            blobs.push(None);
            continue;
        };
        let header = String::from_utf8_lossy(&stream[offset..offset + newline]).to_string();
        offset += newline + 1;

        // "<oid> missing" / "<oid> ambiguous" carry no payload.
        let size = header
            .rsplit(' ')
            .next()
            .and_then(|token| token.parse::<usize>().ok());
        let Some(size) = size else {
            blobs.push(None);
            continue;
        };
        let end = (offset + size).min(stream.len());
        blobs.push(Some(stream[offset..end].to_vec()));
        offset = end + 1; // payload is followed by a trailing newline
    }
    Ok(blobs)
}

/// Supported, non-excluded paths in a revision's tree, sorted.
pub fn tree_paths(repo: &Path, revision: &str, profile: &Profile) -> Result<Vec<String>> {
    let raw = git(repo, &["ls-tree", "-r", "--name-only", "-z", revision])?;
    let mut paths: Vec<String> = raw
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .filter(|path| spec_for_path(path).is_some() && !profile.exclude_patterns.matches(path))
        .map(String::from)
        .collect();
    paths.sort();
    Ok(paths)
}

/// Score every supported file in a revision's tree, without checking it out.
///
/// This is the cache's best case: a revision's blobs are immutable, so a second gate
/// run against the same baseline is served almost entirely from cache.
pub fn scan_revision(
    repo: &Path,
    revision: &str,
    profile: &Profile,
    cache: &mut dyn Cache,
) -> Result<ScanReport> {
    let paths = tree_paths(repo, revision, profile)?;
    let mut reports: Vec<FileReport> = Vec::new();

    for chunk in paths.chunks(BLOB_CHUNK_SIZE) {
        let revisions: Vec<String> =
            chunk.iter().map(|path| format!("{revision}:{path}")).collect();
        let blobs = read_blobs(repo, &revisions)?;

        let mut jobs: Vec<AnalysisJob> = Vec::new();
        for (path, blob) in chunk.iter().zip(blobs) {
            let Some(source) = blob else { continue };
            let spec = spec_for_path(path).expect("tree_paths already filtered on this");
            jobs.push(AnalysisJob { path: path.clone(), language: spec.name, source });
        }
        reports.extend(analyze_jobs(&jobs, profile, cache));
    }

    let scored: Vec<FileReport> =
        reports.iter().filter(|r| r.error.is_none()).cloned().collect();
    Ok(ScanReport {
        version: VERSION.to_string(),
        profile: profile.name.clone(),
        score: aggregate_score(&scored),
        loc: scored.iter().map(|r| r.loc).sum(),
        files: reports,
    })
}
