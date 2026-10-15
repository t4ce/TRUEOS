# Codex responsiveness audit, 2026-10-15

The current capture shows a local responsiveness problem in addition to MCP
startup delay. The server creates its thread in about three seconds, but queued
input waits up to 23.882 seconds and individual Codex continuations occupy their
carrier for 17–19 seconds. There is no completed real model-request send or
measured real-response TTFT in the audited snapshot. Remote model latency cannot
explain these measured local input delays.

## Capture identity and limits

Read-only snapshot at **2026-10-15 10:51:44 UTC**:

- `LatestOfThree.logs` resolved to `trueos-baremetal.0.log`.
- Snapshot length: **1,916,460 bytes**, **13,134 lines**.
- Snapshot SHA-256: `3ebf6873e2546e4d53616b68de7f241df4546e847d8e97170768b51b65f36ba2`.
- First 65,536 bytes SHA-256: `fd33e601d37d0da73534683e2aa485391d8837328286e44ccd13d3e219e20126`.
- Startup run 1 starts at line 6218 (`process_main`).
- Captured expected runtime ELF SHA-256 and current `bld/TRUEOS.elf` agree:
  `de26736cccc0b34da13ba81d7ac8e479608834e030cd7fe96e4012421ffc261c`.

The file is still growing and rotates. These hashes identify this observation,
not every future read of that symlink. The operator identifies the Codex package
as `df598646`; this audit did not independently recover its full package digest
from the captured log. Only allowlisted telemetry, timings, numeric scheduling
identifiers and fixed diagnostic classifications were extracted. No raw log
snapshot, prompts, account identifiers, credentials or tool payloads accompany
this report.

Startup markers and capture offsets use monotonic milliseconds. OTel wall
timestamps have one-second granularity. Summary maxima below are measured
observations within reporting windows; they cannot be added to construct an
exact critical path. A summary emitted after a boundary may include an earlier
observation. Empty or missing terminal events remain unknown.

## Measured timeline

| Boundary | Measurement | Evidence |
| --- | ---: | --- |
| First flushed frame / editable composer | 159 ms after process start | L6378 / L6382 |
| State database initialization | 1.860 s | L6432–6733 |
| App-server initialization | 1.548 s | L6739–6817 |
| Account status complete | 4.563 s after process start | L6871 |
| Account complete to thread start | 2.113 s | L6871–6981 |
| Thread start begin to done | 3.039 s | L6981–8279 |
| Server thread total / create-thread subset | 2.897 s / 1.428 s | L8249 / L7620 |
| Session prompt-ready marker | 60.203 s after process start | L9435 |
| Initial step-context capture | 38.672 s | capture 1, L9304 |
| Initial MCP-ready wait | offset 3–32,959 ms; duration 32.956 s | L9278 |
| Initial router build | offset 32,968–38,665 ms; duration 5.697 s | L9298 |
| Websocket connect | 1.392 s | L9094, 10:39:19 UTC |
| User prompt telemetry | 10:39:21 UTC | L9128, submission span 635 |
| Warmup websocket send | 243 ms; 10:40:16 UTC | L9538, explicitly `warmup=true` |
| Websocket warmup phase | 28.329 s | L9649 |
| Entire startup prewarm | 67.861 s, ready at 10:40:25 UTC | L9651 |
| Turn resolves already-ready prewarm | 1 ms, at 10:44:29 UTC | L11457, turn span 647 |
| Turn step-context capture | 9.318 s, ends 10:44:39 UTC | capture 2, L11525 |
| Turn instructions / router build | 9.287 s / 6.334 s, overlapping | L11519 / L11488 |
| Turn MCP-ready wait | 135 ms | L11468 |
| Completed real send / real TTFT | **Not observed** | no canonical real request, turn TTFT or real SSE TTFT |

The initial `codex_apps` MCP `tools/list` operation reports a **30-second timeout**
at L9276, matching the 32.956-second MCP-ready phase. Further timeout/retry
diagnostics occur at L10223–10224. These identify connector discovery delay;
they are not model inference latency. The code-mode/model compatibility warning
also appears, but its existence does not measure its contribution to the stall.

There is approximately **244 seconds between prewarm ready and turn prewarm
resolution**, and approximately **308 seconds between prompt telemetry and that
resolution**. These intervals are unassigned. Prompt telemetry proves a
submission has entered the session path; it does not by itself identify which
queue, lock, task, protected interaction or request-construction stage accounts
for the delay. The 2.113-second pre-thread interval likewise has no measured
modal-open/modal-close classification in this capture; it must not be labeled
CPU time or user delay without another event.

## Local responsiveness evidence

All listed Codex observations are on **carrier 19**:

