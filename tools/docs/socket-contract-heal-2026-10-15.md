# Socket contract repairs, 2026-10-15

The physical `network_contract` baseline completed 26/28 cases: native
`MSG_PEEK` consumed bytes, and `shutdown(Write)` returned success without FIN.
The kernel now implements both operations across Mio's native and Hull routes.

Peek shares the receive helper with ordinary reads, copying buffered bytes
without popping them. Its guest vmcall masks the peek flag before bounding the
copy size. Buffered bytes remain readable before peer EOF.

Shutdown tracks the two local halves independently. Write shutdown submits an
ordered `FinishTcp`; the adapter drains its pending queue before closing the
smoltcp transmit half. Reads and peer replies remain available. Local read
shutdown discards queued/future input. Queue pressure leaves shutdown retryable;
successful operations wake I/O waiters after releasing state locks. The older
socket C ABI now honors the same Read/Write/Both semantics. Loopback FIN emits
an ordered receive EOF and retains both records for replies and eventual close.
New API variants and wire tags are appended to preserve existing values.

A native QEMU rerun exposed one further defect: after FIN, a reply and peer
close could move smoltcp into TIME-WAIT with unread bytes. The adapter skipped
inactive sockets and discarded those final bytes. It now drains buffered data
before retirement, retains it under backpressure, and retries EOF delivery if
the event queue is full.

Validation: 61 host regression tests pass across the peek, Mio shutdown,
socket-CABI shutdown, wire format, backpressure, connect-close, POSIX listener,
and address-family harnesses. `cargo check`, the kernel build, and `make iso`
pass. This build does not establish that Codex's original turn stall is fixed.

The repaired kernel then passed all 28 Blueprint cases in an isolated QEMU
in 8.225 seconds, using a disposable RAM filesystem and the published Appstore
copy. The independent server observed FIN after all 307,200 bytes, two Pongs,
and four large binary frames. The probe returned its terminal and completed
teardown with zero native jobs. The runtime ELF in `bld/trueos.iso` has the same
SHA-256 as the tested kernel. See the sibling Blueprint's
`validation/healed-qemu-2026-10-15.json`, filtered guest log, and fixture records.

The updated protocol-2 probe checks repeated peeks and all 300 KiB arriving
before FIN. Its Linux baseline passes 28/28 with independently observed FIN,
Pong, and binary-frame witnesses. Build and publish it directly into the runtime
Appstore directory using:

```sh
../TRUEOS-Blueprints/probes/network_contract/build_publish.sh
```

The script keeps the source registered in `probes.json` and publishes only
`network_contract.bp` to `smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps` with its
SHA-256 filename. The rebuilt probe is published and its downloaded bytes have
been hash-verified. The new kernel must be booted to replace its ABI handlers.
The boot image is `bld/trueos.iso`; the physical rig has not been rebooted by
this repair workflow.
