#!/usr/bin/env python3
from __future__ import annotations

import sys


def fnv1a64(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def main(argv: list[str]) -> int:
    if len(argv) != 2 or argv[1] in {"-h", "--help"}:
        print("usage: qjs_cdn_hash.py <url>")
        print("prints: /qjs/cdn/<hash>.mjs and the repo vendoring path")
        return 2

    url = argv[1]
    h = fnv1a64(url.encode("utf-8"))
    hex16 = f"{h:016x}"
    qjs_path = f"/qjs/cdn/{hex16}.mjs"
    repo_path = f"crates/trueos-qjs/app/cdn/{hex16}.mjs"
    print(qjs_path)
    print(repo_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