| Log line | Site | Count | Mean | Maximum |
| --- | --- | ---: | ---: | ---: |
| 9441 | app stdin queue age | 1 | 23,882 ms | 23,882 ms |
| 10897 | Codex run slice | 6 | 3,355 ms | 17,801 ms |
| 10898 | Codex ready-to-poll | 6 | 6,418 ms | 17,802 ms |
| 10945 | Codex ready-to-poll | 70 | 1,206 ms | 17,948 ms |
| 10982 | app stdin queue age | 2 | 10,876 ms | 20,968 ms |
| 11819 | Codex run slice | 4 | 5,885 ms | 18,576 ms |
| 11820 | Codex ready-to-poll | 4 | 11,580 ms | 29,367 ms |
| 11886 | Codex ready-to-poll | 509 | 140 ms | 29,542 ms |
| 11914 | app stdin queue age | 3 | 6,881 ms | 19,740 ms |

`app-stdin-queue-age` measures elapsed time from queued app-console input to
consumption in `blueprint_console_read`; it is independent of model response
time. `ready-to-poll` measures earliest outstanding wake to the next carrier
poll. `run-slice` measures guest context entry through return and host context
restoration. It is elapsed occupation of that continuation interval, not a CPU
utilization counter. `wait-elapsed` can include legitimate I/O or condition
waiting and is not an input-latency metric.

The passive job roster identifies different running continuations during stalls:

- At L10890–10894, AP sample 48: VM 0, generation 2, main job ID **1** is
  `guest-enter`, `RUNNING`, stage age **16,869 ms**, sequence 55,144. Tokio worker
  IDs 2,147,483,650 and 2,147,483,651 are at `poll-return`/`YIELDED`, with stage
  ages 16,879 and 16,870 ms; ID 2,147,483,652 is `WAITING`.
- At L11809–11811, sample 75: worker ID **2,147,483,650** is `guest-enter`,
  `RUNNING`, stage age **14,550 ms**, sequence 64,792. Main job ID 1 is
  `poll-return`/`WAITING`, stage age 14,550 ms.

These observations support carrier monopolization by a long guest continuation
while sibling continuations stop progressing. They do not identify whether the
running code computes, spins on a lock, performs a synchronous filesystem wait,
or enters another blocking library routine. Stage-entry `if=0` is a sampled
boundary value, not proof that interrupts stayed disabled throughout the slice.
No futpoll profiler, guest stack backtrace or sampled guest RIP records were
present in the snapshot.

Nearby heap callers were checked against the exact matching full ELF at
`bld/artifacts/debug-cdb280bc9a0e/TRUEOS.full.elf`. The 2 MiB allocation at L10942
resolves through thread-stack mapping and `threads::submit_guest_from_raw`.
Callers at L11823 resolve to USB endpoint allocation (`NormalTd::clone`,
`Endpoint::poll_request`). Most nearby guest heap caller fields are zero. These
are neighboring allocations, not backtraces of the stalled continuation; they
provide no basis for blaming TLS, crypto, allocation, or a particular FS call.

## Next measurements and fixes

1. Capture bounded passive samples of the running continuation's guest RIP and
   stack, with VM/generation/job ID, carrier, monotonic time and last syscall
   class. Obtain exact package symbols before attributing a hot path. Sampling
   must avoid guest-owned locks and must omit arguments and buffers.
2. Time libc filesystem/pthread boundaries separately: entry, owner-queue
   enqueue, worker begin/end, wake, and caller resume. Record numeric correlation
   IDs and fixed operation/error classes. A synchronous wait must park only its
   continuation and let sibling TUI/network continuations run on the carrier.
3. Add fixed-name turn checkpoints between prompt acceptance, dispatch, context
   capture, request construction, connection acquisition, send begin/end, and
   first response event. Current completion-only request telemetry cannot rule
   out an unfinished request send. Preserve the distinction between whole-turn
   and per-response TTFT.
4. Time TUI queue consume, event dispatch and first flushed frame after input,
   with numeric input-batch IDs. Add protected-interaction shown/resolved events
   so user-await intervals can be separated from event-loop starvation.
5. Diagnose the `codex_apps` MCP discovery timeout as a separate connector
   problem; validate its transport and retry lifecycle before changing timeout
   policy. Once discovery fails, avoid making unrelated rendering or input
   service await it. Router and instructions timings need their own nested
   fixed-phase measurements to distinguish work from waiting.
6. Align code-mode configuration with the selected model's advertised support.
   Treat this compatibility correction independently from the proven local
   scheduling stalls; the warning alone does not establish their cause.

No production files, rig inputs or runtime configuration were changed for this
audit.

## Follow-up: bounded exit-callstack diagnostics

The subsequent scheduler instrumentation emits `codex-slice-exit` for a Codex
continuation that runs for more than 100 ms before yielding, sleeping, waiting,
or completing. Both input timing and Codex progress diagnostics must be enabled.
The record identifies VM, thread ID, carrier, monotonic end time, elapsed slice,
exit reason, up to 12 executable return addresses and a fixed walk-stop reason.
It is limited to one event per second per thread, measured at the parent-side
emission boundary.

The snapshot is taken on the still-running child stack, immediately before its
context switch. Every frame read is checked against that Thread's actual mapped
stack bounds, current RSP, word alignment, two-word read size and monotonically
increasing frame chain. Only addresses within the precomputed kernel, Hull or
Blueprint executable ranges are retained. Capture has no allocation, logging
or lock acquisition; formatting happens after restoring the parent's address
space, TLS/WLS/allocation domains and execution context.

