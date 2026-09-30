from pathlib import Path

LIVE = (
    "src/mio_compat.rs",
    "src/std_abi_shim.rs",
    "src/hv/vmcall.rs",
    "crates/trueos-vm/src/vmcall.rs",
    "src/hv/blueprint/blueprint.rs",
    "src/net/adapter.rs",
)
FORBIDDEN = (
    "SelectorRegistration",
    "MioLocalRegistration",
    "MIO_SELECTOR_WAIT",
    "MIO_LOCAL_REGISTRATIONS",
    "INTEREST_EDGE_MANAGED",
    "TrueosMioReadyEvent",
    "OP_BP_MIO_SELECTOR_",
    "trueos_mio_selector_",
    "mio_selector_register_socket",
    "mio_selector_deregister_socket",
    "mio_selector_poll",
    "mio_selector_wake",
    "notify_mio_local_fd_event",
    "notify_net_event",
)

for path in LIVE:
    source = Path(path).read_text()
    for forbidden in FORBIDDEN:
        if forbidden in source:
            raise SystemExit(f"{path}: duplicate Mio selector residue: {forbidden}")

mio = Path("src/mio_compat.rs").read_text()
shim = Path("src/std_abi_shim.rs").read_text()
if "mio_socket_poll_ready_host" not in mio or "socket_poll_events" not in shim:
    raise SystemExit("socket readiness probing must remain behind poll(2)")

print("Mio selector boundary: userspace registration, TRUEOS readiness only")
