#!/usr/bin/env python3
"""Exercise production thread CABI/proxies, dispatch arms, and launch state.

Only carrier admission, VM identity/topology, and communication-page transport
are modeled. Control functions, both VMCALL dispatch arms, std CABI wrappers,
and generation-scoped policy are read unchanged from production source.
"""

from pathlib import Path
import json
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def function(source: str, name: str) -> str:
    match = re.search(rf'^(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(?:extern\s+"C"\s+)?fn\s+{name}\b', source, re.MULTILINE)
    if match is None:
        raise ValueError(f"missing production function {name}")
    closing = re.search(r"^}\s*$", source[match.end():], re.MULTILINE)
    if closing is None:
        raise ValueError(f"missing closing brace for {name}")
    return source[match.start():match.end() + closing.end()] + "\n"


def main() -> None:
    threads = (ROOT / "src/r/threads.rs").read_text()
    dispatch = (ROOT / "src/hv/vmcall.rs").read_text()
    cabi = (ROOT / "src/std_abi_shim.rs").read_text()
    source = (Path(__file__).with_name("thread_control_abi") / "harness.rs").read_text()
    source = source.replace("@POLICY@", json.dumps(str(ROOT / "src/workers/application_policy.rs")))
    source = source.replace("@PLACEMENT@", json.dumps(str(ROOT / "src/r/threads/placement.rs")))
    names = ["current_id", "swap_native_scope", "spawn", "submit_guest_from_raw", "hull_thread_control", "hull_control", "configure_placement", "swap_scope"]
    source = source.replace("@THREAD_FUNCTIONS@", "\n".join(function(threads, name) for name in names))
    names = ["trueos_cabi_thread_configure_placement_v1", "trueos_cabi_thread_swap_child_role_v1", "trueos_cabi_thread_swap_activity_v1"]
    source = source.replace("@CABI_FUNCTIONS@", "\n".join(function(cabi, name) for name in names))
    constants = ["OP_BP_THREAD_CONTROL_V1", "OP_BP_THREAD_SUBMIT", "STATUS_OK", "STATUS_BAD_ARG"]
    source = source.replace("@CONSTANTS@", "\n".join(re.search(rf"^pub const {name}:.*$", dispatch, re.MULTILINE).group(0) for name in constants))
    start = dispatch.index("        OP_BP_THREAD_CONTROL_V1 => {")
    end = dispatch.index("        OP_BP_MIO_SOCKET_POLL_READY => {", start)
    source = source.replace("@DISPATCH_ARMS@", dispatch[start:end])
    with tempfile.TemporaryDirectory(prefix="trueos-thread-control-abi-") as temporary:
        directory = Path(temporary)
        fixture = directory / "contracts.rs"
        fixture.write_text(source)
        for disabled in [False, True]:
            executable = directory / ("disabled" if disabled else "enabled")
            command = ["rustc", "--edition=2024", "--test", str(fixture), "-o", str(executable)]
            if disabled:
                command += ["--cfg", "experiment_disabled"]
            subprocess.run(command, check=True)
            subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
