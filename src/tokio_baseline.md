# TRUEOS Tokio Baseline

This is the small boot baseline we care about for Tokio-class runtime work. It is not a general log transcript; it records the probe surfaces that are currently meaningful for VMX blueprints, kernel services, and the TRUEOS std-ABI shim.

## Version Surface

- Tokio is wired as `tokio 1.52.1` with `feature full` through the TRUEOS std-ABI shim.
- Mio is probed directly as `mio 1.2.0`.
- Hyper is probed directly as `hyper 1.9` with HTTP/1 loopback coverage.
- `serde_yaml 0.9.34` std wrapper is alive beside `serde_json`.

## Std ABI Surface

- `std.thread_local` boot canary is armed.
- pthread mutex/cond shim routes through TRUEOS spin wait states.
- recursive `std::sync::Mutex::try_lock` correctly blocks.
- `parking_lot` sync surface passes.

## Tokio Runtime Surface

Current-thread runtime is good at early boot:

- `rt.build current_thread`
- `rt.block_on async-body`
- task yield, spawn/join, local set, join set, join macros, try_join macro, abort
- `select`
- sync primitives: oneshot, mpsc, watch, broadcast, notify, mutex, rwlock, semaphore, barrier
- time: sleep, timeout, elapsed timeout, interval
- IO utility: duplex plus stdio handles
- test utility: pause/advance

Multi-thread runtime is readiness-gated:

- build is deferred until `BACKGROUND_AP_WORKER_READY`
- after the gate, builder surface, build, spawn/join, execution lifecycle, and shutdown pass

## Blocking And Carrier Lanes

Blocking work is readiness-gated:

- `spawn_blocking_canary` is deferred until `BACKGROUND_AP_WORKER_READY`
- after the gate, `spawn_blocking_canary` passes
- `blocking.rt.shutdown_timeout` passes

The runtime worker path currently uses TRUEOS AP2+ background spawners. Stackkeeper reserves tagged Tokio lanes for blocking jobs and records `cpu_slot`, `core_kind`, and scratch stack.

`std.thread_local` carrier isolation is accepted only under vthread backing. The important rule from the probe is: use vthread TLS identity for Rayon-style schedulers and similar carrier-hopping workloads.

## Vthread Signal

The vthread probe resumes after hardware tagging and Tokio readiness. It confirms distinct vthread ids, FS bases, TLS addresses, lane indexes, and AP cpu slots. This is the baseline signal that Tokio carrier execution can be made compatible with TRUEOS TLS identity when vthread backing is active.

## Network Surface

Early net readiness is staged:

- `net.socket2.new` passes before full network readiness
- Tokio net is deferred until `NET_SOCKET_READY`
- Mio net is deferred until `NET_ANY_CONFIGURED`

After readiness:

- Mio TCP listener bind passes
- Mio TCP stream connect passes through `mio_compat`
- Mio UDP bind, writable registration, and writable poll pass
- Tokio TCP listener bind passes
- Tokio UDP bind and writable pass

The VMX blueprint `tokio_net.bp` reaches the guest net bootstrap before the first socket constructor:

- launches on a reserved VM hull lane
- imports resolve with `unresolved=0`
- app fs root is prepared under `apps/tokio_net/...`
- current thread id and yield pass
- current-thread runtime builder/build/drop passes, including time-enabled build
- IO-enabled current-thread runtime build/drop passes
- `mio.poll.wake.bootstrap` passes with one event
- `runtime.current_thread_net.build` passes
- the first observed failure point was `net.socket2.new`
- after the TCP socket CABI bridge, `net.socket2.new` passes
- the next observed failure point is `mio.net.udp.bind`

The `net.socket2.new` failure resolved to the direct `trueos_cabi_socket_tcp_open` path touching the host `SOCKETS` registry. That registry uses host heap-backed `BTreeMap` storage, and host heap is intentionally not mapped into the VMX guest EPT. The HV log correctly reported `pf-host-heap-risk src=1`.

