# TRUEOS Rust `std::thread` model

## Statement of truth

TRUEOS is a concurrent target, but a POSIX/OS thread is not a native TRUEOS
execution object. Rust `std::thread` compatibility is implemented with
stackful logical threads on shared TRUEOS executor carriers.

The Rust target keeps its real concurrency properties, including atomics and
concurrent execution. Each accepted standard thread owns a stack, an execution
identity and a keyed TLS namespace. The custom Rust std lifecycle uses
`trueos_cabi_thread_*`; these calls do not allocate an exclusive service lane
per thread. POSIX synchronization and keyed TLS support the std internals.

Native execution is described separately:

```text
Blueprint fore -> executor -> task
                         |
                         +-> stackful std/Tokio thread tasks on shared carriers
                         |
                         +-> explicit TRUEOS worker jobs on leased service lanes
```

`std::thread` is compatibility vocabulary, not the native execution ontology.

## Required std backend behavior

For `target_os = "trueos"`:

- `Thread::new` submits through `trueos_cabi_thread_spawn`. Success transfers
  the entry data exactly once and returns an opaque nonzero handle. Failure
  never calls the entry; std reclaims its `ThreadInit` and reports platform errno.
- `join` consumes the handle and waits for the entry, keyed TLS destructors and
  std thread cleanup. Dropping the handle detaches execution without cancelling
  it. A joined handle is never detached a second time.
- `current_os_id` exposes the stable TRUEOS logical execution identity through
  std's platform identifier API.
- `available_parallelism` reports the advertised application CPU capacity.
  This is advisory; submission can fail if carriers or admission are unavailable.
- `set_name` sends the current thread's name through the platform CABI.
- `sleep` uses `trueos_cabi_sleep_ms`, rounds positive fractional milliseconds
  upwards, and chunks durations without losing time.
- `yield_now` uses `trueos_cabi_poll_once`. On a stackful thread it suspends the
  continuation so the carrier can run other executor work.

Thread parking, mutex contention, condition-variable waits, platform waits and
sleep suspend the continuation instead of recursively polling an executor or
occupying one service lane for the whole thread lifetime. Execution is
cooperative: CPU-bound code must reach a scheduling or waiting operation to
give other work on its carrier a turn.
The Hull main stack yields through VMCALL on synchronous contention; it must
never enter the host executor or access its GS-backed per-CPU state directly.
The separate critical-section spin path never suspends or polls an executor.
It also skips timer polling in the Hull, whose private timer state may retain
host waker addresses.

Before each resume, the scheduler installs the thread's VM/allocation domain,
WLS identity and errno state. Suspension restores the parent carrier's state
before polling unrelated tasks. A continuation stays on its selected carrier.
Accepted guest threads retain a reservation for their VM run generation through
stack retirement. Stop/preserve closes admission and drains accepted tasks,
including detached threads, before reclaiming code, heaps or process resources.
An unfinished cooperative thread keeps teardown pending; cancellation does not
discard a live continuation or release its executable memory on a timeout.

A VM-owned continuation also owns three PMM pages for a private host CR3 root
and its low stack branches. These preserve the Hull main stack and communication
pages at their original addresses, so compiler-generated scoped borrows access
the same physical bytes on every carrier. The other branches retain the host
kernel backing, including its globals, guest heap and guarded thread aliases.
The scheduler switches this view only while the continuation runs, flushing
global translations as well, and restores the parent CR3 before polling waits
or unrelated work. The VM reservation keeps the referenced guest leaf tables
and main-stack backing alive. Retirement frees the owned carrier tables only
after switching away from them; 256 admitted tasks bound this storage to 3 MiB.

## Rust std selection

Rust normally selects `library/std/src/sys/thread/unix.rs` for a Unix-family
target. TRUEOS must be selected before that branch:

```rust
cfg_select! {
    // ...
    target_os = "trueos" => {
        mod trueos;
        pub use trueos::{
            DEFAULT_MIN_STACK_SIZE, Thread, available_parallelism, current_os_id,
            set_name, sleep, yield_now,
        };
    }
    any(target_family = "unix", target_os = "wasi") => {
        // ordinary Unix pthread backend
        // ...
    }
}
```

The canonical reference backend is
`tools/rust-std/trueos_thread.rs`. The installer
`tools/testpy/apply_trueos_rust_std_thread_backend.py` installs this selector and
source and gates the Unix pthread-handle extensions against a Rust source checkout.

The installer selects OS-keyed TLS for TRUEOS before native/no-thread TLS
selectors. It also restores strict std current-thread initialization, replacing
the former permission to rebind a carrier's std thread handle. WLS-slot TLS is
not used for standard threads. The CABI keeps TLS values per process, thread
identity and key generation, runs up to four destructor rounds, and clears
remaining values before publishing completion. The Hull and its host-carried
threads use the same process namespace.

Installation upgrades an earlier canonical backend only when its exact SHA-256
is listed in the installer's reviewed revision table. Unknown local changes are
preserved and reported with their digest. Updating the reference must also add
the previous canonical digest to that table. All source anchors are checked
before writing; `--check` reports a pending upgrade without applying it.

