# Codex responsiveness audit, 2026-10-15

Latest controlled native validation: release kernel9 passes the libc contract
probe in 33 ms and the actual Codex H2/TLS transport probe 10/10. The unchanged
Code Mode package recovered from kernel6's quiet-cell watchdog to successful
64-cell runs in all three runtime profiles on kernel8. That comparison includes
multiple repairs and does not isolate one cause. Kernel9's final Code Mode
preflight also passes all three 64-cell profiles, C/platform clock bracketing
and an independent std Condvar timeout. Authenticated physical end-to-end
responsiveness remains unmeasured. Artifact hashes and evidence paths are
recorded at the end of this report.

The earlier physical capture shows a local responsiveness problem in addition to MCP
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

## Separate HTTP transport finding during native probe setup

The isolated QEMU with runtime
`e6770ac3a5fc64419d9853f996eb7e96963af85a95a2b470dc071e27ae18dd8c`
showed a repeatable port-80 accept gap. A read-only test issued 20 fresh
`GET /dl/` requests, each with `Connection: close`, without retries. This route
intentionally returns a small HTTP 400 response (18 bytes), avoiding filesystem
mutation or a large directory body.

| Spacing after each request | HTTP responses received | Connection resets |
| --- | ---: | ---: |
| 0 ms | 7 / 20 | 13 / 20 (`ECONNRESET`, 104) |
| 250 ms | 20 / 20 | 0 / 20 |

Successful requests generally took about 20 ms; resets took about 10 ms. This
does not establish a zero-length POST-body defect. It reproduces without a
request body or filesystem write.

The generic `vhttp_srv::HttpServer` previously retained one listener handle
through `TcpEstablished` and reissued `OpenTcpListen` only when that accepted
connection emitted `Closed`. The adapter creates one smoltcp listening socket
per command; that socket becomes the accepted connection and is not
automatically replaced. Consequently, there is no listening socket while its
connection is served and closed, followed by the owner's event/command rearm
latency. The HTTP owner polls at 10 ms. The existing audio HTTP service already
replenishes its listener at `TcpEstablished`.

The framework repair reissues the listener when it becomes established, tracks
an outstanding listen command separately from the current listener, and keeps
older session EOF from duplicating or replacing a newer listener. Accepted
sessions are bounded (16 by default, configurable); reaching the declared
capacity pauses rearm until a session retires. Data-before-established event
ordering remains supported. Unknown handles cannot create unlimited sessions.
Network-reported listen failures are surfaced without an immediate retry loop,
and a later capacity-release event can replenish the listener.

Nine regressions execute the actual vnet types, HTTP parser and transport state
without a filesystem mock. They cover two simultaneous peers, twenty sequential
connections, partial send/close handling, stale EOF, session capacity, early
data, listener errors and request-size/malformed-header checks. The owner regression
compiles the actual stream-drain function and verifies that cancelling the active
stream at session capacity forwards its rearm command before returning. These host
checks pass. Root's fresh-kernel native rerun received all 20 immediate HTTP
responses with zero connection resets. This validates the listener repair for
that bounded burst; it is not a broad throughput measurement.

An existing owner limitation remains explicit: `http_trueosfs` discards the
return value of command submission. If its owner queue rejects the rearm command
before the network stack receives it, the framework gets no error event. A
future owner-feedback/retry change should handle that separately; this repair
does not conceal it with an unbounded generic retry loop. The HTTP finding is
independent of the earlier Codex carrier/input starvation measurements.


## Separate reused-VM Appstore investigation

Two retained QEMU serial snapshots show a successful initial pull and a later
catalog-send failure after VM reuse. This audit sent no VM requests. The current
snapshot read for this comparison was 121,702 bytes, SHA-256
`33753c32654c2854e6f65d58c5dd54c0339e55ba5ad831473c442f6df5088d2b`;
the first-boot capture was 123,262 bytes, SHA-256
`53840a6f47737b0d97d908047d81a6b70b116941a9559aea27fed27018653853`.
The mutable current log was subsequently replaced during ongoing validation;
line comparisons from earlier snapshots must not be merged into this run.

