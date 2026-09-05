# TRUEOS Rust `std::thread` model

## Statement of truth

TRUEOS is a concurrent target, but a POSIX/OS thread is not a native TRUEOS
execution object.

The Rust target must therefore keep its real concurrency properties (including
atomics and `target_has_threads`) while selecting a `std::thread` backend that
does not manufacture pthread lifecycle semantics.

Native execution is described separately:

```text
Blueprint fore -> executor -> task
                         |
                         +-> explicit TRUEOS worker/carrier capacity when needed
```

`std::thread` is compatibility vocabulary, not the native execution ontology.

## Required std backend behavior

For `target_os = "trueos"`:

- `Thread::new` returns `io::Error::UNSUPPORTED_PLATFORM`.
- The native `Thread` representation is uninhabited; no pthread handle exists.
- `join` is therefore unreachable for a successfully-created native thread.
- `current_os_id` returns `None`; logical TRUEOS execution IDs are not relabeled
  as OS-thread IDs.
- `available_parallelism` returns `io::Error::UNKNOWN_THREAD_COUNT`; TRUEOS
  worker/carrier capacity is not inferred through `std::thread`.
- `set_name` is a no-op because there is no native std thread to name.
- `sleep` remains a synchronous operation and maps to the canonical
  `trueos_cabi_sleep_ms` platform primitive.
- `yield_now` is a best-effort handoff through `trueos_cabi_poll_once`; it does
  not create or switch an OS thread.

Synchronization, atomics, TLS/WLS, tasks, executors, and workers are independent
concerns and are not disabled by the absence of native std-thread lifecycle.

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
`tools/apply_trueos_rust_std_thread_backend.py` installs this selector and source and gates the Unix pthread-handle
extensions against a Rust source checkout.

## Expected Tokio/Axum consequence

A Tokio current-thread runtime may structurally contain blocking-pool state, but
merely linking or dropping that runtime must no longer require `pthread_create`,
`pthread_join`, `pthread_detach`, or `pthread_kill` from TRUEOS.

The normal server execution path remains:

```text
MAIN
 -> Tokio current-thread executor
 -> Tokio Task (Axum accept loop)
 -> Tokio Task per HTTP connection
 -> Hyper/Axum handler futures
 -> Mio/Tokio readiness
 -> TRUEOS network platform
```

If code actually asks Tokio for `spawn_blocking` (or otherwise invokes
`std::thread::spawn`), thread creation reaches the TRUEOS std backend and fails
explicitly as unsupported. That failure remains intentional. Explicit native work instead uses
`trueos::worker::spawn`; it does not activate Tokio's std-thread pool.

This keeps dormant compatibility machinery from defining the execution model,
while preserving a precise failure point for code that genuinely asks for an
OS-thread-shaped operation.


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
TRUEOS because no raw pthread handle exists. Unrelated Unix targets keep their
extensions. Installed backend/selector/WLS/clock sources enter the std cache
fingerprint. The old TRUEOS Unix-thread lifecycle patching is no longer applied.

Synchronous sleep rounds nanoseconds up to milliseconds and the CABI issues
bounded requests until the entire duration is consumed, including durations
longer than the Hull VMCALL limit. Native guest sleep/yield does not reenter the
local executor. Exported platform waits use zero for immediate observation and
`u64::MAX` for infinity; internal WaitQueue callers keep their existing contract.
Completion waits observe notification generation before testing the predicate.

## Compatibility boundaries and verification

`trueos::net::resolve_host` runs hostname lookup through native work, preserving
std resolver results and errors; numeric addresses bypass worker allocation.
Raw Tokio generic hostname lookup and actual Tokio asynchronous stdin/stdout/
stderr operations still depend on its unsupported blocking pool. Constructing
stdio handles does not establish working asynchronous stdio. TRUEOS's custom
Tokio filesystem adapter retains its asynchronous CABI path.

`tools/test_trueos_rust_std_thread_backend.py` exercises installer fixtures;
`tools/check_native_worker_contract.py --blueprints ../TRUEOS-Blueprints` compares
both SDK declarations, native definitions, loader exports, and VMCALL constants.
Rust regression cases cover duration rounding/chunking, lost completion wakeups,
admission/close interleavings, generation reuse, closure rejection and detach.
These changes require subsequent pinned-toolchain compilation and rig execution;
source checks alone are not evidence of runtime success.
