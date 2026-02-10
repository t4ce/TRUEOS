#!/usr/bin/env python3
from __future__ import annotations

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODS = ROOT / "crates/trueos-qjs/src/trueos_modules.rs"
SHIMS = ROOT / "crates/trueos-qjs/src/trueos_shims.rs"
IO = ROOT / "src/surface/io.rs"
HTTPS = ROOT / "src/v/net/https.rs"
NODE_RS = ROOT / "crates/trueos-qjs/src/node.rs"
ASYNC_OPS = ROOT / "crates/trueos-qjs/src/async_ops.rs"
SHELL_QJS = ROOT / "src/shell/shellqjs.rs"
WAIT_RS = ROOT / "src/wait.rs"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def extract_module_exports_map(src: str) -> dict[str, list[str]]:
    start = src.find("let (init, exports)")
    end = src.find("let m = qjs::JS_NewCModule", start)
    if start < 0 or end < 0:
        return {}
    block = src[start:end]
    out: dict[str, list[str]] = {"complex": [], "fs": [], "process": [], "path": []}
    current: str | None = None
    for line in block.splitlines():
        line = line.strip()
        if 'if name == b"complex"' in line:
            current = "complex"
            continue
        if 'else if name == b"fs"' in line:
            current = "fs"
            continue
        if 'else if name == b"process" || name == b"node:process"' in line:
            current = "process"
            continue
        if 'else if name == b"path" || name == b"node:path"' in line:
            current = "path"
            continue
        if current is None:
            continue
        for exp in re.findall(r'b"([^"\\]+)\\0"', line):
            out[current].append(exp)
    return out


def extract_process_props(src: str) -> list[str]:
    return sorted(set(re.findall(r'JS_SetPropertyStr\(ctx, obj, b"([A-Za-z0-9_]+)\\0"', src)))


def extract_env_keys(src: str) -> list[str]:
    env_block = re.search(r"// env: plain object.*?JS_SetPropertyStr\(ctx, obj, b\"env\\0\".*?$", src, re.S | re.M)
    if not env_block:
        return []
    return sorted(set(re.findall(r'JS_SetPropertyStr\(ctx, env, b"([A-Za-z0-9_]+)\\0"', env_block.group(0))))


def extract_qjs_cabi_imports(src: str) -> list[str]:
    block = re.search(r'extern "C" \{(.*?)\n\}', src, re.S)
    if not block:
        return []
    return sorted(re.findall(r"pub fn ([a-zA-Z0-9_]+)\(", block.group(1)))


def extract_no_mangle_symbols(src: str) -> list[str]:
    return re.findall(r"#\[no_mangle\]\s*\npub unsafe extern \"C\" fn ([a-zA-Z0-9_]+)\(", src)


def extract_kernel_cabi_exports(src: str, prefix: str) -> list[str]:
    return sorted(
        re.findall(
            rf'pub (?:unsafe )?extern "C" fn ({re.escape(prefix)}[a-zA-Z0-9_]+)\(',
            src,
        )
    )


def status(flag: bool) -> str:
    return "Yes" if flag else "Partial/No"