In these retained snapshots, the initial pull creates and establishes one
std-ABI/MIO TCP socket. No second socket-open marker appears before the later
catalog-send failure. This does not establish a TLS handshake failure: the next
useful boundary is resolver dispatch, blocking-job admission/first execution,
then getaddrinfo and DNS-broker submission/completion. DNS-broker phase records
were absent at the captured logging tier, so this boundary remains unmeasured.

Source inspection finds a fresh reqwest client/resolver per pull, owned DNS
broker requests with monotonic IDs, VM-owned socket cleanup, descriptor/TLS-table
reset, and explicit platform-wait retirement. One lifecycle hazard deserves a
focused test: the global MIO compatibility object retains its VNet bridge; a
VMX-backed bridge contains a session ID while Blueprint teardown retires that
session. Which backend the first caller selected is not recorded here. This is
a conditional source concern, not a confirmed explanation for the observed
failure. Record only numeric VM generation/session IDs and static backend/phase
labels when testing it; no hostname, URL or request content is needed.


## Native MCP boundary validation and pending Code Mode repetition

On fresh kernel 4, the actual Codex MCP client passed all nine HTTP/1 native
contract cases. One measured response transferred 330,447 body bytes in 172 ms,
decoded JSON in 2 ms, and completed the probe operation in 215 ms. These are
separate measured spans, not an additive critical-path decomposition. The MCP
agent validated fixture-side peer closure for all four relevant connections.
Root will attach the final artifact manifest after the remaining tests.

The quiet 64-cell Code Mode repetition has **not** passed. On kernel 5/runtime
`8082cc60`, its first five cases passed, then the 64-cell case remained pending.
The 60-second Tokio watchdog eventually reported failure; its initially absent
message does not establish a permanently stopped timer driver. A short coordinated
GDB snapshot after failure found no active continuation pointers and an empty
VM0 job roster, so the retired worker's parked wait/waker cannot be reconstructed
from that snapshot. No VM reset or diagnostic request was sent by this audit.

A separate concrete timer-loop overhead appears in the exact kernel ELF:
`r::time::poll` unconditionally copies 144,008 bytes when moving its fixed-capacity
9000-waker vector into by-value iteration, even with zero due entries. It also
reserves approximately 288 KiB of stack. Two sparse debugger observations landed
inside this copy. This warrants a bounded implementation review, but sparse
observations do not establish its CPU share or explain the failed Code Mode
repetition. Capture live job phase, wait object, timeout and wake generation
before retirement to investigate that failure.


The timer wake loop now uses `to_wake.drain(..)`, retaining FIFO wake order and
executing callbacks outside the timer queue lock. A host harness compiles the
actual full driver and verifies due-versus-future entries, reentrant scheduling,
and exact waker/drop ownership. Optimized LLVM IR from the same driver before
and after the one-line change shows the unconditional 144,008-byte memcpy
present before and absent after. This establishes removal of capacity-sized
copying; it does not establish a quiet64 fix. Native kernel 6 validation remains
with the controller.

`tools/testpy/gdb_codex_records.py` provides a bounded read-only `codex-records`
command for the next live stall, before retirement erases the roster. It reads
at most 32 host-owned records using a numeric whitelist and rejects busy rosters,
invalid pointers/lengths and invalid phase values. Five synthetic bounds/privacy
checks pass. The documented x86_64 offsets were checked against kernel 5's
`JobDiagnostic::new` assembly and must be revalidated if the record or compiler
layout changes; this is not a stable debug ABI. The command never dereferences
thread names, guest payloads or the reported wait object, and makes no inferior
calls. Attach/detach must remain coordinated with the VM owner and other debugger
users.


## C clock and pthread absolute-deadline domain repair

A concrete source mismatch was found independently of the native outcome:
`blueprint_shims::clock_gettime` sampled the local Embassy clock, while
`pthread_clock_nanos(CLOCK_MONOTONIC)` sampled the host platform clock. Hull
preparation zeroes its private BSS, so its Embassy/TSC origin initializes later
than the host origin. A direct C clock_gettime caller can therefore form an
absolute deadline in a different domain from the pthread comparator.

