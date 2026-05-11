#!/usr/bin/env python3
"""Export Git commit batches for generated history/story review.

The harness never rewrites history. It creates deterministic JSONL batches that
agents can consume, plus a manifest that makes coverage easy to verify.
"""

from __future__ import annotations

import argparse
import json
import re
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


def read_jsonl(path: Path) -> list[dict[str, object]]:
    records: list[dict[str, object]] = []
    with path.open(encoding="utf-8") as handle:
        for line_no, line in enumerate(handle, start=1):
            try:
                value = json.loads(line)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"{path}:{line_no}: invalid JSON: {exc}") from exc
            if not isinstance(value, dict):
                raise SystemExit(f"{path}:{line_no}: expected object")
            records.append(value)
    return records


def message_template(record: dict[str, object]) -> str:
    sha = str(record["sha"])
    original = str(record.get("original_message", ""))
    subject = str(record.get("original_subject", ""))
    author_date = str(record.get("author_date", ""))
    index = record.get("index", "?")
    total = record.get("total", "?")
    stat = str(record.get("stat", "")).strip()
    files = record.get("files", [])
    if not isinstance(files, list):
        files = []
    file_lines = []
    for item in files[:80]:
        if not isinstance(item, dict):
            continue
        path = item.get("path", "")
        added = item.get("added", "?")
        deleted = item.get("deleted", "?")
        file_lines.append(f"- {path} (+{added}/-{deleted})")
    if len(files) > 80:
        file_lines.append(f"- ... {len(files) - 80} more files")

    return "\n".join(
        [
            f"# Commit Message: {sha}",
            "",
            "## Metadata",
            "",
            f"- index: {index} / {total}",
            f"- sha: `{sha}`",
            f"- author_date: `{author_date}`",
            f"- weak_original_message: `{str(record.get('weak_original_message', False)).lower()}`",
            f"- original_subject: `{subject}`",
            "",
            "## Original Message",
            "",
            "```text",
            original,
            "```",
            "",
            "## Generated Message",
            "",
            "```text",
            "",
            "```",
            "",
            "## Story Notes",
            "",
            "",
            "## Evidence",
            "",
            "### Files",
            "",
            "\n".join(file_lines) if file_lines else "- (no file changes recorded)",
            "",
            "### Stat",
            "",
            "```text",
            stat,
            "```",
            "",
        ]
    )


def write_message_templates(out: Path, records: list[dict[str, object]], *, overwrite: bool) -> int:
    messages_dir = out / "messages"
    messages_dir.mkdir(parents=True, exist_ok=True)
    written = 0
    for record in records:
        sha = str(record["sha"])
        path = messages_dir / f"{sha}.md"
        if path.exists() and not overwrite:
            continue
        path.write_text(message_template(record), encoding="utf-8")
        written += 1
    return written


def write_range_plan(
    out: Path,
    records: list[dict[str, object]],
    *,
    agent_count: int,
    context: int,
) -> list[dict[str, object]]:
    ranges_dir = out / "ranges"
    ranges_dir.mkdir(parents=True, exist_ok=True)
    ranges: list[dict[str, object]] = []
    total = len(records)
    if total == 0:
        return ranges
    agent_count = max(1, min(agent_count, total))
    chunk = (total + agent_count - 1) // agent_count
    for agent_index, start in enumerate(range(0, total, chunk), start=1):
        end = min(start + chunk, total)
        owned = records[start:end]
        context_start = max(0, start - context)
        context_end = min(total, end + context)
        context_records = records[context_start:context_end]
        range_record = {
            "agent": agent_index,
            "owned_start_index": owned[0]["index"],
            "owned_end_index": owned[-1]["index"],
            "owned_start_sha": owned[0]["sha"],
            "owned_end_sha": owned[-1]["sha"],
            "owned_count": len(owned),
            "context_start_index": context_records[0]["index"],
            "context_end_index": context_records[-1]["index"],
            "context_count": len(context_records),
            "write_glob": f"messages/{{{owned[0]['sha']}..{owned[-1]['sha']}}}.md",
        }
        ranges.append(range_record)
        write_jsonl(ranges_dir / f"agent_{agent_index:02d}_owned.jsonl", owned)
        write_jsonl(ranges_dir / f"agent_{agent_index:02d}_context.jsonl", context_records)

    lines = [
        "# Agent Range Plan",
        "",
        "Each agent owns only its `owned` JSONL range, but should read the adjacent",
        "`context` JSONL file before writing messages so nearby small commits tell a",
        "coherent story.",
        "",
        "## Rules",
        "",
        "- Write only files under `messages/<sha>.md` for owned commits.",
        "- Preserve `Original Message` exactly.",
        "- Fill `Generated Message` with a real commit message grounded in the diff.",
        "- Use `Story Notes` for continuity, uncertainty, and links to neighboring commits.",
        "- Keep the first line concise and imperative when possible.",
        "- Do not rewrite Git history from this catalog.",
        "",
        "## Assignments",
        "",
    ]
    for item in ranges:
        lines.append(
            "- Agent {agent}: owned {owned_start_index}-{owned_end_index} "
            "({owned_count} commits), context {context_start_index}-{context_end_index}".format(**item)
        )
    lines.append("")
    (out / "AGENT_RANGES.md").write_text("\n".join(lines), encoding="utf-8")
    return ranges


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


