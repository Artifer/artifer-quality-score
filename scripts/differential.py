"""Differential test: the Rust port must agree with the Python oracle exactly.

Compares `aqs scan --json` from both implementations file by file, finding by
finding. Any disagreement is a bug in the port, measured against a known-good
reference.
"""

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PY = REPO / ".venv/bin/aqs"
RS = REPO / "rust/target/release/aqs"


def run(binary, target):
    result = subprocess.run(
        [str(binary), "scan", str(target), "--json", "--no-cache"],
        capture_output=True,
        text=True,
    )
    if result.returncode not in (0, 1):
        print(f"  {binary.name} failed: {result.stderr[:800]}")
        return None
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        print(f"  {binary.name} emitted invalid JSON: {exc}")
        print(result.stdout[:500])
        return None


def finding_key(finding):
    return (
        finding["category"],
        finding.get("function"),
        finding["highlight_range"]["start_line"],
    )


def flatten(file_report):
    findings = list(file_report["file_level_code_smells"])
    for group in file_report["function_level_code_smells"]:
        findings.extend(group["code_smells"])
    return {finding_key(f): f for f in findings}


def compare(target):
    print(f"\n=== {target}")
    py, rs = run(PY, target), run(RS, target)
    if py is None or rs is None:
        return False

    problems = []
    if py["score"] != rs["score"]:
        problems.append(f"AGGREGATE score {py['score']} vs {rs['score']}")
    if py["loc"] != rs["loc"]:
        problems.append(f"AGGREGATE loc {py['loc']} vs {rs['loc']}")

    py_files = {f["path"]: f for f in py["files"]}
    rs_files = {f["path"]: f for f in rs["files"]}

    only_py = set(py_files) - set(rs_files)
    only_rs = set(rs_files) - set(py_files)
    for path in sorted(only_py):
        problems.append(f"missing in rust: {path}")
    for path in sorted(only_rs):
        problems.append(f"extra in rust: {path}")

    for path in sorted(set(py_files) & set(rs_files)):
        a, b = py_files[path], rs_files[path]
        for field in ("score", "loc", "language", "syntax_errors"):
            if a.get(field) != b.get(field):
                problems.append(f"{path}: {field} {a.get(field)!r} vs {b.get(field)!r}")
        fa, fb = flatten(a), flatten(b)
        for key in sorted(set(fa) - set(fb), key=str):
            problems.append(f"{path}: finding only in python: {key} {fa[key]['details']!r}")
        for key in sorted(set(fb) - set(fa), key=str):
            problems.append(f"{path}: finding only in rust:   {key} {fb[key]['details']!r}")
        for key in sorted(set(fa) & set(fb), key=str):
            for field in ("details", "penalty", "value", "threshold"):
                if fa[key].get(field) != fb[key].get(field):
                    problems.append(
                        f"{path}: {key} {field}: {fa[key].get(field)!r} vs {fb[key].get(field)!r}"
                    )

    files = len(py_files)
    findings = sum(len(flatten(f)) for f in py_files.values())
    if problems:
        print(f"  {len(problems)} MISMATCH(ES) over {files} files / {findings} findings")
        for line in problems[:30]:
            print(f"    {line}")
        if len(problems) > 30:
            print(f"    ... and {len(problems) - 30} more")
        return False

    print(f"  IDENTICAL — {files} files, {findings} findings, score {py['score']}")
    return True


def main():
    targets = sys.argv[1:]
    if not targets:
        print(__doc__)
        print("usage: python scripts/differential.py <path> [<path> ...]")
        return 2
    for binary in (PY, RS):
        if not binary.exists():
            print(f"missing {binary} — build it first "
                  f"(uv pip install -e . / cargo build --release)")
            return 2
    ok = all(compare(Path(t)) for t in targets)
    print("\n" + ("ALL TARGETS IDENTICAL" if ok else "DIFFERENCES FOUND"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