The installed std path must be distinguished carefully: Unix Condvar selects
CLOCK_MONOTONIC, but forms its deadline through `Timespec::now(Self::CLOCK)`
(`std/sys/pal/unix/sync/condvar.rs:70`). That helper's TRUEOS early branch
(`std/sys/pal/unix/time.rs:75`) already calls the host platform monotonic
function and returns before the generic libc clock_gettime fallback. The
installed std Condvar and std Instant therefore already share that clock.
The C ABI repair does not demonstrate a clock-domain cause of quiet64.

Both C clock IDs now use the same provider as pthread absolute-deadline
comparison. That provider samples platform monotonic nanoseconds once; in the
Hull this crosses VMCALL to host Chronos. CLOCK_REALTIME adds the boot UTC-second
base to the same monotonic sample, retaining subsecond precision. Neither path
consults the Hull-private Embassy epoch. This fixes the demonstrated source
contract mismatch, without asserting it caused every observed wait.

The actual C `clock_gettime` and production pthread wait implementation pass a
host contract test with synthetic local/host origin differences of 15.470 and
600.123 seconds, for both MONOTONIC and REALTIME. The C-formed deadline remains
short, times out, and returns with the mutex owned; invalid clock IDs/null output
are rejected. This checks direct C-formed deadlines against production pthread
logic; it does not execute the installed std Condvar code or reproduce the native
quiet-cell failure. All 15 pthread semantics checks pass. A native clock-bracketing
and Condvar probe is being staged separately; the existing quiet-cell package
remains unchanged for the controller's before/after kernel comparison. No
quiet64 success is claimed yet.

The bounded roster decoder was also verified against the exact kernel 6 ELF,
SHA-256 `c707df199aeae1efa9ce0ecd05632289e5539cc3447f409589d3eeb20ff91166`:
Arc allocation 192 bytes, Record payload 176 bytes, roster stride 32 bytes, and
all scalar initialization offsets remain as documented. No debugger attach was
performed during the VM restart.


Kernel 7's first build attempt failed on the environment integration's undefined
`TRUEOS_EFAULT` at `std_abi_shim.rs:767` (E0425), not on either clock change.
The environment owner was notified to correct that integration error. Until a
fresh kernel and the unchanged quiet-cell artifact complete their controlled
comparison, the failure mechanism remains unproven. In particular, evidence for
an independent std timer/runtime scheduling or parked-worker issue must not be
replaced by the now-corrected C-clock hypothesis.


## Latest physical capture and final scoped host checks

Read-only snapshot at 2026-10-15 12:24:20 UTC: `LatestOfThree.logs` resolved to
`trueos-baremetal.2.log`, 169,812 bytes and 1,096 lines. SHA-256:
`319efdb204e48f21e8af68f26d52c121df648ae244f85985afda1e7f474d40b8`;
first 65,536 bytes SHA-256:
`66ec929a1a8c1f013ef9278c627e5c533209714bf56f1c6de3741678bd66c70d`.
This is a different capture from the earlier audited Codex session. Its header
records expected runtime ELF digest
`16b1c9d9d43921c52fc1d1592251781393dea2378104e7a375d4cb24c006cede`;
that is an expected artifact identity, not independent proof of the executing
image. No full Codex package identity was recovered.

The latest capture contains zero recognized Codex startup markers, canonical
OTel events, context captures, exit call stacks, MCP phases, SQLx setup/RPC phases
or input/progress records. It therefore supplies no new Codex-stage latency
measurements and cannot validate the newer instrumentation or fixes on physical
hardware. No rig input, reset, deployment or other mutation was performed.

Scoped final-source host checks were run once and all passed:

| Harness | Passing tests |
| --- | ---: |
| Offline canonical timing parser/privacy whitelist | 13 |
| Bounded GDB roster decoder/privacy checks | 5 |
| Actual continuation scheduler, WLS leases, waits and exit-frame capture | 52 |
| Actual timer driver wake/retain/reentrant/drop contract | 1 |
| Actual HTTP framework and cancelled-stream owner contract | 9 |
| Total in this scoped verification run | 80 |

The timer harness additionally compiled optimized before/after LLVM IR and
confirmed the 144,008-byte capacity copy is absent after the drain change.
These host results verify the exercised contracts; they do not substitute for
native quiet64, transport, or physical responsiveness acceptance. The separate
15-test C clock/pthread semantics run is recorded above and is not double-counted
in this 80-test total.

