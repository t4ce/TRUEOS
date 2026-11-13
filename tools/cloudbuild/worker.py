"""Run approved build manifests on a dedicated, pre-provisioned cloud worker.

This is an operator process, not a public arbitrary-code execution endpoint.
Use a disposable builder account/container with no signing keys or deploy
credentials. Source and sibling checkouts must be clean and already pinned.
"""
from __future__ import annotations

import fcntl
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

from build_plan import validate_manifest
from buildins import preparation_record
from identity import canonical_json
from service import APIError, Store, digest_file

TOOLCHAIN_COMMIT = "af3d95584dbddcae597890340995509a7fb47a50"


def git(repository: Path, *arguments: str) -> str:
    return subprocess.run(["git", "-C", str(repository), *arguments], check=True,
                          capture_output=True, text=True, timeout=30).stdout.strip()


def clean_revision(repository: Path) -> str:
    revision = git(repository, "rev-parse", "HEAD")
    if git(repository, "status", "--porcelain", "--untracked-files=normal"):
        raise ValueError(f"Worker checkout {repository.name} must be clean before building")
    return revision


def preflight(repository: Path, manifest: dict) -> dict:
    source = clean_revision(repository)
    validate_manifest(manifest, source)
    modules = git(repository, "submodule", "status", "--recursive")
    if any(line.startswith(("-", "+", "U")) for line in modules.splitlines()):
        raise ValueError("Initialize every submodule at its committed revision before starting the worker")
    if git(repository, "config", "--default", "false", "--get", "core.sparseCheckout") == "true":
        raise ValueError("Cloud builds need a complete checkout, not a sparse checkout")
    buildins_enabled = bool(manifest["buildins"])
    siblings = {}
    sibling_names = ["TRUEOS-Blueprints"]
    if buildins_enabled:
        sibling_names.insert(0, "TRUEOS-Buildins")
    for name in sibling_names:
        path = repository.parent / name
        if not path.is_dir():
            raise ValueError(f"Provision the pinned sibling checkout {name} before starting the worker")
        siblings[name] = clean_revision(path)
    for executable in ("cargo", "rustc", "python3", "patch", "nm", "readelf"):
        if not shutil.which(executable):
            raise ValueError(f"Missing worker dependency: {executable}")
    toolchain = subprocess.run(["rustc", "-vV"], cwd=repository, check=True,
                              capture_output=True, text=True, timeout=30).stdout.strip()
    if f"commit-hash: {TOOLCHAIN_COMMIT}" not in toolchain.splitlines():
        raise ValueError("Worker must use the archived TRUEOS Rust toolchain from the release workflow")
    return {"source_commit": source, "sibling_commits": siblings, "submodules": modules,
            "rustc": toolchain, "config_hash": manifest["config_hash"],
            "buildins_enabled": buildins_enabled,
            "buildins": manifest["buildins"],
            "reproducibility": "Input provenance only; bit-identical rebuilds have not been established."}


def run_build(repository: Path, manifest: dict, spec_path: Path, log_path: Path,
              timeout: int = 7200) -> None:
    # Do not forward the web process's environment, bearer tokens, grants or
    # issuer path into Cargo/build scripts. Use a credential-free worker account.
    allowed = {"PATH", "HOME", "CARGO_HOME", "RUSTUP_HOME", "LD_LIBRARY_PATH", "TMPDIR", "LANG", "LC_ALL"}
    environment = {name: value for name, value in os.environ.items() if name in allowed}
    environment.update({"TRUEOS_CLOUDBUILD_SPEC": str(spec_path),
                        "CARGO_BUILD_JOBS": "2", "RAYON_NUM_THREADS": "2"})
    with log_path.open("wb") as log:
        process = subprocess.Popen(manifest["command"], cwd=repository, env=environment,
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=timeout)
        except BaseException:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            raise
    if code:
        raise ValueError(f"Build exited with status {code}; the operator can inspect the private job log")


