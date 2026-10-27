#!/usr/bin/env python3
"""Install the reference TRUEOS `std::thread` sys backend into rust-src.

TRUEOS remains a concurrent Rust target (`target_has_threads` must not be
falsified). Rust std selects its stackful lifecycle backend and process/thread
keyed TLS instead of the earlier unsupported lifecycle and WLS-slot TLS.
"""

from __future__ import annotations

import argparse
import hashlib
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
TRUEOS_TLS_SELECTOR = '''    target_os = "trueos" => {
        mod os;
        pub use os::{Storage, thread_local_inner, value_align};
        pub(crate) use os::{LocalPointer, local_pointer};
    }
'''

# Exact canonical revisions previously installed by this integration. When the
# reference changes, retain the reviewed previous digest here so packing can
# upgrade it without treating an arbitrary locally edited backend as ours.
KNOWN_BACKEND_SHA256 = frozenset({
    # Initial PR #37 integration, before extracting/testing sleep_with.
    "ba0041a1e49ecbc166c6ba02abd769eb1f47632a7acd5112c5ee2fdbf6bdcdba",
    # Reviewed unsupported lifecycle backend with duration rounding tests.
    "e6b682b491fbd3dd44928525f79500d1fbe3a67c954caa52dca5f3d868581a15",
})


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
        if before in source.replace(after, "", 1):
            raise SystemExit(f"{path}: duplicate source anchor: {before!r}")
        return source
    if source.count(before) != 1:
        raise SystemExit(f"{path}: expected exactly one source anchor: {before!r}")
    return source.replace(before, after, 1)


def keyed_tls_selector(source: str, path: Path) -> str:
    """Select OS-keyed TLS, also upgrading the legacy no_threads selector."""
    cfg_anchor = "cfg_select! {\n"
    if source.count(TRUEOS_TLS_SELECTOR) > 1:
        raise SystemExit(f"{path}: duplicate TRUEOS TLS selector")
    if 'target_os = "trueos" => {' in source and TRUEOS_TLS_SELECTOR not in source:
        raise SystemExit(f"{path}: conflicting TRUEOS TLS selector")
    if source.count(cfg_anchor) < 1:
        raise SystemExit(f"{path}: missing TLS selector anchor")
    selector_start = source.index(cfg_anchor) + len(cfg_anchor)
    try:
        selector_end = source.index("    target_thread_local => {", selector_start)
    except ValueError:
        raise SystemExit(f"{path}: missing native TLS selector anchor") from None
    prefix = source[selector_start:selector_end]
    if "mod no_threads;" not in prefix or "pub use no_threads::" not in prefix:
        raise SystemExit(f"{path}: unrecognized no_threads TLS selector")
    old_entry = '        target_os = "trueos",\n'
    if prefix.count(old_entry) > 1:
        raise SystemExit(f"{path}: duplicate legacy TRUEOS TLS selector")
    prefix = prefix.replace(old_entry, "")
    if TRUEOS_TLS_SELECTOR not in prefix:
        prefix = TRUEOS_TLS_SELECTOR + prefix
    elif not prefix.startswith(TRUEOS_TLS_SELECTOR):
        raise SystemExit(f"{path}: TRUEOS TLS must be selected before no_threads")
    return source[:selector_start] + prefix + source[selector_end:]


def strict_thread_initialization(source: str, path: Path) -> str:
    """Remove the legacy TRUEOS permission to overwrite a carrier's TLS handle."""
    old = '#[cfg(any(target_os = "trueos", target_os = "zkvm"))]'
    inverse = '#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]'
    if old not in source and inverse not in source:
        return source
    if source.count(old) != 1 or source.count(inverse) != 1:
        raise SystemExit(f"{path}: unrecognized legacy TRUEOS current-thread hooks")
    source = source.replace(old, '#[cfg(target_os = "zkvm")]', 1)
    source = source.replace(inverse, '#[cfg(not(target_os = "zkvm"))]', 1)
    return source.replace("// TRUEOS carrier lanes may host", "// Legacy zkvm carrier lanes may host", 1)


def installation(root: Path) -> dict[Path, str]:
    """Preflight every input before changing any file in the selected sysroot."""
    thread_dir = root / "library/std/src/sys/thread"
    selector_path = thread_dir / "mod.rs"
    backend_path = thread_dir / "trueos.rs"
    reference_path = Path(__file__).resolve().parent.parent / "rust-std/trueos_thread.rs"

    reference = reference_path.read_text(encoding="utf-8")
    if backend_path.exists():
        existing = backend_path.read_text(encoding="utf-8")
        existing_digest = hashlib.sha256(backend_path.read_bytes()).hexdigest()
        if existing != reference and existing_digest not in KNOWN_BACKEND_SHA256:
            raise SystemExit(
                f"{backend_path}: unrecognized TRUEOS thread backend "
                f"(sha256={existing_digest}); refusing to overwrite local changes"
            )
    selector = selector_path.read_text(encoding="utf-8")
    if selector.count(UNIX_SELECTOR) != 1:
        raise SystemExit(f"{selector_path}: expected exactly one Unix thread selector anchor")
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
    tls_path = root / "library/std/src/sys/thread_local/mod.rs"
    tls = keyed_tls_selector(tls_path.read_text(encoding="utf-8"), tls_path)
    current_path = root / "library/std/src/thread/current.rs"
    current = strict_thread_initialization(current_path.read_text(encoding="utf-8"), current_path)
    return {backend_path: reference, selector_path: selector, unix_path: unix,
            tls_path: tls, current_path: current}


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
    print("trueos-rust-std-thread: lifecycle=stackful sleep=true yield=true tls=keyed")


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
