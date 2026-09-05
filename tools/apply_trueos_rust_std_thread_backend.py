#!/usr/bin/env python3
"""Install the reference TRUEOS `std::thread` sys backend into rust-src.

This is intentionally a narrow source adaptation. TRUEOS remains a concurrent
Rust target (`target_has_threads` must not be falsified), but Rust std selects a
TRUEOS-specific thread backend before the generic Unix/pthread backend.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import tempfile

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


def replace_once(source: str, before: str, after: str, path: Path) -> str:
    if source.count(after) == 1:
        return source
    if source.count(before) != 1:
        raise SystemExit(f"{path}: expected exactly one source anchor: {before!r}")
    return source.replace(before, after, 1)


def installation(root: Path) -> dict[Path, str]:
    """Preflight every input before changing any file in the selected sysroot."""
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
    selector = selector_path.read_text(encoding="utf-8")
    if 'target_os = "trueos" => {' in selector and TRUEOS_SELECTOR not in selector:
        raise SystemExit(f"{selector_path}: conflicting TRUEOS thread selector")
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
    if selector.count(TRUEOS_SELECTOR) != 1 or selector.index(TRUEOS_SELECTOR) > selector.index(UNIX_SELECTOR):
        raise SystemExit(f"{selector_path}: TRUEOS must be selected exactly once before Unix")

    unix_path = root / "library/std/src/os/unix/mod.rs"
    unix = unix_path.read_text(encoding="utf-8")
    unix = replace_once(unix, "pub mod thread;", '#[cfg(not(target_os = "trueos"))]\npub mod thread;', unix_path)
    unix = replace_once(unix, "    pub use super::thread::JoinHandleExt;", '    #[cfg(not(target_os = "trueos"))]\n    pub use super::thread::JoinHandleExt;', unix_path)
    return {backend_path: reference, selector_path: selector, unix_path: unix}


def install(root: Path, check: bool = False) -> None:
    planned = installation(root)
    changed = [path for path, source in planned.items() if not path.exists() or path.read_text(encoding="utf-8") != source]
    if check and changed:
        raise SystemExit("TRUEOS std backend not installed: " + ", ".join(map(str, changed)))
    # Stage all writes first, then replace each file atomically. A repeated run
    # safely finishes installation if the process was interrupted between files.
    staged = []
    try:
        for path in changed:
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent, delete=False) as temporary:
                staged.append((Path(temporary.name), path))
                temporary.write(planned[path])
            staged[-1][0].chmod(path.stat().st_mode & 0o777 if path.exists() else 0o644)
        for temporary, path in staged:
            os.replace(temporary, path)
    finally:
        for temporary, _ in staged:
            temporary.unlink(missing_ok=True)

    print(f"trueos-rust-std-thread: backend={root / 'library/std/src/sys/thread/trueos.rs'}")
    print("trueos-rust-std-thread: lifecycle=unsupported sleep=true yield=true")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="Verify installed sources without writing")
    parser.add_argument(
        "rust_src",
        type=Path,
        help="Rust source root (the directory containing library/std)",
    )
    args = parser.parse_args()
    install(rust_root(args.rust_src), check=args.check)


if __name__ == "__main__":
    main()