def scaffold(args: argparse.Namespace) -> int:
    cwd = Path(args.repo).resolve()
    out = Path(args.out).resolve()
    export_args = argparse.Namespace(
        repo=str(cwd),
        out=str(out),
        revs=args.revs,
        batch_size=args.batch_size,
        max_diff_bytes=args.max_diff_bytes,
        exclude=args.exclude,
        limit=args.limit,
        progress=args.progress,
    )
    export(export_args)
    records = read_jsonl(out / "commits.jsonl")
    written = write_message_templates(out, records, overwrite=args.overwrite)
    ranges = write_range_plan(out, records, agent_count=args.agents, context=args.context)
    print(f"wrote {written} message templates into {out / 'messages'}")
    print(f"wrote {len(ranges)} agent ranges into {out / 'ranges'}")
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
    missing_messages = [sha for sha in actual if not (out / "messages" / f"{sha}.md").exists()]
    if missing_messages:
        preview = ", ".join(missing_messages[:5])
        problems.append(f"missing message files for {len(missing_messages)} commits: {preview}")

    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        return 1

    print(f"valid: {len(actual)} commits, {batch_count} batches")
    print(f"weak original messages: {weak}")
    print(f"truncated diffs: {truncated}")
    if (out / "messages").exists():
        print(f"message files: {len(list((out / 'messages').glob('*.md')))}")
    return 0


GENERATED_BLOCK_RE = re.compile(
    r"## Generated Message\s*\n\s*```text\n(?P<body>.*?)\n```\s*\n",
    re.DOTALL,
)


def validate_messages(args: argparse.Namespace) -> int:
    out = Path(args.out).resolve()
    commits_path = out / "commits.jsonl"
    if not commits_path.exists():
        print(f"missing {commits_path}", file=sys.stderr)
        return 1

    records = read_jsonl(commits_path)
    missing: list[str] = []
    malformed: list[str] = []
    empty: list[str] = []
    filled = 0

    for record in records:
        sha = str(record["sha"])
        path = out / "messages" / f"{sha}.md"
        if not path.exists():
            missing.append(sha)
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        match = GENERATED_BLOCK_RE.search(text)
        if not match:
            malformed.append(sha)
            continue
        body = match.group("body").strip()
        if not body:
            empty.append(sha)
            continue
        filled += 1

    print(f"message files expected: {len(records)}")
    print(f"filled generated messages: {filled}")
    print(f"empty generated messages: {len(empty)}")
    print(f"missing files: {len(missing)}")
    print(f"malformed files: {len(malformed)}")

    if args.show and (empty or missing or malformed):
        if missing:
            print("missing preview: " + ", ".join(missing[: args.show]))
        if malformed:
            print("malformed preview: " + ", ".join(malformed[: args.show]))
        if empty:
            print("empty preview: " + ", ".join(empty[: args.show]))

    return 1 if missing or malformed or empty else 0


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

    scaffold_parser = subparsers.add_parser("scaffold", help="export commits and create per-sha message files")
    scaffold_parser.add_argument("--repo", default=".", help="repository path")
    scaffold_parser.add_argument("--out", default="tools/CommitMessageGenerate/work/catalog", help="output directory")
    scaffold_parser.add_argument("--revs", default="origin/main", help="revision range for git rev-list")
    scaffold_parser.add_argument("--batch-size", type=int, default=200)
    scaffold_parser.add_argument("--max-diff-bytes", type=int, default=120_000)
    scaffold_parser.add_argument("--exclude", action="append", default=[], help="pathspec to exclude")
    scaffold_parser.add_argument("--limit", type=int, default=0, help="limit commits for a smoke test")
    scaffold_parser.add_argument("--progress", type=int, default=50, help="stderr progress interval")
    scaffold_parser.add_argument("--agents", type=int, default=5, help="number of agent ranges")
    scaffold_parser.add_argument("--context", type=int, default=12, help="neighbor commits included as read-only context")
    scaffold_parser.add_argument("--overwrite", action="store_true", help="overwrite existing message templates")
    scaffold_parser.set_defaults(func=scaffold)

    validate_parser = subparsers.add_parser("validate", help="validate exported coverage")
    validate_parser.add_argument("--repo", default=".", help="repository path")
    validate_parser.add_argument("--out", default=".gitstory", help="output directory")
    validate_parser.add_argument("--revs", default="origin/main", help="revision range for git rev-list")
    validate_parser.add_argument("--limit", type=int, default=0, help="match an exported smoke-test limit")
    validate_parser.set_defaults(func=validate)

    validate_messages_parser = subparsers.add_parser(
        "validate-messages",
        help="validate that per-sha message files have generated messages",
    )
    validate_messages_parser.add_argument(
        "--out",
        default="tools/CommitMessageGenerate/work/catalog_origin_main",
        help="catalog output directory",
    )
    validate_messages_parser.add_argument("--show", type=int, default=10, help="preview count for problems")
    validate_messages_parser.set_defaults(func=validate_messages)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