def process_one(store: Store, repository: Path, *, timeout=7200,
                preflight_fn=preflight, build_fn=run_build) -> bool:
    """Claim and execute one job. Injected functions support isolated worker tests."""
    repository = repository.resolve()
    staging = repository / "tgt"
    staging.mkdir(parents=True, exist_ok=True)
    # prepare.sh publishes fixed paths within a checkout; hold a per-checkout
    # lock across the entire build and copy, including multiple queue workers.
    with (staging / "cloudbuild-worker.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return False
        job = store.claim()
        if job is None:
            return False
        work = store.directory / "jobs" / job["id"]
        work.mkdir(parents=True, mode=0o700)
        bundle = staging / ("rigreal.tar.lz4" if json.loads(job["manifest"])["profile"] == "release" else "rigdev.tar.lz4")
        backup = None
        published = False
        try:
            manifest = validate_manifest(json.loads(job["manifest"]))
            if job["fingerprint"]:
                with store.connect() as db:
                    store.authorize(db, job["fingerprint"], manifest, int(store.clock()), job["license_id"])
            provenance = preflight_fn(repository, manifest)
            spec = work / "manifest.json"
            spec.write_bytes(canonical_json(manifest))
            # A successful exit without fresh output must never license an old
            # image under this job's feature manifest. Preserve the old pointer
            # on the same filesystem and restore it if this attempt fails.
            if bundle.exists() or bundle.is_symlink():
                backup = staging / (".cloudbuild-previous-" + job["id"])
                bundle.replace(backup)
            build_fn(repository, manifest, spec, work / "build.log", timeout)
            # Source changes during a build invalidate the provenance claim.
            for path, expected in [(repository, provenance["source_commit"])] + [
                (repository.parent / name, sha) for name, sha in provenance.get("sibling_commits", {}).items()
            ]:
                if git(path, "rev-parse", "HEAD") != expected or git(path, "status", "--porcelain", "--untracked-files=no"):
                    raise ValueError("A source checkout changed during the build")
            provenance["buildins_preparation"] = preparation_record(
                staging / "buildins" / manifest["profile"], manifest["profile"], manifest["buildins"]
            )
            actual = bundle.resolve(strict=True)
            if not actual.is_relative_to(staging.resolve()) or not actual.is_file():
                raise ValueError("Build output is outside the worker staging directory")
            output = store.artifact_path(job["id"])
            output.parent.mkdir(parents=True, mode=0o700)
            temporary = output.with_suffix(".partial")
            shutil.copyfile(actual, temporary)
            temporary.chmod(0o600)
            temporary.replace(output)
            for name, path in [("kernel_lock_sha256", repository / "Cargo.lock"),
                               ("loader_lock_sha256", repository / "tools/trueos-boot/Cargo.lock")]:
                provenance[name] = digest_file(path) if path.is_file() else None
            provenance["built_at"] = int(store.clock())
            sha256, size = digest_file(output), output.stat().st_size
            if not size:
                raise ValueError("The build produced an empty artifact")
            (output.parent / "manifest.json").write_bytes(canonical_json(manifest))
            (output.parent / "provenance.json").write_bytes(canonical_json(provenance))
            store.finish_build(job["id"], sha256, size, provenance)
            published = True
        except APIError:
            store.fail(job["id"], "The key's build grant is no longer valid.")
        except subprocess.TimeoutExpired:
            store.fail(job["id"], "Build or preflight timed out; the operator can inspect the private job log.")
        except (OSError, ValueError, subprocess.SubprocessError) as exc:
            # Errors raised deliberately by this worker are safe to expose;
            # raw OS/subprocess diagnostics may contain private host details.
            message = str(exc) if isinstance(exc, ValueError) else "Worker prerequisite or build failed; the operator can inspect the private job log."
            (work / "error.txt").write_text(str(exc), encoding="utf-8")
            store.fail(job["id"], message)
        except BaseException:
            store.fail(job["id"], "Worker interrupted; request a fresh build after the operator restarts it.")
            raise
        finally:
            if backup is not None:
                if published:
                    backup.unlink(missing_ok=True)
                else:
                    bundle.unlink(missing_ok=True)
                    backup.replace(bundle)
        return True


def work_forever(store: Store, repository: Path, *, once=False, timeout=7200):
    while True:
        processed = process_one(store, repository, timeout=timeout)
        if once:
            return
        if not processed:
            time.sleep(2)