The physical log continued growing during this audit. A later recheck read
172,001 bytes, SHA-256
`75790d0b74b5fb250b017bd25f24a8622fea1e940fbe564e366e8c716be52838`.
The earlier hash identifies the explicitly timestamped snapshot above, rather
than asserting that the collector stopped or that future reads are unchanged.


The latest expected physical digest was subsequently matched to one local
metadata-selected runtime artifact, without scanning/hash-reading every ELF:
`bld/artifacts/debug-9481bff15c3b/TRUEOS.elf`, 73,104,776 bytes, SHA-256
`16b1c9d9d43921c52fc1d1592251781393dea2378104e7a375d4cb24c006cede`.
Its BUILD_INFO records `build_mode=debug`, build ID `9481bff15c3b`, commit
`9481bff15c3bc634e7d3c1f6a932b69397e2fa7c`, timestamp
`2026-10-15T12:00:42Z`, and empty Cargo build flags. The checked-in dev profile
uses optimization level 1; release uses level 2. This establishes the expected
physical artifact as a debug-profile build for performance-comparison purposes;
stripping its runtime symbols does not change the compilation profile. It still
does not independently establish which image is currently executing.


## Bounded import audit of the newly built Codex package

Inspected `TRUEOS-Blueprints/dist/codex.bp`, SHA-256
`14c1c4fa9bdf1b10159916ea195f2ce3c2b6faf03d23b14cf4bf7320829226af`,
66,668,798 bytes. Its single tar/LZ4 member contains 293 undefined ELF imports.
No application configuration, runtime credentials or user content was inspected.

Compared import names with the initial audited Blueprint known/runtime handlers, Unix
compatibility resolver, and generated CABI/ring resolver tables. Forty-three
names initially had no literal provider registration in those tables. The general
unresolved-import fallback installs a per-name stub and returns integer zero on
invocation. Provider registration alone does not establish that its semantics
are complete; this is a bounded static inventory, not exhaustive API acceptance.
The highest-confidence original gaps had actual relocations in linked callers
(repairs and accepted native results follow below):

| Import without a real provider | Linked caller / impact if reached |
| --- | --- |
| `__ctype_b_loc` | `tree_sitter_bash_external_scanner_scan`; pointer-returning fallback produces null instead of a character table |
| `iswspace`, `iswalpha`, `iswdigit`, `iswalnum` | Bash scanner character classification has zero-result fallback rather than actual classification |
| `fstatat` | rustix `statat` assumes its uninitialized output struct is initialized after zero/success; fallback does not write it |
| `statvfs` | Codex doctor `available_space` likewise calls `assume_init` after zero/success without a written result |
| `mkdirat` | rustix `mkdirat` reports successful directory creation without an operation |
| `futimens` | std `File::set_times` reports success without changing timestamps |
| `getpwuid_r` | Default shell/home-directory lookup has a success-shaped fallback without the required output contract |
| `sigemptyset`, `sigaddset`, `pthread_sigmask`, `kill` | TUI suspend path cannot rely on proper signal-set initialization/masking or suspend delivery |

The Bash parser is called from `codex-shell-command::try_parse_shell` and
apply-patch invocation parsing. Its scanner's classification imports therefore
deserve a real parser contract test, beyond checking launch-time binding. The
`fstatat` and `statvfs` output contracts are especially concrete: false success
can expose uninitialized Rust values. Proper failure would be preferable to a
zero-returning Joker until the operation is implemented. This initial inventory preceded the bounded repairs recorded below.

Other unregistered imports include fortified stdio formatting, tree-sitter dot
output helpers, user/group/process metadata and filesystem operations. Some
linked functions are optional or diagnostic, so undefined-import presence does
not prove execution during the observed latency. The four `ZSTD_trace_*` hooks
also require weak-symbol/optional-call treatment rather than assuming they are
active missing functionality.

The main socket/DNS/read/write/close imports have named providers; the inventory
did not find a new missing main-network import. `getenv`, `setenv`, `unsetenv`
and `environ` now have real environment providers in the current source. Native
thread creation uses explicit TRUEOS CABI imports rather than the rejected
POSIX `pthread_create/join/detach` imports. Existing pthread synchronization
imports bind to providers, while the separate timed-wait/runtime investigation
remains open. No causal connection between these newly inventoried gaps and
quiet64 has been established.


