#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable
from urllib.parse import urljoin
from urllib.request import Request, urlopen


def fnv1a64(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


URL_RE = re.compile(
    r"""(?x)
    (?:\bimport\s*(?:\(|\s+)|\bexport\s+\*\s+from\s+)
    [^\n]*?
    (?:(?:from\s+)?)
    ["']
    (?P<spec>https?://[^"']+|\./[^"']+|\.\./[^"']+|/[^"']+)
    ["']
    """
)


def iter_seed_urls(seedlist_path: Path) -> list[str]:
    urls: list[str] = []
    for raw in seedlist_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        urls.append(line)
    return urls


def cache_paths_for_url(url: str) -> tuple[str, Path]:
    h = fnv1a64(url.encode("utf-8"))
    hex16 = f"{h:016x}"
    qjs_path = f"/qjs/cdn/{hex16}.mjs"
    repo_path = Path("crates/trueos-qjs/app/cdn") / f"{hex16}.mjs"
    return qjs_path, repo_path


def fetch_url(url: str, timeout: float) -> bytes:
    req = Request(url, headers={"User-Agent": "TRUEOS-qjs-cdn-seed/1"})
    with urlopen(req, timeout=timeout) as r:
        return r.read()


def discover_url_imports(base_url: str, src: bytes) -> list[str]:
    try:
        text = src.decode("utf-8", errors="ignore")
    except Exception:
        return []

    out: list[str] = []
    for m in URL_RE.finditer(text):
        spec = m.group("spec")
        if not spec:
            continue

        if spec.startswith("http://") or spec.startswith("https://"):
            out.append(spec)
        else:
            # Resolve relative URL specs the same way the runtime loader will.
            out.append(urljoin(base_url, spec))
    return out


@dataclass(frozen=True)
class Result:
    url: str
    qjs_cache_path: str
    repo_path: Path
    action: str


def seed_one(
    url: str,
    *,
    timeout: float,
    force: bool,
    verify_only: bool,
) -> tuple[Result, bytes]:
    qjs_cache_path, repo_path = cache_paths_for_url(url)
    repo_path.parent.mkdir(parents=True, exist_ok=True)

    if repo_path.exists():
        existing = repo_path.read_bytes()
        if verify_only:
            # Verify-only still needs to know whether content matches.
            fresh = fetch_url(url, timeout=timeout)
            if existing != fresh:
                return Result(url, qjs_cache_path, repo_path, "mismatch"), fresh
            return Result(url, qjs_cache_path, repo_path, "ok"), fresh

        # Not verify-only.
        fresh = fetch_url(url, timeout=timeout)
        if existing == fresh:
            return Result(url, qjs_cache_path, repo_path, "ok"), fresh
        if not force:
            return Result(url, qjs_cache_path, repo_path, "mismatch"), fresh
        repo_path.write_bytes(fresh)
        return Result(url, qjs_cache_path, repo_path, "updated"), fresh

    if verify_only:
        # Missing file is treated as mismatch in verify-only mode.
        fresh = fetch_url(url, timeout=timeout)
        return Result(url, qjs_cache_path, repo_path, "missing"), fresh

    fresh = fetch_url(url, timeout=timeout)
    repo_path.write_bytes(fresh)
    return Result(url, qjs_cache_path, repo_path, "written"), fresh


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(
        description="Seed TRUEOS QuickJS URL cache files into crates/trueos-qjs/app/cdn/",
    )
    ap.add_argument(
        "seedlist",
        nargs="?",
        default="crates/trueos-qjs/app/cdn/seedlist.txt",
        help="Path to seedlist.txt (default: crates/trueos-qjs/app/cdn/seedlist.txt)",
    )
    ap.add_argument(
        "--recursive",
        action="store_true",
        help="Recursively fetch URL imports discovered in fetched modules",
    )
    ap.add_argument(
        "--timeout",
        type=float,
        default=20.0,
        help="Per-request timeout in seconds (default: 20)",
    )
    ap.add_argument(
        "--force",
        action="store_true",
        help="Overwrite existing cached files if content differs",
    )
    ap.add_argument(
        "--verify-only",
        action="store_true",
        help="Do not write; just verify current files match the URLs",
    )

    ns = ap.parse_args(argv[1:])
    seedlist_path = Path(ns.seedlist)
    if not seedlist_path.is_file():
        print(f"seedlist not found: {seedlist_path}", file=sys.stderr)
        return 2

    queue: list[str] = iter_seed_urls(seedlist_path)
    seen: set[str] = set()

    mismatches = 0
    written = 0
    updated = 0

    while queue:
        url = queue.pop(0)
        if url in seen:
            continue
        seen.add(url)

        res, fresh = seed_one(
            url,
            timeout=ns.timeout,
            force=bool(ns.force),
            verify_only=bool(ns.verify_only),
        )

        if res.action in {"written"}:
            written += 1
        elif res.action in {"updated"}:
            updated += 1
        elif res.action in {"mismatch", "missing"}:
            mismatches += 1

        print(f"{res.action}: {res.url}")
        print(f"  -> {res.qjs_cache_path}")
        print(f"  -> {res.repo_path}")

        if ns.recursive:
            for dep in discover_url_imports(url, fresh):
                if dep not in seen:
                    queue.append(dep)

    if mismatches:
        if ns.force:
            # With --force, mismatches were overwritten.
            pass
        else:
            print(
                f"ERROR: {mismatches} cached file(s) differ from the pinned URLs. "
                f"Re-run with --force to overwrite.",
                file=sys.stderr,
            )
            return 3

    if ns.verify_only and (written or updated):
        # Should never happen, but keep semantics clear.
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