This is an **exit call stack**, not an interrupted-PC sample or CPU attribution.
It identifies the call path that finally relinquished a long continuation; it
cannot identify where that continuation spent most of its time, and a
continuation that never returns to a boundary produces no exit record.
Executable ranges are initialized on the parent at the first poll. Missing
frame pointers or a range mismatch stop the walk explicitly; no fallback scans
arbitrary stack words. Executor placement and wake/wait semantics are unchanged.

The production scheduler host harness passed **47 tests**, including valid,
corrupt, cyclic, unaligned, overflowing and out-of-bounds frame chains; bounded
depth; non-executable payload rejection; per-slice duration/rate limiting; and a
real switched-stack case verifying nonempty child frames and parent-side log
formatting after context restoration. Native runtime evidence remains pending
the next kernel build and run.

## Follow-up: leased live Blueprint WLS identities

The WLS audit confirmed that opaque pthread IDs had been treated as array
indexes: each VM/realm had 20 slots, and IDs 20, 21 and all later sequence
numbers clamped to the same final slot. Pthread IDs are monotonically allocated
31-bit sequences with a carrier-realm tag; the 64-entry pthread join registry
and 256-thread scheduler admission limit do not prevent this collision.

WLS now leases a free live slot for the full opaque ID at scheduler admission.
Each VM and Hull/carrier realm retains its own 20-slot capacity. Concurrent
admission of a duplicate ID, exhaustion or an invalid identity fails before
entry execution with `EAGAIN`; there is no clamping or identity fallback. Lookup
uses the published atomic ID table and remains stable while the Thread owns its
lease. The lease retires after the stack/address space, independently of pthread
join-table removal, detach, completion notification or suspension. Admission
failure drops provisional leases. A retired slot can serve arbitrarily many
later distinct IDs; simultaneously live slots remain separate.

This is an exported WLS identity-contract repair. It is **not evidence that the
current QuickJS/std TLS aliases after 20 creations**: the installed TRUEOS std
selector uses the keyed `os` TLS backend, and Tokio delegates its thread-local
macro to `std::thread_local!`. The older per-WLS-slot `no_threads` implementation
is not selected for TRUEOS in the current toolchain. No generic reset of legacy
per-slot TLS storage was introduced by this repair; its consumers still need
their own lifetime/reset contract.

The scheduler harness now uses the production WLS module rather than a WLS mock.
All **52 scheduler/WLS tests** and **13 pthread semantics tests** pass. Coverage
includes IDs above 20; distinct VM/realm tables; 20 simultaneous leases and
explicit exhaustion; duplicate admission races; 2,000 sequential lease admissions
while retaining a live peer; slot recycling; and a real switched continuation
whose lease survives suspension and completion until task retirement. Native
integration remains part of the next kernel build/run.

## Reading the new diagnostics

`tools/testpy/codex_phase_report.py` now accepts the exact fixed diagnostic
schemas and reports them alongside canonical OTel events. JSON remains the
default; `--format markdown` includes the same sanitized summaries. Trace-safe
OTel mirrors are ignored. Unknown phases/methods/states and arbitrary fields
are excluded. Prompt/tool/SQL payload text cannot supply span context.

| Record family | Report fields | What it establishes |
| --- | --- | --- |
| `codex.mcp_phase` | `mcp_captures` | Separate operation/HTTP phase spans, first body/event progress, bytes, HTTP status and deadline |
| `sqlx_sqlite::remote_timing` | `sqlx_setup`, `sqlx_rpc`, `sqlx_connections` | Setup completion and actor-queue versus serialize/write/header/body/parse measurements |
| `codex-slice-exit` | `slice_exits`, `slice_summaries` | Which continuation and exit call path followed an elapsed slice over 100 ms |

MCP HTTP and operation sequences are independent; they are not automatically
joined or added. A started-only phase has null duration and end, not zero.
An operation error/cancellation does not identify network versus scheduling
delay until its HTTP phase measurements are also available. Connection and
sequence identifiers are process-local. SQLx records contain neither an
authoritative Codex session ID nor a VM ID, so mixed-process captures need
additional lifecycle evidence before treating matching connection IDs as a
cross-process join. Its queue/wire maxima are separate observations, not one
combined critical path. Fast RPC timing records are intentionally suppressed
by the emitter, so their absence is not evidence of a missing request.

Slice return addresses are limited to 12 numeric entries. Malformed or
overlong frame arrays become unknown; private frame text is never emitted.
An exit stack shows the call path that gave control back. It cannot establish
that its last filesystem, lock, allocation or timer call consumed the whole
preceding slice, and it cannot diagnose a continuation that never exits.

The parser's 13 synthetic regressions cover each new schema, rejected private
fields/unknown enums, incomplete phases, optional RPC durations, ignored
trace-safe mirrors, bounded frames and payload impersonation of span IDs.
These are parser checks, not new native performance results. The startup,
input-queue and long-slice measurements earlier in this report remain the
recorded findings; a speedup or a specific CPU/blocking cause requires the
next native capture with matching kernel/package identities.
