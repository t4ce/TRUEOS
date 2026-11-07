#!/usr/bin/env python3
"""Inspect builtin ELF imports and the kernel contracts behind their names.

This is a read-only source/artifact inventory, not a runtime trace. It requires
the host `nm` tool and never rebuilds, loads, publishes, or runs a builtin.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[2]


def names(text):
    return set(text.split())


# Semantic families are explicit; the legacy Unix classifier stays unchanged.
# In particular, a TRUEOS prefix does not establish a different implementation.
FAMILIES = {
    "legacy_c_pointer_allocation": names(
        "malloc calloc realloc free posix_memalign malloc_usable_size"),
    "native_named_c_pointer_allocation": names(
        "trueos_cabi_alloc trueos_cabi_calloc trueos_cabi_realloc "
        "trueos_cabi_free trueos_cabi_malloc_usable_size"),
    "rust_system_layout_allocation": names("sys_alloc_aligned sys_dealloc"),
    "posix_mutex": names(
        "pthread_mutex_init pthread_mutex_destroy pthread_mutex_lock "
        "pthread_mutex_trylock pthread_mutex_unlock pthread_mutexattr_init "
        "pthread_mutexattr_destroy pthread_mutexattr_settype"),
    "posix_condition_variable": names(
        "pthread_cond_init pthread_cond_destroy pthread_cond_wait "
        "pthread_cond_timedwait pthread_cond_signal pthread_cond_broadcast "
        "pthread_condattr_init pthread_condattr_destroy pthread_condattr_setclock"),
    "posix_tls": names(
        "pthread_key_create pthread_key_delete pthread_getspecific pthread_setspecific"),
    "posix_thread_lifecycle": names(
        "pthread_create pthread_join pthread_detach pthread_self pthread_setname_np "
        "pthread_attr_init pthread_attr_destroy pthread_attr_setstacksize"),
    "native_execution_tls": names(
        "trueos_platform_thread_local_get trueos_platform_thread_local_set "
        "trueos_platform_thread_local_register_cleanup"),
    "native_generation_wait": names(
        "trueos_tokio_platform_wait_observe trueos_tokio_platform_wait_after "
        "trueos_tokio_platform_wake_one trueos_tokio_platform_wake_all"),
    "native_named_thread_join": names(
        "trueos_cabi_thread_join trueos_cabi_thread_detach"),
    "fd_transport_and_flags": names(
        "close dup dup2 fcntl fcntl64 flock read readv write writev pipe poll socketpair"),
    "posix_terminal": names("ioctl isatty cfmakeraw tcgetattr tcsetattr"),
    "posix_filesystem": names(
        "open open64 openat access chdir getcwd fstat fstat64 fstatat fstatat64 "
        "stat stat64 lstat lstat64 readlink realpath mkdir mkdirat rmdir linkat "
        "rename renameat unlink unlinkat lseek pread pread64 pwrite pwrite64 "
        "ftruncate ftruncate64 fsync fdatasync fchmod fchown futimens futimes utimes statvfs"),
    "posix_network": names(
        "socket bind listen accept accept4 connect recv recvfrom send sendto shutdown "
        "setsockopt getsockopt getpeername getsockname getaddrinfo freeaddrinfo gai_strerror"),
    "errno_and_environment": names(
        "errno_location __errno __errno_location environ getenv setenv unsetenv "
        "strerror_r __xpg_strerror_r"),
    "posix_time_calendar": names("clock_gettime gettimeofday nanosleep localtime_r time"),
    "posix_process_signals": names(
        "fork execvp waitpid getpid geteuid setuid setgid setgroups setpgid setsid "
        "chroot signal sigaction raise"),
    "dynamic_library_loader": names("dlopen dlclose dlsym dlerror dladdr"),
    "byte_string_character_helpers": names(
        "memcpy memmove memset memcmp strlen memchr strchr strcmp strcspn strncmp "
        "strrchr strspn abs iswspace iswalpha iswdigit iswalnum __ctype_b_loc "
        "__memcpy_chk __memmove_chk __memset_chk __stack_chk_fail qsort"),
    "math_helpers": names(
        "sin cos tan asin acos atan atan2 sinh cosh asinh acosh atanh log log2 "
        "log10 log1p exp exp2 expm1 pow sqrt ceil floor trunc round fabs fmod ldexp "
        "sinf cosf tanf asinf acosf atanf atan2f sinhf coshf asinhf acoshf atanhf "
        "logf log2f log10f log1pf expf exp2f expm1f powf sqrtf ceilf floorf truncf "
        "roundf fabsf fmodf ldexpf"),
    "c_abi_termination": names("exit _exit abort"),
}


def file_info(path):
    if not path.is_file():
        return {"path": str(path), "exists": False}
    data = path.read_bytes()
    return {"path": str(path), "exists": True, "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
            "mtime_ns": path.stat().st_mtime_ns}


def resolver_arms(path):
    """Read direct Some(function as *const ...) match arms, including aliases."""
    source = path.read_text()
    pattern = (r'((?:"[^"\n]+"\s*(?:\|\s*)?)+)=>\s*(?:\{\s*)?'
               r'Some\(\s*([\w:]+)\s+as\s+\*const')
    result = {}
    for match in re.finditer(pattern, source):
        for symbol in re.findall(r'"([^"\n]+)"', match[1]):
            result[symbol] = {"implementation": match[2],
                              "resolver_source": str(path),
                              "line": source.count("\n", 0, match.start()) + 1}
    return result


def function_source(path, name):
    """Top-level Rust item convention used by the audited shim helpers."""
    source = path.read_text()
    match = re.search(rf'^(?:pub(?:\([^)]*\))? )?(?:unsafe )?'
                      rf'(?:extern "[^"]+" )?fn {re.escape(name)}\(', source, re.M)
    if match is None:
        return None
    end = source.find("\n}", match.end())
    if end == -1:
        return None
    return {"source": str(path), "line": source.count("\n", 0, match.start()) + 1,
            "text": source[match.start():end + 2]}


def allocation_evidence(resolvers):
    """Confirm named frontends still reach registry helpers in current source."""
    path = ROOT / "src/std_abi_shim.rs"
    adapter_path = ROOT / "src/blueprint_shims.rs"
    proof = {}
    for name, method in (("c_allocation_insert", "insert"),
                         ("c_allocation_remove", "remove"),
                         ("c_allocation_get", "get")):
        item = function_source(path, name)
        confirmed = bool(item and re.search(
            rf'C_ALLOCATIONS\.lock\(\)\.{method}\(', item["text"]))
        proof[name] = dict(item or {}, registry_helper_confirmed=confirmed,
                           status="confirmed" if confirmed else "unconfirmed")
    helpers = {
        "c_malloc_aligned": "c_allocation_insert",
        "c_free_ptr": "c_allocation_remove",
        "c_realloc_ptr": "c_allocation_get",
        "calloc": "c_malloc_aligned",
        "trueos_cabi_malloc_usable_size": "c_allocation_get",
    }
    for name, callee in helpers.items():
        item = function_source(path, name)
        confirmed = bool(item and proof[callee]["registry_helper_confirmed"] and
                         re.search(rf'\b{callee}\s*\(', item["text"]))
        proof[name] = dict(item or {}, registry_helper_confirmed=confirmed,
                           status="confirmed" if confirmed else "unconfirmed")
    frontends = {}
    for name in sorted(FAMILIES["native_named_c_pointer_allocation"] |
                       FAMILIES["legacy_c_pointer_allocation"]):
        implementation = resolvers.get(name, {}).get("implementation", "")
        if not implementation:
            continue
        implementation_file = (path if implementation.startswith("crate::std_abi_shim::")
                               else adapter_path if implementation.startswith(
                                   "crate::blueprint_shims::") else None)
        item = (function_source(implementation_file, implementation.rsplit("::", 1)[-1])
                if implementation_file else None)
        body = item["text"].split("{", 1)[1] if item else ""
        callees = [helper for helper, evidence in proof.items()
                   if evidence["registry_helper_confirmed"] and item and
                   re.search(rf'\b{helper}\s*\(', body)]
        frontends[name] = {"implementation": implementation,
                           "registry_path_confirmed": bool(callees),
                           "status": "confirmed" if callees else "unconfirmed",
                           "calls_registry_helpers": callees}
        if item:
            frontends[name].update(source=item["source"], line=item["line"])
    return {"source": file_info(path), "frontends": frontends,
            "adapter_source": file_info(adapter_path), "helper_evidence": proof}


def audit(builtins):
    classifier_path = ROOT / "src/unix_compat.rs"
    source = classifier_path.read_text()
    classifier = source.split("pub(crate) fn is_unix_import", 1)[1].split(
        "pub(crate) fn resolve_import", 1)[0]
    unix_names = set(re.findall(r'"([^"\n]+)"', classifier))
    unsupported = source.split("pub(crate) fn unsupported_unix_import_reason", 1)[1].split(
        "pub(crate) fn is_unix_import", 1)[0]
    unsupported_names = set(re.findall(r'"([A-Za-z_]\w*)"', unsupported))
    generated = sorted(ROOT.glob(
        "tgt/x86_64-unknown-trueos/*/build/TRUEOS-*/out/generated_portal_imports.rs"),
        key=lambda path: path.stat().st_mtime_ns)
    # Use only the newest generated CABI resolver; optional before the first build.
    sources = [classifier_path, *generated[-1:], ROOT / "src/hv/blueprint/blueprint.rs"]
    resolvers = {}
    for path in sources:
        resolvers.update(resolver_arms(path))
    for name in unsupported_names:
        resolvers.pop(name, None)
    alloc = allocation_evidence(resolvers)
    families = dict(FAMILIES)
    families["confirmed_registry_backed_allocation"] = {
        name for name, evidence in alloc["frontends"].items()
        if evidence["registry_path_confirmed"]}

    ready_path = builtins / "ready.json"
    ready = json.loads(ready_path.read_text()) if ready_path.is_file() else {}
    objects = sorted(builtins.glob("objects/*/module.stripped.o"))
    selected = ready.get("packages")
    unselected = []
    missing = []
    if isinstance(selected, dict):
        # Object caches outlive retired packages. Count only the generation
        # selected by ready.json, and keep stale/missing artifacts visible.
        unselected = [module for module in objects if module.parent.name not in selected]
        objects = [module for module in objects if module.parent.name in selected]
        missing = sorted(set(selected) - {module.parent.name for module in objects})
    if not objects:
        raise ValueError(f"no objects/*/module.stripped.o under {builtins}")
    consumers, all_imports = {}, set()
    for module in objects:
        symbols = sorted({line.split()[-1] for line in subprocess.check_output(
            ["nm", "-u", str(module)], text=True).splitlines() if line.strip()})
        imported = set(symbols)
        all_imports.update(imported)
        name = module.parent.name
        package = file_info(builtins / "dist" / f"{name}.bp")
        expected = ready.get("packages", {}).get(name)
        consumers[name] = {
            "module": file_info(module), "package": package,
            "ready_package_sha256": expected,
            "package_matches_ready": (package.get("sha256") == expected
                                      if expected and package["exists"] else None),
            "undefined_count": len(symbols), "undefined": symbols,
            "unix_compat_count": len(imported & unix_names),
            "unix_compat_imports": sorted(imported & unix_names),
            "unsupported_unix_imports": sorted(imported & unsupported_names),
            "families": {key: sorted(imported & group)
                         for key, group in families.items() if imported & group},
        }
    inventory = {}
    for family, group in families.items():
        users = [name for name, value in consumers.items() if family in value["families"]]
        inventory[family] = {"consumer_count": len(users), "consumers": users,
                             "imported_symbols": sorted(all_imports & group)}
    return {
        "utc": datetime.now(timezone.utc).isoformat(), "builtins_directory": str(builtins),
        "scope": "Selected compact builtin ELF imports, package hashes, and current kernel resolver source.",
        "limits": [
            "Static imports identify contracts, not runtime call frequency or performance cost.",
            "No claim of source freshness, loaded rig version, or historical size change.",
            "Semantic families are explicit audited name sets; every import remains in the inventory.",
            "Resolver paths are parsed from current sources, not proof of a deployed kernel binding.",
            "Direct textual match-arm parsing does not evaluate cfg or dynamic resolver conditions; source provenance is advisory.",
            "Missing or unparsed resolver arms remain visible as unknown; no fallback success is inferred.",
            "Unconfirmed allocation paths mean insufficient textual evidence, not absence of registry use.",
        ],
        "classifier": dict(file_info(classifier_path), symbol_count=len(unix_names)),
        "ready_manifest": file_info(ready_path),
        "consumer_selection": "ready manifest packages" if isinstance(selected, dict) else "all cached objects (no ready package set)",
        "unselected_cached_modules": [file_info(path) for path in unselected],
        "selected_missing_modules": missing,
        "resolver_sources": [file_info(path) for path in sources],
        "generated_cabi_resolver_available": bool(generated),
        "allocation_implementation_evidence": alloc,
        "consumer_count": len(consumers), "consumers": consumers, "families": inventory,
        "import_implementations": {name: resolvers[name] for name in sorted(all_imports)
                                   if name in resolvers},
        "unknown_resolver_imports": sorted(all_imports - resolvers.keys()),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--builtins", type=Path, default=ROOT / "tgt/buildins/release",
                        help="Builtin output directory (default: %(default)s)")
    parser.add_argument("--output", type=Path, help="Write JSON here instead of stdout")
    args = parser.parse_args()
    try:
        result = audit(args.builtins.resolve())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"audit_platform_contracts: {error}\n")
    data = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(data)
    else:
        print(data, end="")


if __name__ == "__main__":
    main()