## Tokio execution

Pinned, vendored Tokio 1.52.3 can create standard worker threads and its blocking
pool through the TRUEOS backend. Multi-thread runtime workers, `spawn_blocking`
and `block_in_place` use independent logical stacks. A parked persistent worker
leaves its carrier available for replacement workers and blocking-pool jobs.

Both current-thread and multi-thread runtimes retain the normal asynchronous
network path:

```text
MAIN
 -> Tokio executor
 -> Tokio Task (Axum accept loop)
 -> Tokio Task per HTTP connection
 -> Hyper/Axum handler futures
 -> Mio/Tokio readiness
 -> TRUEOS network platform
```

Runtime creation does not certify every generic std I/O operation: hostname,
file and stdio behavior still depends on its corresponding TRUEOS adapter.
Explicit `trueos::worker::spawn` remains available for work that deliberately
leases a native service lane.

## Guarded stacks and capacity

The default standard-thread stack is 2 MiB. Requests are rounded to 4 KiB pages,
with a 64 KiB platform minimum and 64 MiB maximum. Dedicated PMM backing maps
cacheable, writable, non-executable RAM between two absent guard pages. All
TRUEOS Rust target specifications enable inline stack probing, so a large Rust
frame cannot jump over the lower guard. Initialization first touches the alias
on the selected carrier; retirement happens after returning to its parent stack.

Safe Rust can share references into one thread's stack with another carrier,
for example with `std::thread::scope`. Carrier affinity alone therefore does
not prove that no remote TLB contains the stack alias. Until synchronous remote
TLB invalidation is provided, retired virtual aliases are never reused. Leaf
mappings are removed and physical backing is returned to PMM. Empty page-table
frames remain allocated so remote paging caches never reference recycled table
frames.

The dedicated virtual arena is 512 GiB. With two guard pages per reservation,
it admits 261,123 default-size stacks per boot; uniform minimum/maximum stacks
allow 7,456,540/8,191 reservations. A mapping failure after reserving an address
also consumes that window. Exhaustion rejects creation rather than wrapping or
reusing an alias. Page-table retention is bounded by about 1 GiB of leaf table
frames for the full arena, plus intermediate tables and allocator bookkeeping.

Other admission bounds are 256 simultaneously admitted stackful tasks globally,
64 joinable thread records per process, 128 TLS keys per process, and 512 nonzero
thread/key TLS values per process. Detaching releases the join record while the
running task still counts against global admission. Limits and unavailable
carriers produce explicit task/handle creation errors. TLS key and value limits
are separately reported through the pthread key/set operations.

## Native Blueprint runtime lanes

The Blueprint SDK exposes `worker::capacity()`, `worker::spawn(F)`, and an
awaitable `worker::JoinHandle<R>`. These use the existing
`trueos_service_lane_submit_job` Rust ABI and the new
`trueos_service_lane_available_capacity` query. Both repositories declare the
contract in `crates/trueos-v/src/worker_abi.rs`; this Rust object ABI requires the
pinned nightly, unlike versioned CABI structures.

Capacity is advisory and may be zero. Submission is authoritative: zero accepts
and owns the closure, while -2 (unavailable/closing), -5 (invalid job), and -6
(transport failure) consume/drop it without running it. The SDK preserves these
errors. A completion error does not represent a Tokio panic payload.

Each job leases an AP service lane and distinct concurrent WLS identity. Build,
run and drop a current-thread runtime inside the job, then return only the result.
Never move a live runtime/enter guard between lanes. Slots can be reused; a later
job may observe prior worker-local values. No new std ThreadId or fresh TLS is
promised per submission.

The kernel reserves every accepted job against its VM run generation before
queueing, rolling back if no carrier can be leased. Stop/preserve closes
admission. After VM exit, accepted work drains before resource suspension,
checkpoint capture or executable/process cleanup. A dropped join handle detaches
work; it does not cancel it. An unfinished job keeps teardown pending and its
resources retained. Native work must therefore be finite/cooperative in v1;
panic/abort recovery and forced native cancellation are not implemented.

## Builder and primitive integration

The Blueprint packer selects the pinned toolchain, checks native source ABI
agreement, and invokes this repository's installer via `TRUEOS_REPO_ROOT` or the
sibling TRUEOS checkout. Building the host packer alone does not mutate rust-src.
The installer preflights all anchors and conflicting files before writes and
supports `--check`. It excludes `std::os::unix::thread` and its prelude export for
TRUEOS because its opaque logical handle is not the generic Unix raw-handle
interface. Unrelated Unix targets keep their extensions. Installed lifecycle,
key-based TLS/current-thread and clock sources, plus the custom target
specification, enter the std cache fingerprint. The old WLS-slot TLS source
mutation is no longer applied.

