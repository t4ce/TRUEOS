#!/usr/bin/env python3
"""Build provenance records for TRUEOS ELF/ISO artifacts.

The record is a small hash chain:

    previous_record_sha256 -> source tree hash -> ELF/ISO hashes -> record hash

It is intentionally plain JSON plus SHA-256 so it can be inspected with boring
tools and later signed with GPG, minisign, signify, or a transparency log.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
from pathlib import Path
from typing import Any


SCHEMA = "trueos.provenance-chain.v1"
EXCLUDED_DIRS = {
    ".git",
    ".limine",
    ".codex_tmp",
    "__pycache__",
    "bld",
    "node_modules",
    "target",
    "tgt",
}
EXCLUDED_SUFFIXES = {
    ".pyc",
}
EXCLUDED_PATHS = {
    "tools/nvme.img",
}
TOOL_COMMANDS = [
    ("git", ["git", "--version"]),
    ("make", ["make", "--version"]),
    ("cargo", ["cargo", "-Vv"]),
    ("rustc", ["rustc", "-Vv"]),
    ("rustup", ["rustup", "show", "active-toolchain"]),
    ("rust-lld", ["rust-lld", "--version"]),
    ("ar", ["ar", "--version"]),
    ("strip", ["strip", "--version"]),
    ("xorriso", ["xorriso", "-version"]),
    ("mkfs.vfat", ["mkfs.vfat", "--version"]),
    ("mcopy", ["mcopy", "-V"]),
    ("zstd", ["zstd", "--version"]),
]
RELEVANT_ENV = [
    "AR",
    "CC",
    "CFLAGS",
    "CARGO_BUILD_FLAGS",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "CARGO_UNSTABLE_JSON_TARGET_SPEC",
    "LD",
    "LD_LIBRARY_PATH",
    "PATH",
    "RUSTC",
    "RUSTDOC",
    "RUSTFLAGS",
    "RUSTUP_HOME",
    "SMOLTCP_IFACE_MAX_ADDR_COUNT",
]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json(data: dict[str, Any]) -> bytes:
    text = json.dumps(data, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return (text + "\n").encode("utf-8")


def hash_file(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            size += len(chunk)
            digest.update(chunk)
    return digest.hexdigest(), size


def run_text(argv: list[str], cwd: Path) -> tuple[int, str]:
    try:
        completed = subprocess.run(
            argv,
            cwd=cwd,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
    except FileNotFoundError as err:
        return 127, str(err)
    return completed.returncode, completed.stdout.strip()


def git_text(root: Path, argv: list[str]) -> str | None:
    code, output = run_text(["git", *argv], root)
    if code != 0:
        return None
    return output


def should_skip(path: Path, rel: str) -> bool:
    if rel in EXCLUDED_PATHS:
        return True
    if path.suffix in EXCLUDED_SUFFIXES:
        return True
    if path.name.endswith((".iso", ".7z")):
        return True
    return False


def source_entries(root: Path) -> tuple[list[str], int, int]:
    rows: list[str] = []
    file_count = 0
    bytes_hashed = 0

    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(name for name in dirnames if name not in EXCLUDED_DIRS)
        base = Path(dirpath)
        for filename in sorted(filenames):
            path = base / filename
            rel = path.relative_to(root).as_posix()
            if should_skip(path, rel):
                continue

            st = path.lstat()
            mode = stat.S_IMODE(st.st_mode)
            if stat.S_ISLNK(st.st_mode):
                kind = "symlink"
                data = os.readlink(path).encode("utf-8", "surrogateescape")
                digest = sha256_bytes(data)
                size = len(data)
            elif stat.S_ISREG(st.st_mode):
                kind = "file"
                digest, size = hash_file(path)
            else:
                continue

            rows.append(f"{digest}  {kind} {mode:04o} {size} {rel}\n")
            file_count += 1
            bytes_hashed += size

    return rows, file_count, bytes_hashed


def write_source_manifest(root: Path, out_path: Path) -> dict[str, Any]:
    rows, file_count, bytes_hashed = source_entries(root)
    content = "".join(rows).encode("utf-8")
    out_path.write_bytes(content)
    return {
        "path": out_path.name,
        "sha256": sha256_bytes(content),
        "file_count": file_count,
        "bytes_hashed": bytes_hashed,
    }


def artifact_record(root: Path, path: Path) -> dict[str, Any]:
    digest, size = hash_file(path)
    return {
        "path": path.relative_to(root).as_posix() if path.is_relative_to(root) else str(path),
        "sha256": digest,
        "size": size,
    }


def parse_build_info(path: Path | None) -> dict[str, str]:
    if path is None or not path.exists():
        return {}
    values: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        key, sep, value = line.partition("=")
        if sep:
            values[key] = value
    return values


def collect_tools(root: Path) -> dict[str, Any]:
    tools: dict[str, Any] = {}
    rust_lld = shutil.which("rust-lld")
    if rust_lld is None:
        sysroot = gitless_command(["rustc", "--print", "sysroot"], root)
        if sysroot:
            candidate = Path(sysroot) / "lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld"
            if candidate.exists():
                rust_lld = str(candidate)

    for name, argv in TOOL_COMMANDS:
        real_argv = argv
        path = shutil.which(argv[0])
        if name == "rust-lld" and rust_lld is not None:
            path = rust_lld
            real_argv = [rust_lld, "--version"]
        code, output = run_text(real_argv, root)
        tools[name] = {
            "path": path,
            "exit_code": code,
            "version": output.splitlines()[:8],
        }
    return tools


def gitless_command(argv: list[str], root: Path) -> str | None:
    code, output = run_text(argv, root)
    if code != 0:
        return None
    return output


def collect_git(root: Path) -> dict[str, Any]:
    status = git_text(root, ["status", "--short"]) or ""
    submodules = git_text(root, ["submodule", "status", "--recursive"]) or ""
    return {
        "commit": git_text(root, ["rev-parse", "HEAD"]),
        "commit_short": git_text(root, ["rev-parse", "--short=12", "HEAD"]),
        "dirty": bool(status.strip()),
        "status_sha256": sha256_bytes(status.encode("utf-8")),
        "status_short": status.splitlines(),
        "submodules_sha256": sha256_bytes(submodules.encode("utf-8")),
        "submodules": submodules.splitlines(),
    }


def collect_env() -> dict[str, str]:
    return {name: os.environ[name] for name in RELEVANT_ENV if name in os.environ}


def previous_hash(out_dir: Path) -> str | None:
    latest = out_dir / "latest.json"
    if not latest.exists():
        return None
    try:
        data = json.loads(latest.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return sha256_bytes(latest.read_bytes())
    value = data.get("record_sha256")
    return value if isinstance(value, str) else sha256_bytes(latest.read_bytes())


def resolve_record_path(root: Path, record_path: str) -> Path:
    path = Path(record_path)
    return path if path.is_absolute() else root / path


def build_record(args: argparse.Namespace) -> int:
    root = Path(args.source_root).resolve()
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    stamp = dt.datetime.now(dt.UTC).strftime("%Y%m%dT%H%M%SZ") + f"-{os.getpid()}"
    source_manifest_path = out_dir / f"source-files-{stamp}.sha256"
    source_manifest = write_source_manifest(root, source_manifest_path)

    artifacts: dict[str, Any] = {}
    for name, value in [
        ("runtime_elf", args.elf),
        ("debug_elf", args.debug_elf),
        ("iso", args.iso),
    ]:
        if value:
            path = Path(value).resolve()
            if not path.exists():
                print(f"error: missing artifact for {name}: {path}", file=sys.stderr)
                return 2
            artifacts[name] = artifact_record(root, path)

    record: dict[str, Any] = {
        "schema": SCHEMA,
        "created_utc": dt.datetime.now(dt.UTC).isoformat(timespec="seconds"),
        "source_root": str(root),
        "source_manifest": source_manifest,
        "git": collect_git(root),
        "build": {
            "info_path": str(Path(args.build_info).resolve()) if args.build_info else None,
            "info": parse_build_info(Path(args.build_info).resolve() if args.build_info else None),
        },
        "artifacts": artifacts,
        "tools": collect_tools(root),
        "environment": collect_env(),
        "previous_record_sha256": previous_hash(out_dir),
    }

    record_hash = sha256_bytes(canonical_json(record))
    record["record_sha256"] = record_hash
    record_path = out_dir / f"record-{stamp}-{record_hash[:16]}.json"
    record_path.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    shutil.copy2(record_path, out_dir / "latest.json")
    shutil.copy2(source_manifest_path, out_dir / "latest.source-files.sha256")

    print(f"provenance_record={record_path}")
    print(f"record_sha256={record_hash}")
    print(f"source_manifest_sha256={source_manifest['sha256']}")
    for name, artifact in artifacts.items():
        print(f"{name}_sha256={artifact['sha256']}")
    return 0


def verify_record(args: argparse.Namespace) -> int:
    root = Path(args.source_root).resolve()
    record_path = Path(args.record).resolve()
    record = json.loads(record_path.read_text(encoding="utf-8"))
    expected_hash = record.get("record_sha256")
    payload = dict(record)
    payload.pop("record_sha256", None)
    actual_hash = sha256_bytes(canonical_json(payload))
    ok = True

    if expected_hash != actual_hash:
        print(f"record hash mismatch: expected {expected_hash}, got {actual_hash}", file=sys.stderr)
        ok = False

    manifest_info = record.get("source_manifest", {})
    manifest_name = manifest_info.get("path")
    manifest_path = record_path.parent / manifest_name if isinstance(manifest_name, str) else None
    if manifest_path is None or not manifest_path.exists():
        print("source manifest missing next to record", file=sys.stderr)
        ok = False
    else:
        manifest_hash = sha256_bytes(manifest_path.read_bytes())
        if manifest_hash != manifest_info.get("sha256"):
            print("stored source manifest hash mismatch", file=sys.stderr)
            ok = False

    check_manifest = record_path.parent / ".verify-source-files.sha256"
    recomputed = write_source_manifest(root, check_manifest)
    try:
        check_manifest.unlink()
    except FileNotFoundError:
        pass
    if recomputed.get("sha256") != manifest_info.get("sha256"):
        print(
            f"source tree hash mismatch: expected {manifest_info.get('sha256')}, "
            f"got {recomputed.get('sha256')}",
            file=sys.stderr,
        )
        ok = False

    for name, artifact in record.get("artifacts", {}).items():
        artifact_path = resolve_record_path(root, artifact["path"])
        if not artifact_path.exists():
            print(f"artifact missing: {name} {artifact_path}", file=sys.stderr)
            ok = False
            continue
        digest, size = hash_file(artifact_path)
        if digest != artifact.get("sha256") or size != artifact.get("size"):
            print(f"artifact hash mismatch: {name}", file=sys.stderr)
            ok = False

    if ok:
        print(f"verified_record_sha256={expected_hash}")
        return 0
    return 1


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    attest = sub.add_parser("attest", help="write a new provenance chain record")
    attest.add_argument("--source-root", default=".")
    attest.add_argument("--out-dir", default="bld/provenance")
    attest.add_argument("--elf", required=True)
    attest.add_argument("--debug-elf")
    attest.add_argument("--iso", required=True)
    attest.add_argument("--build-info")
    attest.set_defaults(func=build_record)

    verify = sub.add_parser("verify", help="verify a provenance record against local files")
    verify.add_argument("--source-root", default=".")
    verify.add_argument("--record", default="bld/provenance/latest.json")
    verify.set_defaults(func=verify_record)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
