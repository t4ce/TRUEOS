# Codex guest heap exhaustion, 2026-10-17

The supplied `bld/baremetal-logs/LatestOfThree.logs` resolves to
`trueos-baremetal.0.log`. It confirms guest VM 0 runs out of its own 1 GiB
heap. The screenshot's 1,610,243-byte allocation failure is also present in
this capture. This establishes guest heap exhaustion; it does not establish
exhaustion of physical machine memory.

## Evidence

Line numbers refer to the supplied capture, before any log rotation.

- Line 6097: after loading the 121,749,264-byte executable image, the guest
  has 640,129,560 free bytes.
- Lines 11999–12029: 29 consecutive successful allocations of 16 MiB,
  aligned to 4096 bytes, consume 464 MiB. The first allocation has a matching
  successful pthread-create record. The burst occurs during plugin scanning.
- Lines 12030–12125: 24 further stack allocations fail. The scheduler
  explicitly reports `reason=stack`, with 37 live threads against a limit
  of 256. The initial failure still has 23,060,920 free bytes, but its
  largest free block is only 11,339,264 bytes, too small for a 16 MiB stack.
- Lines 12224–12235: the final 1,610,243-byte allocation fails with only
  560,896 free bytes and a largest free block of 412,368 bytes. Rust then
  reports allocation failure and Blueprint aborts.

The final allocator trace has no caller addresses. It does not identify the
object being allocated, nor does it attribute every stack in the burst to a
particular higher-level operation.

## Change

In `../codex/codex-rs/arg0/src/lib.rs`, the main Codex runtime now uses
`max_blocking_threads(8)` when targeting TRUEOS. Other platforms retain
Tokio's default. Thread stacks remain 16 MiB.

Previously, this runtime left Tokio's blocking-worker limit at 512. TRUEOS
backs each guest thread stack eagerly from the guest heap
(`src/r/threads/stack.rs`), so a burst of blocking work can consume heap
space long before reaching Tokio's limit. Tokio filesystem operations also
use its blocking pool. The new limit queues excess work and bounds this
runtime's blocking-worker stack backing to 128 MiB, excluding async workers,
the main thread and independently spawned threads.

This prevents the main runtime's blocking pool from reproducing the observed
stack-allocation burst. It is not a general bound on all guest allocations,
and the capture alone cannot rule out other memory-pressure sources.

## Validation

The two runtime regression tests pass with
`cargo test -p codex-arg0 --lib runtime_tests --offline`:

- TRUEOS policy: 24 blocking jobs finish, at most eight run concurrently,
  and an async task runs while the blocking pool is saturated.
- Other-platform policy: all 24 blocking jobs can start concurrently.

All 10 `codex-arg0` library tests also pass with
`cargo test -p codex-arg0 --lib --offline`. Rust formatting and whitespace
checks pass. The new tests are in
`../codex/codex-rs/arg0/src/runtime_tests.rs`.

No guest package was rebuilt or deployed and no rig was reset. The original
authenticated Codex workflow still requires native validation with a rebuilt
package.