Synchronous sleep rounds nanoseconds up to milliseconds and the CABI issues
bounded requests until the entire duration is consumed, including durations
longer than the Hull VMCALL limit. Native guest sleep/yield does not reenter the
local executor. Exported platform waits use zero for immediate observation and
`u64::MAX` for infinity; internal WaitQueue callers keep their existing contract.
Completion waits observe notification generation before testing the predicate.

## Compatibility boundaries and verification

`trueos::net::resolve_host` retains its native-worker hostname adapter, preserving
std resolver results and errors; numeric addresses bypass worker allocation.
Tokio's blocking pool is now available to generic blocking adapters. Constructing
stdio handles alone does not establish working asynchronous stdio. TRUEOS's
custom Tokio filesystem adapter retains its asynchronous CABI path.

The supported thread lifecycle is the custom Rust std/TRUEOS CABI path. Explicit
Unix imports `pthread_create`, `pthread_join`, `pthread_detach` and
`pthread_kill` remain rejected by the compatibility import classifier. This
does not affect the tested Rust std/Tokio path, which calls
`trueos_cabi_thread_spawn`, `trueos_cabi_thread_join` and
`trueos_cabi_thread_detach`. POSIX-shaped mutex, condition-variable and keyed
TLS operations used by std are resolved through their corresponding shims.

`tools/testpy/test_trueos_rust_std_thread_backend.py` exercises installer fixtures
and the actual std backend against a native CABI lifecycle fixture.
`tools/testpy/test_guarded_thread_stack.py` covers real guard faults and kernel
backing/ownership rollback. `tools/testpy/test_thread_scheduler.py` runs the
production continuation/wait machinery with mocked carrier services.
`tools/testpy/test_thread_carrier_tables.py` checks selective host/guest branch
copying, large-page splitting, effective permissions and carrier-table ownership.
`tools/testpy/test_spin_progress_routing.py` executes the production dispatcher
to check continuation suspension, Hull VMCALL yielding, ordinary host polling
and critical-section paths that must never suspend or reenter an executor.
`tools/testpy/check_native_worker_contract.py --blueprints ../TRUEOS-Blueprints` compares
both SDK declarations, native definitions, loader exports, and VMCALL constants.
The Blueprint `tokio_mrt` production probe covers real multi-thread runtime
startup, scoped stack borrowing and TLS separation/destructors, parked wakeups,
blocking-pool work, `block_in_place`, sockets and repeated shutdown/recreation.
Pinned-toolchain compilation and host execution are separate evidence from rig
execution; a packed probe alone does not establish success on TRUEOS hardware.

On 2026-10-03, the normal kernel and locally embedded production probe passed
the full QEMU acceptance run in `bld/thread-acceptance/qemu-run-6`. The recorded
result is `PASS` in `result.json`; `shell.log` contains the individual coverage
markers. Total boot and probe time was 12.849 seconds. Observed coverage was:

- Two joined std threads, one detached std thread, remembered and cross-thread
  park wakeups, and three exit TLS destructors.
- Two scoped children borrowing the Hull stack and two nested children borrowing
  guarded carrier stacks; distinct identities and scoped TLS destructors passed.
- Two independently constructed multi-thread runtimes, each with two scheduler
  workers, timer/yield tasks, sixteen blocking jobs, `block_in_place`, and a
  loopback TCP exchange through a listener bound to an ephemeral port. Each wave
  observed six thread starts, six stops and six TLS destructors before shutdown
  verification completed.
- Two native leased lanes across two waves, each reporting `[512, 512]` completed
  task rounds and the expected checksums.

With an ISO already built containing `tokio_mrt`, reproduce this check from the
repository root using a fresh evidence directory:

```sh
python3 tools/qemu/verify-tokio-platform.py \
  --iso bld/thread-acceptance/trueos.iso \
  --output bld/thread-acceptance/qemu-run-next --timeout 90
```

The runner uses an isolated snapshot and private loopback port forwards. It
submits the probe through legacy Shell2 TCP port 4245; Shell3 port 22 currently
does not execute Blueprint applications. This evidence establishes the tested
std/Tokio paths in QEMU, within the capacities above. Physical-machine execution
has not been verified by this run. Scheduling remains cooperative, and forced
cancellation and panic/abort recovery are not implemented; the target uses the
abort panic strategy. Generic file, hostname and stdio adapters remain subject
to their own compatibility contracts.

The additional `velosrv` smoke run in
`bld/thread-acceptance/qemu-velosrv-2` reached application initialization after
filesystem setup and import resolution. It stopped in userdata-directory
initialization because `std::env::current_exe()` returned `Unsupported`
(errno 38). Executable-path discovery remains a separate compatibility boundary;
this run does not establish complete velosrv startup.

On 2026-10-04, the `velosrv` launch environment supplies
`VELOREN_USERDATA=<VM HOME>/userdata`, selecting its existing app-scoped storage
before that fallback. The existing packed server in
`bld/thread-acceptance/qemu-velosrv-env-2` gets past the executable-path panic;
its native `vmx_env` view shows `/apps/velosrv/userdata`. Startup then stops at
the missing Veloren asset directory in the empty test filesystem. This verifies
the userdata environment bridge, not complete server startup.
