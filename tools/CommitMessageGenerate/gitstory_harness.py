#!/usr/bin/env python3
"""Export Git commit batches for generated history/story review.

The harness never rewrites history. It creates deterministic JSONL batches that
agents can consume, plus a manifest that makes coverage easy to verify.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


WEAK_MESSAGES = {
    "",
    ".",
    "ok",
    "okay",
    "k",
    "yes",
    "y",
    "no",
    "n",
    "fix",
    "fixes",
    "fixed",
    "wip",
    "work",
    "tmp",
    "temp",
    "update",
    "changes",
    "stuff",
    "commit",
}


def git(args: list[str], cwd: Path, *, text: bool = True) -> str | bytes:
    result = subprocess.run(
        ["git", *args],
        cwd=cwd,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=text,
    )
    return result.stdout


def git_optional(args: list[str], cwd: Path) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        return ""
    return result.stdout


def weak_message(message: str) -> bool:
    subject = message.strip().splitlines()[0] if message.strip() else ""
    normalized = subject.lower().strip()
    if normalized in WEAK_MESSAGES:
        return True
    return len(normalized.split()) <= 2 and len(normalized) <= 12


def truncate_text(value: str, max_bytes: int) -> tuple[str, bool]:
    raw = value.encode("utf-8", errors="replace")
    if len(raw) <= max_bytes:
        return value, False
    clipped = raw[:max_bytes].decode("utf-8", errors="ignore")
    return clipped + "\n\n[diff truncated by gitstory_harness]", True


def parse_numstat(output: str) -> list[dict[str, object]]:
    files: list[dict[str, object]] = []
    for line in output.splitlines():
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        added, deleted, path = parts[0], parts[1], "\t".join(parts[2:])
        files.append(
            {
                "path": path,
                "added": None if added == "-" else int(added),
                "deleted": None if deleted == "-" else int(deleted),
            }
        )
    return files


def commit_record(
    cwd: Path,
    sha: str,
    index: int,
    total: int,
    max_diff_bytes: int,
    excludes: list[str],
) -> dict[str, object]:
    message = git(["show", "-s", "--format=%B", sha], cwd).rstrip("\n")
    subject = message.strip().splitlines()[0] if message.strip() else ""
    parents = git(["show", "-s", "--format=%P", sha], cwd).strip().split()
    author = git(["show", "-s", "--format=%an <%ae>", sha], cwd).strip()
    author_date = git(["show", "-s", "--format=%aI", sha], cwd).strip()
    committer_date = git(["show", "-s", "--format=%cI", sha], cwd).strip()

    pathspec = [f":(exclude){item}" for item in excludes]
    stat = git_optional(["show", "--format=", "--stat", "--summary", sha, "--", *pathspec], cwd).strip()
    numstat = git_optional(["show", "--format=", "--numstat", sha, "--", *pathspec], cwd)
    diff = git_optional(
        [
            "show",
            "--format=",
            "--find-renames",
            "--find-copies",
            "--patch",
            "--no-ext-diff",
            sha,
            "--",
            *pathspec,
        ],
        cwd,
    )
    diff, diff_truncated = truncate_text(diff, max_diff_bytes)

    return {
        "index": index,
        "total": total,
        "sha": sha,
        "parents": parents,
        "author": author,
        "author_date": author_date,
        "committer_date": committer_date,
        "original_message": message,
        "original_subject": subject,
        "weak_original_message": weak_message(message),
        "files": parse_numstat(numstat),
        "stat": stat,
        "diff_truncated": diff_truncated,
        "diff": diff,
        "generated_message": "",
        "story_notes": "",
    }


def write_jsonl(path: Path, records: list[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for record in records:
            handle.write(json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n")


def export(args: argparse.Namespace) -> int:
    cwd = Path(args.repo).resolve()
    out = Path(args.out).resolve()
    rev_args = ["rev-list", "--reverse", args.revs]
    commits = [line for line in git(rev_args, cwd).splitlines() if line]
    if args.limit:
        commits = commits[: args.limit]

    out.mkdir(parents=True, exist_ok=True)
    records: list[dict[str, object]] = []
    total = len(commits)

    for index, sha in enumerate(commits, start=1):
        records.append(commit_record(cwd, sha, index, total, args.max_diff_bytes, args.exclude))
        if args.progress and (index == total or index % args.progress == 0):
            print(f"exported {index}/{total}", file=sys.stderr)

    write_jsonl(out / "commits.jsonl", records)

    batch_paths: list[Path] = []
    for batch_index, start in enumerate(range(0, len(records), args.batch_size), start=1):
        batch = records[start : start + args.batch_size]
        batch_path = out / "batches" / f"batch_{batch_index:03d}.jsonl"
        write_jsonl(batch_path, batch)
        batch_paths.append(batch_path)

    manifest_lines = [
        "# Git Story Harness Manifest",
        "",
        f"- repo: `{cwd}`",
        f"- revs: `{args.revs}`",
        f"- commits: `{len(records)}`",
        f"- batch_size: `{args.batch_size}`",
        f"- batches: `{len(batch_paths)}`",
        f"- max_diff_bytes: `{args.max_diff_bytes}`",
        f"- excludes: `{', '.join(args.exclude) if args.exclude else '(none)'}`",
        "",
        "## Batch Files",
        "",
    ]
    for path in batch_paths:
        manifest_lines.append(f"- `{path.relative_to(out)}`")
    manifest_lines.extend(
        [
            "",
            "## Agent Contract",
            "",
            "For each JSONL record, preserve `original_message` exactly.",
            "Fill `generated_message` with a diff-grounded commit message.",
            "Use `story_notes` for cross-commit narrative context or uncertainty.",
            "Do not rewrite Git history from these files until the generated output is reviewed.",
            "",
        ]
    )
    (out / "manifest.md").write_text("\n".join(manifest_lines), encoding="utf-8")

    print(f"wrote {len(records)} commits into {out}")
    print(f"wrote {len(batch_paths)} batches")
    return 0


def validate(args: argparse.Namespace) -> int:
    cwd = Path(args.repo).resolve()
    out = Path(args.out).resolve()
    commits_path = out / "commits.jsonl"
    if not commits_path.exists():
        print(f"missing {commits_path}", file=sys.stderr)
        return 1

    expected = [line for line in git(["rev-list", "--reverse", args.revs], cwd).splitlines() if line]
    if args.limit:
        expected = expected[: args.limit]
    actual: list[str] = []
    weak = 0
    truncated = 0
    with commits_path.open(encoding="utf-8") as handle:
        for line_no, line in enumerate(handle, start=1):
            record = json.loads(line)
            sha = record.get("sha")
            if not isinstance(sha, str):
                print(f"line {line_no}: missing sha", file=sys.stderr)
                return 1
            actual.append(sha)
            weak += 1 if record.get("weak_original_message") else 0
            truncated += 1 if record.get("diff_truncated") else 0

    problems: list[str] = []
    if actual != expected:
        problems.append("commits.jsonl order/content does not match git rev-list")
    if len(actual) != len(set(actual)):
        problems.append("commits.jsonl contains duplicate shas")

    batch_count = sum(1 for _ in (out / "batches").glob("batch_*.jsonl"))
    if batch_count == 0 and actual:
        problems.append("no batch files found")

    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        return 1

    print(f"valid: {len(actual)} commits, {batch_count} batches")
    print(f"weak original messages: {weak}")
    print(f"truncated diffs: {truncated}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    export_parser = subparsers.add_parser("export", help="export commit records and batches")
    export_parser.add_argument("--repo", default=".", help="repository path")
    export_parser.add_argument("--out", default=".gitstory", help="output directory")
    export_parser.add_argument("--revs", default="HEAD", help="revision range for git rev-list")
    export_parser.add_argument("--batch-size", type=int, default=200)
    export_parser.add_argument("--max-diff-bytes", type=int, default=120_000)
    export_parser.add_argument("--exclude", action="append", default=[], help="pathspec to exclude")
    export_parser.add_argument("--limit", type=int, default=0, help="limit commits for a smoke test")
    export_parser.add_argument("--progress", type=int, default=50, help="stderr progress interval")
    export_parser.set_defaults(func=export)

    validate_parser = subparsers.add_parser("validate", help="validate exported coverage")
    validate_parser.add_argument("--repo", default=".", help="repository path")
    validate_parser.add_argument("--out", default=".gitstory", help="output directory")
    validate_parser.add_argument("--revs", default="HEAD", help="revision range for git rev-list")
    validate_parser.add_argument("--limit", type=int, default=0, help="match an exported smoke-test limit")
    validate_parser.set_defaults(func=validate)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
