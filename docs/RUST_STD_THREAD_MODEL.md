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
`tools/apply_trueos_rust_std_thread_backend.py` performs only this selector and
source installation against a Rust source checkout.

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
explicitly as unsupported. That failure is intentional until a separate native
worker/carrier adapter is designed for that operation.

This keeps dormant compatibility machinery from defining the execution model,
while preserving a precise failure point for code that genuinely asks for an
OS-thread-shaped operation.