def main() -> None:
    mods = read(MODS)
    shims = read(SHIMS)
    io = read(IO)
    https = read(HTTPS)
    node_rs = read(NODE_RS)
    async_ops = read(ASYNC_OPS)
    shell_qjs = read(SHELL_QJS)
    wait_rs = read(WAIT_RS)

    module_exports = extract_module_exports_map(mods)
    process_props = extract_process_props(mods)
    env_keys = extract_env_keys(mods)
    qjs_cabi_imports = extract_qjs_cabi_imports(shims)
    qjs_libc = extract_no_mangle_symbols(shims)
    kernel_fs_cabi = extract_kernel_cabi_exports(io, "trueos_cabi_fs_")
    kernel_net_cabi = extract_kernel_cabi_exports(https, "trueos_cabi_net_fetch_")

    # Runtime/semantic checks (heuristic, static analysis only).
    has_node_process = bool(module_exports.get("process"))
    has_node_path = bool(module_exports.get("path"))
    has_node_worker_threads = ("worker_threads" in node_rs) or ("node:worker_threads" in mods)
    has_nexttick_queue = "call immediately" not in mods and ("nextTick queue" in mods or "microtask" in mods)
    has_execute_pending_job = "JS_ExecutePendingJob" in shell_qjs
    has_cwd_mutation = all(x in process_props for x in ["cwd"]) and ("qjs_process_chdir" in mods)
    has_blocking_wait = "spawn_and_wait_local" in wait_rs
    has_async_promise_pump = "pub unsafe fn pump(" in async_ops
    has_net_async_abi = all(k in qjs_cabi_imports for k in ["trueos_cabi_net_fetch_start", "trueos_cabi_net_fetch_result", "trueos_cabi_net_fetch_discard"])
    has_timer_api = "setTimeout" in mods or "setInterval" in mods

    semantics_rows = [
        (
            "Worker threads (`worker_threads`)",
            "Yes",
            status(has_node_worker_threads),
            "not found in native loader/shims",
        ),
        (
            "Node-like nextTick ordering",
            "Queued microtask semantics",
            status(has_nexttick_queue),
            "currently immediate call path",
        ),
        (
            "Pending job/microtask execution",
            "Yes",
            status(has_execute_pending_job),
            "JS_ExecutePendingJob hook in qjs shell path",
        ),
        (
            "Mutable process cwd (`cwd/chdir`)",
            "Yes",
            status(has_cwd_mutation),
            "process.chdir + normalized cwd state present",
        ),
        (
            "Evented async completion pump (Promises)",
            "Yes",
            status(has_async_promise_pump),
            "async_ops::pump bridges async_fs completions",
        ),
        (
            "Async network fetch boundary",
            "Yes",
            status(has_net_async_abi),
            "start/result/discard C-ABI in use",
        ),
        (
            "Blocking sync wrappers still in runtime",
            "Avoid for Node-like flow",
            "Yes" if has_blocking_wait else "No",
            "spawn_and_wait_local exists; use sparingly",
        ),
        (
            "Node timer globals (`setTimeout`, `setInterval`)",
            "Yes",
            status(has_timer_api),
            "no explicit timer global shim detected",
        ),
    ]

    rows = [
        (
            "Native module loader (`node:*`, npm URLs)",
            "Yes",
            status(bool(module_exports["process"]) and bool(module_exports["path"])),
            f"modules: {', '.join(k for k,v in module_exports.items() if v)}",
        ),
        (
            "`node:process` core shape",
            "Yes",
            status(all(k in process_props for k in ["env", "argv", "cwd", "versions", "platform", "arch"])),
            f"exports: {', '.join(module_exports['process'])}",
        ),
        (
            "`process.env` defaults",
            "Yes",
            status(len(env_keys) >= 4),
            f"keys: {', '.join(env_keys)}",
        ),
        (
            "`node:path` coverage",
            "Broad",
            status(set(module_exports["path"]) >= {"join"}),
            f"exports: {', '.join(module_exports['path']) or '<none>'}",
        ),
        (
            "File API for JS",
            "read/write + streams",
            status(bool(module_exports["fs"])),
            f"exports: {', '.join(module_exports['fs'])}",
        ),
        (
            "Network fetch ABI for JS loader",
            "Async",
            status(all(k in qjs_cabi_imports for k in ["trueos_cabi_net_fetch_start", "trueos_cabi_net_fetch_result", "trueos_cabi_net_fetch_discard"])),
            "start/result/discard present",
        ),
        (
            "Kernel FS C-ABI",
            "Yes",
            status(len(kernel_fs_cabi) >= 6),
            f"{len(kernel_fs_cabi)} exports",
        ),
        (
            "C runtime/libc shims for QJS",
            "Large",
            status(len(qjs_libc) >= 20),
            f"{len(qjs_libc)} shim symbols",
        ),
    ]

    print("# TRUEOS QJS Feature Matrix (vs full Node expectation)")
    print()
    print("| Capability | Full Node | TRUEOS QJS Now | Notes |")
    print("|---|---|---|---|")
    for cap, node_exp, now_s, note in rows:
        print(f"| {cap} | {node_exp} | {now_s} | {note} |")
    print()
    print("## Extracted Facts")
    print(f"- native module exports: {module_exports}")
    print(f"- process properties: {process_props}")
    print(f"- process.env default keys: {env_keys}")
    print(f"- QJS imported C-ABI symbols: {qjs_cabi_imports}")
    print(f"- kernel fs C-ABI exports: {kernel_fs_cabi}")
    print(f"- kernel net fetch C-ABI exports: {kernel_net_cabi}")
    print(f"- qjs libc shim symbols: {len(qjs_libc)}")
    print()
    print("## Runtime Semantics Matrix")
    print()
    print("| Runtime Capability | Full Node | TRUEOS QJS Now | Notes |")
    print("|---|---|---|---|")
    for cap, node_exp, now_s, note in semantics_rows:
        print(f"| {cap} | {node_exp} | {now_s} | {note} |")

    gaps: list[str] = []
    if not has_node_worker_threads:
        gaps.append("Missing `worker_threads` module support.")
    if not has_nexttick_queue:
        gaps.append("`process.nextTick` is immediate-call, not queued microtask semantics.")
    if not has_timer_api:
        gaps.append("No explicit Node timer global shim (`setTimeout`/`setInterval`) detected.")
    if has_blocking_wait:
        gaps.append("Blocking helper (`spawn_and_wait_local`) exists and can stall if overused.")

    print()
    print("## Priority Gaps")
    if gaps:
        for i, gap in enumerate(gaps, 1):
            print(f"{i}. {gap}")
    else:
        print("1. No major semantic gaps detected by current static checks.")


if __name__ == "__main__":
    main()