### Bounded classification/output-contract repair after the inventory

Saved the complete allowlisted import inventory and all 43 original gaps in
`tools/docs/codex-abi-imports-2026-10-15.json`; it contains ABI names and static
source evidence, with no runtime payload. Six imports now have explicit
providers: `__ctype_b_loc`, `iswspace`, `iswalpha`, `iswdigit`, `iswalnum` and
`statvfs`. At that repair stage, thirty-seven names remained without a literal provider
registration. After the filesystem follow-up below, thirty-four remain; that
list is not a claim that all are executed.

Classification uses an explicit immutable **C locale**, independent of LANG.
The byte table covers indices -128 through 255, including EOF, with the biased
pointer-to-pointer layout and network-order flags required by local glibc
`ctype.h`. ASCII receives all twelve correct category bits; high bytes and
negative indices are unclassified in that locale. Wide classification has the
same C-locale ASCII categories. WEOF, U+0085, U+00A0, U+2003 and other non-ASCII
values are unclassified. No Unicode locale switch is silently invented.
The package does not import `__ctype_tolower_loc` or `__ctype_toupper_loc`, so
those APIs were not added speculatively.

There is no compatible namespace/mount-capacity query behind the POSIX
`statvfs` output structure yet. Its provider therefore returns -1 and sets
ENOSYS, leaving output untouched. It does not return zero without initializing
the structure or fabricate available capacity.

Four new host contract tests pass. They compare every one of the 384 byte-table
entries and every Unicode scalar-range input to the four wide classifiers
against local libc in C locale, including WEOF. They compile the actual
production providers with real tree-sitter 0.25.10/Bash 0.25.1 C parser/scanner
sources and parse words, assignments, loops and a heredoc. They check stable
accessor identity across eight threads, plus statvfs failure/errno and unchanged
output canaries. This validates actual classification and parsing behavior,
not merely resolver presence. A separate native libc contract probe belongs to
the controller; native acceptance and any relationship to Codex latency remain
pending. These four tests are additional to the earlier pinned 80-test run.


## Accepted native results on release kernel 9

The controller accepted a **release-profile** kernel with these identities:

| Artifact | SHA-256 |
| --- | --- |
| ISO | `ec072679aa88e190f2f4a7d36ab7bc2b65e3086aef8edd3b8a9fd16b01e97fef` |
| Runtime ELF | `99cf34f8f98e7b031cdf5577eb85c71bc98b5914235e77ea15acfc4ec0357bd7` |
| Full symbol ELF | `cf13ed84cdc7781774ecfe225caae72944ab9f7bd269f21acdbde9a4e59204b0` |

This is distinct from the debug-profile expected physical artifact identified
above. Controlled release-QEMU timings must not be presented as a measured
physical debug-to-release speedup.

The native `libc_contract` package `1c110129` passed in **33 ms**, including
classification, relative filesystem operations, initialized metadata/error-output
contracts, and honest unsupported-capacity/timestamp results. Evidence:
`TRUEOS-Blueprints/probes/libc_contract/validation/native-kernel9.json` and
`native-kernel9.log`. Probe-owned filesystem cleanup was checked before success.

The filesystem follow-up adds `fstatat`, `fstatat64`, `mkdirat` and `futimens`
providers. Three of those names occur in the audited 293-import package;
`fstatat64` is an additional compatible export. Together with classification and
statvfs, **nine of the original 43 provider gaps** now have explicit handlers;
**34 remain unregistered** in the saved inventory. `fstatat` initializes metadata
on success and rejects unsupported flags. Directory descriptors retain path
anchors, not inode handles with rename-stable identity. `mkdirat` checks existing
target and immediate-parent semantics, but its broker-backed create is not an
atomic exclusive operation against concurrent namespace writers. Capacity
reporting remains unsupported (`statvfs`: -1/ENOSYS, output untouched). Writable
POSIX timestamps remain unsupported (`futimens`: valid FD ENOSYS, invalid FD
EBADF); logical filesystem revisions are not fabricated wall-clock timestamps.

