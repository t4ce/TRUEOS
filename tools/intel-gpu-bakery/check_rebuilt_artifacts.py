#!/usr/bin/env python3
"""Compare rebuilt artifacts, allowing only relocatable contract provenance comments."""
import argparse
from pathlib import Path
import subprocess


def comparable(path, data):
    if path.endswith(".contract.rs"):
        # Full provenance remains in the manifest and is checked by verify.py.
        # These comments also record historical source names and checkout paths.
        return b"".join(
            line for line in data.splitlines(keepends=True)
            if not line.startswith((b"// source: ", b"// normalized commands: "))
        )
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory")
    parser.add_argument("--restore-comments", action="store_true",
                        help="restore checked-in contract comments after all comparisons pass")
    args = parser.parse_args()
    root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
    changed = subprocess.check_output(
        ["git", "diff", "HEAD", "--name-only", "-z", "--", args.directory], cwd=root
    ).decode().split("\0")
    failures = []
    restorations = []
    for path in filter(None, changed):
        original = subprocess.run(["git", "show", f"HEAD:{path}"], cwd=root, capture_output=True)
        current = root / path
        if original.returncode or not current.is_file() or comparable(path, original.stdout) != comparable(path, current.read_bytes()):
            failures.append(path)
        elif path.endswith(".contract.rs"):
            restorations.append((current, original.stdout))
    if failures:
        for path in failures:
            print(f"rebuilt artifact differs: {path}")
        return 1
    if args.restore_comments:
        # Only write after every artifact passes. No executable or ABI changes
        # are discarded; the allowed differences are the two comment fields.
        for current, original in restorations:
            current.write_bytes(original)
        print(f"Restored checked-in provenance comments in {len(restorations)} contract(s).")
    print("Rebuilt artifacts match (excluding contract source/command comments).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
