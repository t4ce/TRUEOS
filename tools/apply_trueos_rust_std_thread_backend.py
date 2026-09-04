#!/usr/bin/env python3
"""Install the reference TRUEOS `std::thread` sys backend into rust-src.

This is intentionally a narrow source adaptation. TRUEOS remains a concurrent
Rust target (`target_has_threads` must not be falsified), but Rust std selects a
TRUEOS-specific thread backend before the generic Unix/pthread backend.
"""

from __future__ import annotations

import argparse
from pathlib import Path

UNIX_SELECTOR = '    any(target_family = "unix", target_os = "wasi") => {\n'
TRUEOS_SELECTOR = '''    target_os = "trueos" => {
        mod trueos;
        pub use trueos::{
            DEFAULT_MIN_STACK_SIZE, Thread, available_parallelism, current_os_id, set_name, sleep,
            yield_now,
        };
    }
'''


def rust_root(path: Path) -> Path:
    path = path.resolve()
    if (path / "library/std/src/sys/thread/mod.rs").is_file():
        return path
    if path.name == "library" and (path / "std/src/sys/thread/mod.rs").is_file():
        return path.parent
    raise SystemExit(
        f"{path}: expected a Rust source root containing library/std/src/sys/thread/mod.rs"
    )


def install(root: Path) -> None:
    thread_dir = root / "library/std/src/sys/thread"
    selector_path = thread_dir / "mod.rs"
    backend_path = thread_dir / "trueos.rs"
    reference_path = Path(__file__).resolve().parent / "rust-std/trueos_thread.rs"

    reference = reference_path.read_text(encoding="utf-8")
    if backend_path.exists():
        existing = backend_path.read_text(encoding="utf-8")
        if existing != reference:
            raise SystemExit(
                f"{backend_path}: existing TRUEOS thread backend differs from the canonical reference"
            )
    else:
        backend_path.write_text(reference, encoding="utf-8")

    selector = selector_path.read_text(encoding="utf-8")
    if TRUEOS_SELECTOR not in selector:
        if selector.count(UNIX_SELECTOR) != 1:
            raise SystemExit(
                f"{selector_path}: expected exactly one Unix thread selector anchor"
            )
        selector = selector.replace(
            UNIX_SELECTOR,
            TRUEOS_SELECTOR + UNIX_SELECTOR,
            1,
        )
        selector_path.write_text(selector, encoding="utf-8")

    print(f"trueos-rust-std-thread: backend={backend_path}")
    print("trueos-rust-std-thread: lifecycle=unsupported sleep=true yield=true")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "rust_src",
        type=Path,
        help="Rust source root (the directory containing library/std)",
    )
    args = parser.parse_args()
    install(rust_root(args.rust_src))


if __name__ == "__main__":
    main()
