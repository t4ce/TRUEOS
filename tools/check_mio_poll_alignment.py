from pathlib import Path

mio = Path("src/mio_compat.rs").read_text()
shim = Path("src/std_abi_shim.rs").read_text()
poll = Path("src/unix_abi_shim.rs").read_text()
wait = Path("src/wait.rs").read_text()
platform = Path("src/r/platform.rs").read_text()
vmcall = Path("src/hv/vmcall.rs").read_text()
adapter = Path("src/net/adapter.rs").read_text()

required = {
    "mio readiness guest bridge": (mio, "OP_BP_MIO_SOCKET_POLL_READY"),
    "owner-checked readiness probe": (mio, "mio_socket_poll_ready_for_vm_host"),
    "POSIX socket probe wrapper": (shim, "mio_socket_poll_ready(backend, interests)"),
    "VM-local IO key": (wait, "BLUEPRINT_IO_WAIT_KEY"),
    "Hull wait observe VMCall": (platform, "guest_platform_wait_observe(key)"),
    "Hull wait after VMCall": (platform, "guest_platform_wait_after(key, observed, timeout_ms)"),
    "platform wait outcome": (vmcall, "PlatformWait"),
    "network IO wake": (adapter, "platform_wake_all_blueprint_io_waiters"),
    "poll observe-before-probe": (poll, "let observed = blueprint_wait.then"),
    "poll generation wait": (poll, "trueos_tokio_platform_wait_after"),
    "spurious typed-state return": (poll, "woke_without_ready"),
}
for label, (source, needle) in required.items():
    if needle not in source:
        raise SystemExit(f"missing {label}: {needle}")

if "mio_socket_poll_ready_host(backend, interests)?" in shim:
    raise SystemExit("POSIX poll bypasses the Hull/host socket ownership boundary")
if "wakers.swap_remove(index)" not in wait:
    raise SystemExit("finite VM-local waits retain timed-out task wakers")
if "trueos_cabi_sleep_ms(sleep_ms);\n        if let Some(remaining)" in poll:
    raise SystemExit("Blueprint poll regressed to periodic sleep/rescan")

print("Mio/poll alignment: userspace registration + host readiness + VM-local wait")