The unchanged Code Mode package `a6eb684a` provides a controlled kernel
comparison: kernel 6's current-thread quiet case stopped after three completed
cells and reached its watchdog, whereas kernel 8 passed all three runtime
profiles. Each successful profile completed **64 JavaScript cells, 128 delegated
timers and 64 closures**:

| Runtime profile | Quiet 64-cell elapsed | Maximum cell |
| --- | ---: | ---: |
| multi-thread, one worker | 622 ms | 15 ms |
| multi-thread, four workers | 784 ms | 18 ms |
| current-thread, time-only | 683 ms | 19 ms |

Evidence is in `TRUEOS-Blueprints/probes/code_mode_probe/validation/`:
`native-kernel6-controls.{json,log}` and `native-kernel8-controls.{json,log}`.
Kernel 8 combines timer-waker deduplication, the C clock repair and eager
process-environment initialization. Recovery of the unchanged package establishes
a successful combined-kernel comparison, **not a sole-cause attribution** to
one change. The final kernel9 Code Mode preflight is a separate completed run,
recorded below; the earlier kernel8 timings remain attached to their own run.

On accepted kernel9, the actual Codex RMCP/RouteAware transport probe `9f12bab4`
passed **10/10** cases, with 20 actual H2 requests and ALPN `h2`. The trusted CA
path verified TLS; a wrong-root fixture produced `UnknownIssuer` and **zero HTTP
requests**. Tools/list returned 106 tools in **238 ms** for JSON and **209 ms**
for SSE. Header/body deadlines completed in **302/303 ms**, cancellation in
**154 ms**, and recovery in 220 ms. Fixture peer-cleanup checks and native process
teardown completed cleanly; the controlled fixture remains available for follow-up.
Environment readback and a native worker's environment view also passed.
Evidence: `TRUEOS-Blueprints/probes/mcp_http_probe/validation/`
`native-cert-h2-kernel9.json` (and the associated sanitized transcript).

These are controlled synthetic transport/provider measurements on the native
runtime, without production authorization or full Codex contention. They prove
the exercised H2/TLS/deadline/cancellation and provider contracts, not a full
physical user-turn latency result. The rebuilt Codex package remains identical
at `14c1c4fa9bdf1b10159916ea195f2ce3c2b6faf03d23b14cf4bf7320829226af`.
It was published to the user-specified Apps destination and its 66,668,798 remote
bytes were read back and verified against that complete SHA-256 digest.


### Completed kernel9 Code Mode preflight

Package `4358aa759172a022400dd76defba589fdb007d784fffac91281b46c9c13be2bd`
passed all cases on the accepted kernel9 ISO/full ELF identified above. The
native C CLOCK_MONOTONIC reading fell within the platform clock bracket; the
three observations were all 7,355,000,000 ns. An independent std Condvar 2 ms
wait returned `timed_out=true` after **7 ms**. This directly validates both
clock interfaces and a std timeout in this controlled native run, without
claiming that the C clock repair alone explains the earlier failure.

Each runtime completed 64 quiet JavaScript cells, 128 delegated calls and 64
closures:

| Runtime profile | Quiet 64-cell elapsed | Maximum cell |
| --- | ---: | ---: |
| multi-thread, one worker | 541 ms | 12 ms |
| multi-thread, four workers | 765 ms | 51 ms |
| current-thread, time-only | 703 ms | 33 ms |

Evidence: `TRUEOS-Blueprints/probes/code_mode_probe/validation/`
`native-kernel9-preflight.json` and `native-kernel9-preflight.log`. This uses
the production provider with a controlled delegate/KernelBackend, without a
model request or production authentication. Native `gri` was admitted and still
running at the observation deadline; UI4 visibility was **not proven**. Neither
this result nor the transport probe measures authenticated physical full-Codex
turn latency. The published Codex header will show `14c1c4fa`. The ABI repairs
require the updated kernel; the accepted release image is available at
`bld/codex-native-runtime-accepted/trueos.iso` and its boot bundle at
`bld/codex-native-runtime-accepted/TrueOS.lz4`. Neither the physical rig nor its
boot image was reset or replaced by these controlled runs. Both owned test VMs
were stopped after evidence capture; unrelated VMs and the physical rig were
left running.