The `mio.net.udp.bind` failure is the same class one layer higher: BP guest code enters the direct `trueos_mio_*` host compatibility surface, whose socket vectors and selector registration state live in host memory.

Code-side status: the TCP socket CABI now has a VM-call bridge for open, close, nonblocking, bind, connect, poll-connect, send, recv, shutdown, take-error, and peer address queries. The direct Mio compatibility CABI now also has a VM-call bridge for listener bind, stream connect, UDP bind/connect/send/recv, socket close/address/error helpers, accept, selector registration, deregistration, and poll. `tokio_net.bp` still needs a fresh boot validation before being promoted from "Mio/socket bridge in code" to "guest net baseline passes".

Secure DNS is probe-sensitive:

- `raw.githubusercontent.com` secure lookup failed with `NoAnswer` after DoH/DoT endpoint timeouts
- `time.google.com` later resolved over DoH and DoT, with a disagreement warning between returned IPs

Treat secure DNS failure as a network/TLS/DNS availability signal, not a Tokio runtime failure by itself.

## FS Surface

Kernel filesystem runtime ops are readiness-gated:

- surface check passes early through the TRUEOS CABI backend
- runtime ops defer until `TRUEOSFS_ROOT_MOUNTED`

After root mount, the kernel Tokio probe passes:

- write passes
- read passes
- read_to_string passes
- remove_file passes
- CABI helper path passes
- file write/flush passes
- file read_to_end passes
- read/drop passes
- remove-after-read passes
- fs probe suite passes
- fs runtime shutdown passes

The VMX blueprint `tokio_fs.bp` now passes the productive guest path too:

- launches on a reserved VM hull lane
- imports resolve with `unresolved=0`
- app fs root is prepared under `apps/tokio_fs/...`
- current thread id and yield pass
- current-thread runtime builder/build/drop passes, including time-enabled build
- runtime `block_on` enters the fs sequence
- `tokio::fs::write` passes
- `tokio::fs::read` passes with expected length
- `tokio::fs::read_to_string` passes with expected length
- `OpenOptions` surface passes
- file write/flush passes
- file read_to_end passes
- seek/rewrite/flush passes
- `try_exists` passes
- nested `create_dir_all` plus write/read passes
- remove_file passes
- blueprint returns normally
- VM preserve snapshot is saved by `hv-store`

This makes `tokio_fs.bp` a clean baseline for guest std/Tokio FS routing through the VMX CABI path, not only for the in-kernel Tokio probe path.

## Hyper Surface

Hyper is alive beside Tokio:

- HTTP/1 client loopback request/response passes
- HTTP/1 server loopback request/response passes
- network HTTP/1 probing remains spawn-service readiness-gated

## Kernel Service Interaction

The chat HTTP service launches after `NET_V4_CONFIGURED + TRUEOSFS_ROOT_MOUNTED`. In this baseline it submits its Tokio runtime to the blocking lane and reaches TCP listen.

This is useful as the production-service counterpart to the synthetic Tokio probes: current-thread surfaces, multi-thread runtime, spawn-blocking, network, FS, and Hyper-style HTTP can coexist when the readiness gates fire in order.

## Baseline Interpretation

Green:

- Tokio current-thread runtime surface
- Tokio multi-thread runtime after AP worker readiness
- blocking jobs after AP worker readiness
- vthread/TLS identity under carrier backing
- Mio/socket2 after net readiness
- Tokio net after socket readiness
- FS runtime ops after root mount
- Hyper HTTP/1 loopback
- `tokio_fs.bp` guest FS baseline

Watch:

- secure DNS endpoint availability and DoH/DoT disagreement
- carrier lane ownership when VMX blueprints and Tokio blocking jobs run together
- any `std.thread_local` user that assumes OS-thread identity instead of vthread identity
- `tokio_net.bp` after the socket and Mio CABI VM-call bridges land in a boot image
