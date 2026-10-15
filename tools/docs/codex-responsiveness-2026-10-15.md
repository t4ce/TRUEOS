# Codex responsiveness audit, 2026-10-15

Latest physical follow-up distinguishes two failures. The retained automatic
turn has a native continuation still running after 370.768 seconds, with
same-carrier siblings not advancing. The next boot exits during terminal
initialization at 89 ms: lease claim status -5, epoch 0. No completed real model
send is observed in the stalled capture; existing completion-only telemetry
cannot exclude an unfinished send. Controlled native probe passes do not
establish full Codex or physical responsiveness. Pinned capture identities and
clock limitations are recorded below.

Latest controlled native validation: the first allocator/diagnostic repair
kernel (`da52b6df…`) passes the libc contract in 33 ms, the actual Codex H2/TLS
transport probe 10/10, all three 64-cell Code Mode runtime profiles, C/platform
clock bracketing and an independent std Condvar timeout. These runs include
multiple repairs and do not isolate one cause. Authenticated physical
end-to-end responsiveness remains unmeasured. Artifact hashes and evidence
paths are recorded at the end of this report.

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


## Physical automatic-turn follow-up, 13:26 UTC

This read-only audit compared retained captures without sending rig input. Raw
logs, prompts, account identifiers and payloads were not copied into the report.
The SHA-256 identities below pin observations, not subsequent rotating reads.

| Snapshot | Bytes / lines | SHA-256 | Codex telemetry |
| --- | --- | --- | --- |
| `.0.log`, 13:26:17 UTC | 93,478 / 552 | `6f4804352519f5e5ea4f98af016626134ea3bcc47b098a51a1170948a4c5bead` | none recognized |
| `.1.log`, 13:26:17 UTC | 84,758 / 488 | `9a40d77de885c2174ef6c967213e7998c9962d13cd066b455d044deea4b14759` | none recognized |
| `.2.log`, 13:26:17 UTC | 3,165,434 / 27,783 | `129343afb7fc555492dd483099d0a8aa4a81495b2ad3d3c2cff52f2da91afe79` | 55 startup markers, 86 canonical events, 2 captures, 8 MCP groups, 46 SQLx setup records, 116 SQLx RPC records, 182 exit snapshots |

The expected runtime digest in `.0` and `.1` is
`0524416a7f5fdee75a9a2466dde5c46218ebe48cb4627ac300c2d68cd06cdd17`.
It matches the local `debug-9481bff15c3b/TRUEOS.elf`; BUILD_INFO records debug,
empty Cargo flags, timestamp 12:53:14 UTC, and commit
`9481bff15c3bc634e7d3c1f6a932b69397e2fa7c`. The dev profile uses opt-level 1.
Their snapshots contain no Codex startup sequence, so they do not yet support a
second-turn comparison.

The stalled `.2` capture expects runtime
`b110076314dd99dd8e78453bf4e13567d5653d565f3e855eda84382fe3e66c8a`.
A matching preserved build manifest/ELF was not established in this audit;
reused artifact paths had been overwritten. Its build profile therefore remains
unverified here. It must not be equated with the accepted release kernel9
`99cf34f8…` or silently treated as the same binary as either later boot.
The operator identifies the application as package `14c1c4fa`; its full digest
was established for the built package above, but no full package digest was
independently recovered from this capture.

### Measured progress before the stall

One canonical startup run begins at line 9860, with the editable composer visible
at **174 ms** (line10023). State DB initialization takes **2.079 s**; app-server
initialization **1.561 s**; account status **1.496 s**. Thread start completes
at **8.515 s**, after **2.470 s** in that span. Prompt-ready appears at
**32.954 s** (line11930).

Prewarm context capture1 takes **11.154 s**: MCP-ready spans **6.968 s** and
router construction **4.183 s**. The actual large tools/list request transfers
**599,042 bytes**, with HTTP headers **269 ms**, body **602 ms**, JSON decode
**4.377 s**, and whole-operation **5.361 s**. It succeeds; no MCP timeout is
needed to explain this startup. Decode elapsed is wall time and may include
scheduler interruptions, not exclusively JSON CPU.

Canonical wall timestamps place the user-prompt event at **13:06:47**
(line11617), completed WebSocket warmup at **13:06:54** (line11767), turn-side
prewarm resolution at **13:08:54** (line14159), and context capture2 completion
at **13:09:06** (line14344). Thus **127 seconds** separate prompt and turn-side
resolve, including a **120-second gap after warmup readiness**. The resolve
operation itself reports only 1 ms. Capture2 takes **11.580 s**: instructions
**11.563 s**, MCP-ready **3.081 s**, router **8.140 s**, with overlap. These
spans are not summed into an additive critical path.

The only WebSocket request is classified as **warmup**, successful in 171 ms.
There is no canonical real turn request or response TTFT in this snapshot.
Warmup response completion must not be presented as the automatic turn's answer.
The last canonical semantic completion is context capture2 at 13:09:06. Later
scheduler and SQLx records show partial local activity without a completed real
model-send record. That completion-only boundary cannot exclude a send that
has entered and not returned.
Wall timestamps have one-second resolution, and host monotonic records use a
separate clock; this report does not force subsecond joins between them.

### Carrier and queue evidence

All long Codex continuations below use VM1, generation2, carrier19, worker
2147483650. Measured host-monotonic intervals from completed exit records:

| Log line | Begin / end (ms) | Slice duration | Exit reason |
| --- | --- | ---: | --- |
| 19177 | 985235 / 1123154 | 137,919 ms | Wait |
| 20525 | 1124176 / 1227206 | 103,030 ms | Yield |
| 20875 | 1228628 / 1253330 | 24,702 ms | Yield |
| 21133 | 1253330 / 1274219 | 20,889 ms | Yield |
| 23052 | 1275086 / 1420397 | 145,311 ms | Yield |

These are **exit-callstack** records, not sampled current RIPs or direct CPU
attribution. At the final bounded progress observation (line27731), this worker
is RUNNING/guest-enter, sequence105346 unchanged, age **370,768 ms**. No exit
snapshot exists for that in-progress interval. Sibling2147483652 remains
YIELDED/poll-return with the same age. Main continuation1 remains
WAITING/poll-return, sequence82776 unchanged, age **516,458 ms** (line27732).
The independent progress observer continues reporting; the same-carrier
continuations do not all advance. This is a local continuation-starvation
symptom, while the executing call path still requires exact symbolization.

Aggregated carrier19 records reach **145,341 ms ready-to-poll latency**
(line23054), **145,311 ms run slice** (line23053), and **165,083 ms wait elapsed**
(line20532). Those measures overlap the continuation intervals and cannot be
added. Other front-end paths are materially smaller in these observations:
application stdin queue maximum9 ms, SSH output queue33 ms, SSH routing-pass1 ms,
and shell-service-pass534 ms. The long carrier-ready latency is not measured
network round-trip latency or evidence of late input delivery at those queues.

SQLx reports a completed RPC after **241,976 ms queued**, then **1,329 ms wire**
(line20569, connection2/request210). Another request reports **242,004 ms queued**
at a write-stage record (line20528, connection10/request209); its missing successful
completion must not become zero duration. Completed RPC wire elapsed reaches
**18,918 ms**, of which header wait is **18,916 ms** (line14473). Across observed
completed summaries, serialization is at most5 ms and parse at most30 ms;
these are not evidence that parsing accounts for the multi-minute queue delay.
SQLx records lack authoritative VM identity, so connection IDs are not used to
prove an exact thread/session join.

The capture therefore supplies a concrete local progress boundary and long
unassigned intervals, but does not yet establish whether the worker is spinning,
blocked in a compatibility call, or consuming CPU in ordinary guest code.
Exact kernel/REL symbolization is assigned separately. No production patch or
rig mutation was made during this follow-up.


The independent symbolization audit subsequently resolved the packed Codex REL
addresses for the same `14c1c4fa` package. Repeated Yield exits return through
Tokio multi-thread worker's explicit `std::thread::yield_now` after its 10 ms
carrier-turn elapsed check (`worker.rs:580–581`). Another observed return site
is `AsyncFsOperation::ready` after `trueos_cabi_poll_once`, and one is std
DirBuilder after libc mkdir. Those are where a completed continuation yielded
or waited; they still do not identify where its preceding 138/103/145 seconds
were spent. A periodic outer-loop yield check does not preempt a long task poll.
Exact kernel ELF attribution remained unresolved at this report update.


## Next-build passive ABI breadcrumbs

New bounded per-job breadcrumbs address the missing observation for a
continuation that has not returned. The existing passive progress roster now
reports a static current operation (`none`, allocator operations, mutex, condvar,
stat, mkdir, wait, sleep or yield), entry monotonic timestamp/age, nesting depth,
entry/exit counters and last completed operation. `none` distinguishes guest
execution outside these selected instrumented boundaries. It does not prove
ordinary CPU work or absence of another uninstrumented compatibility operation.

Capture uses fixed atomics embedded in the host-owned job record. Entry/exit
perform no allocation, locking, formatting or logging. A sequence check rejects
changing observations; a same-carrier interrupt arriving during publication
skips that observation without spinning. Nested waits remain visible and restore
the original outer entry timestamp on return. Guards stay on their pinned
continuation stack and are explicitly unsafe to let escape the owner's lifetime;
forced cancellation discards that stack before releasing its record. No Arc is
cloned into a guard that could leak a retained record on stack discard.

Initial hooks cover pthread mutex/trylock and condition waits, stat/fstatat,
mkdir/mkdirat, plus native wait/sleep/yield. Allocator operation IDs are available
for the allocator owner's actual-path hooks. No resolved pthread rwlock boundary
was present in the audited source/package, so no speculative hook was added.
Instrumentation is inactive outside a timed Codex continuation with progress
probing enabled. Hull-only code has no native Thread record and remains outside
this coverage. This is a diagnostic boundary observation, not PC sampling or a
claim about which operation caused the prior physical stall.

The final scheduler harness passes **59 tests**, including four new breadcrumb
contracts: passive visibility before return, nested restoration, rejection of
incomplete/reentrant publication, and a real suspended child stack surviving
until forced cancellation/record retirement without carrier inheritance. The
POSIX-at harness passes **5 tests** with instrumentation inactive in its host
fixture. Record layout changed: the earlier kernel5/6 GDB decoder offsets are
not valid for this build. Pin the exact new full ELF and revalidate offsets
before any debugger-based decoding. Native observations of these new fields
remain pending deployment by the controller.


## Second-boot follow-up: early terminal-lease exit, not the prior stall

At 13:48:14 UTC, the rotating latest capture resolved to `.0.log`, 1,260,029
bytes / 9,183 lines, SHA-256
`9af29e25d713827d45c8acd9a76c09651eb8bf693e825bce08437a59bd9f52a7`;
first 65,536 bytes SHA-256
`4bca300d330773d55415d2bc484017dc7b38f62c4d31ab60739504c444915eae`.
Its expected runtime is now
`6ef89b9b5164efd7ebfab9b1165661e14a38f7382517b953944c259c70fbe994`,
not the earlier `.0` snapshot's `0524416a…`. It matches the checked local
`debug-a8929a33e29e/TRUEOS.elf`; BUILD_INFO records debug/opt-level1,
commit `a8929a33e29e32af167f7a1d94cc0a88495e5a28`, timestamp13:25:36 UTC.
Comparisons must retain those separate boot identities.

Main native thread entry is observed at **47 ms** (line6222); runtime creation
completes at51 ms; presentation configuration at88 ms. Terminal checks pass at
89 ms. The last marker is `terminal_modes_begin` at89 ms (line6300), followed
by the fixed error class **failed to claim TRUEOS terminal lease**, numeric
**status=-5, epoch=0** (line6320). The source performs this explicit lease claim
first in `set_modes`, before ANSI/bracketed-paste, raw mode or keyboard setup.
The evidence therefore does not identify a raw-mode ioctl failure.

This run then **exits**. Its native job1 is COMPLETED on carrier19/generation2
(line6336). The final VM-exit diagnostic reports reason0x12 and guest RIP
0xFFFFFFFF80B01A29 (line6337), followed by clean_exit=1/preserve_exit=0
(line6345). VM0 goes offline with native_jobs=0 and seven wait queues retired
(line6348). That lifecycle clean-exit flag means teardown completed; it does
not mean TUI initialization succeeded. There is no event loop, first frame,
MCP phase or real model request to time in this attempt.

Zero exit snapshots/progress-job records are consistent with this short-lived
startup: the exit-trace threshold is100 ms and passive roster observations have
a multi-second cadence. Their absence alone does not establish failed Codex
classification. The earlier stalled run used VM1, whereas this early exit uses
VM0/generation2; terminal lease routing/context requires the lifecycle owner's
source audit.

The current local `dist/codex.bp` hashes to
`e5ba8f6ec5620424be7394c98a93260721897f73def756f5dd8dd9ad833cf660`,
which differs from14c1c4fa. This is only a local build-artifact observation.
There is no full application digest or TRUEOS_APP_SHA256 metadata in the pinned
capture, so the launched package cannot be independently identified from it.
No raw terminal/error payload was printed and no rig input, reset or source
patch was performed during this diagnosis.

## Repair: admit allocator diagnostics before scanning the heap

The native-carrier allocation path called `hv_guest_heap_stats` before deciding
whether a successful allocation warranted a watermark log. That operation walks
the entire guest free list under its heap mutex. The subsequent log-bucket
filter suppressed output but did not suppress this work. The retained physical
capture reports **46,920 free blocks** at line25755, with 503,319,776 free bytes
and a 454,506,320-byte largest block. This establishes expensive repeated work
in a path used by Codex; it does not measure its total contribution to the
unreturned continuation.

The repair puts an atomic per-VM admission check before the scan and host-domain
switch. Ordinary successful allocations take at most one diagnostic sample per
second. Allocations of at least1 MiB and allocation failures retain immediate
diagnostics. Allocation placement, free validation and free-list invariants are
unchanged. The gate neither allocates nor takes a lock.

`tools/testpy/test_allocator_watermark_cost.py` extracts the production handler
and heap statistics implementation. With 46,920 free nodes and 200 quiet
allocations over a simulated199 ms, both independently generated heap geometries
change from **200 to1 full scans**, **9,384,000 to46,920 node visits**, and
**200 to1 host-domain entries**. Four tests pass, including concurrent admission
and immediate large-allocation sampling. Host timing receipts are in
`bld/codex-physical-stall-20261015/allocator-watermark-cost.log`; they are not
physical-target speedup measurements.

Ordinary frees still validate against the whole free list before their insertion
walk. That separate linear cost is retained and remains a candidate for a
measured allocator improvement. Likewise, a SQLx RPC's30-second wire deadline
still starts after dequeue; the observed242-second queue wait falls outside
that deadline. Neither limitation is silently reported as repaired here.

## Added boundaries and controlled validation for the repair

Codex now emits fixed-name `started` boundaries before polling the next turn
future or performing synchronous prompt construction, followed by complete,
error or cancelled records. Coverage includes post-capture display roots,
skills/plugins, hooks, input recording, base instructions, incremental tools,
prompt construction, request metadata and entry to `client_session.stream`.
An unfinished stream entry can therefore be distinguished from a missing
completion record. Five core timing tests and14 safe-report parser tests pass;
payloads and request arguments are excluded.

The kernel adds passive ABI breadcrumbs described above, including actual
allocator entry points, and a fixed64-record async-filesystem tracker. The latter
distinguishes admission, service start/finish and first consumer observation,
with bounded static operation names, queue age and waiter counts. Observation
does not acquire a filesystem lock or copy paths/data. Tracker, lifecycle and
rename harnesses pass3,8 and13 tests respectively. These diagnostics identify
elapsed boundaries; they are not CPU profiles and do not themselves impose
new filesystem or queue deadlines.

The first repair build is isolated at
`bld/codex-stall-repair-20261015/`, with release flags and full symbols retained:

| Artifact | SHA-256 |
| --- | --- |
| `trueos.iso` | `168feecbd968568a9fbd8e732329dc2d48191d9d0327b57a18bdaf9a9f88ab27` |
| `artifacts/TRUEOS.elf` | `da52b6df7c6c587b1308addd9ac83724bd6a412036fde1169e59c9066042ca49` |
| `artifacts/TRUEOS.full.elf` | `6383c973a218a7d6344bc57db2eb3a6f1a8df7d881ab5ec9889dc08d5769cfab` |

This build passes the native libc contract in33 ms and the production MCP
HTTPS/H2 probe **10/10**:106 tools in203 ms via JSON and212 ms via SSE. The
fixture verifies20 actual H2/ALPN requests, certificate rejection, cancellation,
deadlines and four cleanup cases, without production authentication. No panic
or EPT fault was observed. Full identities, source hashes, test commands and
evidence hashes are in `native-h2-acceptance.json` in that build directory.
The owned H2 VM was stopped after capture; its synthetic HTTPS fixture remains
available. These passes do not establish that the physical full-Codex stall is
resolved.

On the same repair kernel, the unchanged `4358aa75` Code Mode probe passes the
C/platform clock bracket, an independent std Condvar timeout, and all three
runtime profiles. Each profile completes64 quiet JavaScript cells,128 delegated
calls and64 closures: one-worker572 ms (maximum14 ms), four-worker501 ms
(maximum14 ms), current-thread745 ms (maximum54 ms). Safe evidence is
`TRUEOS-Blueprints/probes/code_mode_probe/validation/native-stall-repair-controls.{json,log}`.
These figures exercise the production provider with synthetic delegates; they
do not measure a model turn.

The separate native `gri` request is admitted, but Gridpaper visibility remains
unproven. A later read-only audit found the Appstore helper actually exits after
a catalog HTTP send error, before catalog parsing, package download or Gridpaper
launch. The retained generic error drops its source chain, preventing attribution
to DNS, TLS, connection or response-header failure. The readiness timeout is a
downstream observer of that failed launch, rather than evidence that the helper
is still running in a queue.

The Codex package containing the new turn boundaries is published at the
authorized Apps share as `codex.bp§§e5ba8f6ec5620424be7394c98a93260721897f73def756f5dd8dd9ad833cf660`.
Readback from that share verifies the full SHA-256 and66,677,686-byte size.
Publishing the application does not replace the physical rig's kernel.

Appstore now records fixed send-stage evidence for the next launch: a static
endpoint category, elapsed milliseconds, HTTP status when available, reqwest
`is_timeout`/`is_connect`/`is_request` flags, and a bounded source-category chain.
Known I/O errors use their typed error kind; recognized TLS certificate variants
produce static classes such as `tls_unknown_issuer`. Error text, URLs, headers
and bodies are absent from these records. The existing 90-second request timeout
is unchanged. This diagnoses the catalog failure; it does not establish its cause
or repair networking.

The production classification/privacy harness passes three tests, including
nested private error text and cyclic sources. The real TRUEOS Appstore release
pack and C ABI guard pass with publishing explicitly disabled:

```text
python3 buildins/appstore/test_http_diagnostics.py
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp appstore
```

The resulting `dist/appstore.bp` is 2,009,924 bytes, SHA-256
`866d84bc49070c98067a5af3c1e9c65e49a91697be0222e387140292d3564a42`.
Appstore is embedded in the kernel's built-in AppDB seed, so this evidence becomes
available only in a kernel built with that updated package. No separate Appstore
publication or physical-rig action was performed for this change.


## Terminal-claim launch handoff repair and focused acceptance

The second-boot status -5 is specifically `BlueprintTerminalLeaseError::Busy`,
not POSIX EIO or a transport failure. Native execution context and VM0 are not
intrinsically unsupported: a subsequent VM0/generation3 launch successfully
claims epoch1 at line6605 in the same capture. That later application is not
classified as Codex; there is no second Codex process_main or canonical turn
telemetry, and its successful surface claim must not be presented as Codex
recovering from the earlier exit.

A focused actual-Shell3 regression reproduced one concrete launch race: an old
native helper remains active while accepted-word feedback defers navigation;
new Blueprint attachment and startup proceed, but the guest's one-shot terminal
claim sees the other active route and returns Busy. The new test failed before
repair while all 33 existing Shell3 tests passed. This proves that route
transition defect, but the original physical Busy record did not log its exact
predicate, so that specific branch is **not proven as the physical cause**.

The final synchronous admission order is now:

1. Attach the new Blueprint target, including resource registration.
2. Admit its executor task token.
3. Park previous **native helpers only** on the same frontend, clearing pending
   native input while retaining their screen/paint state for later resume.
4. Spawn the admitted task, which may then execute its first terminal claim.

Attachment and task-pool failures discard the new target without parking the old
native helper. Handoff commit is infallible and occurs before spawn, avoiding a
one-shot claim before preparation. Generic/native-to-native attachment keeps its
existing behavior. Other frontends are unchanged. Existing Blueprint owners,
VM generations and lease epochs retain their explicit release/exclusivity
checks; no competing-owner claim predicate was relaxed. The new surface can
buffer its startup frame while feedback owns presentation, and final selection
preserves that already-claimed owner rather than parking it again.

Failed claims additionally emit `terminal-claim-rejected` with numeric VM,
generation and frontend plus a fixed reason: dead-slot, not-selected, wrong-vm,
other-active-route, stale-owner or remote-begin; missing-run/missing-route are
also distinguished. Logging occurs after releasing ROUTES, with at most one
record per VM generation (unknown-run failures have a separate one-shot bound).
No target/slot name, path, terminal bytes or user payload is logged. This makes
a future generic -5 distinguishable without introducing a retry or timeout change.

Final focused host verification:

| Command | Passing cases |
| --- | ---: |
| `python3 tools/testpy/test_shell3_tui.py` | 37 |
| `python3 tools/testpy/test_hv_launcher.py` | 7 (4 task lifecycle + 3 actual enqueue admission) |

The tests exercise the real Shell3 terminal bridge and real enqueue body with
host platform fixtures. They cover the red/green native-helper race through
buffered paint and final input/owner selection, failed resource attachment,
already-attached failure, task-pool exhaustion, unrelated frontends, competing
Blueprint ownership, token drop-bomb safety, and attachment→token→park→spawn
ordering. The logging callback checks that ROUTES can be acquired when logging,
that repeats are bounded by run, and that private test slot names are absent.
These are host contract results. Native/full-Codex confirmation of the repaired
handoff is pending the controller's new kernel; no physical rig input or reset
was performed by this audit.

The final kernel (`dc0bfe30`, ISO `d2b8ce4e`) boots and executes the updated
Appstore, but its Code Mode probe is **NOT_RUN**, rather than failed: the first
catalog request fails before package selection, download or launch. The new
record reports171 ms, `is_timeout=0`, `is_connect=1`, `is_request=1`, source depth4,
`io_kind=io_other`, and no recognized TLS certificate class. MIO records a resolved
TCP connection attempt followed by **TCP closed before established**, placing the
failure after DNS resolution and before TLS. Appstore then exits cleanly.

An independent host comparison to the same public catalog resolves
`trueos.eu` to `188.193.106.78`; a direct Python socket gets
`ConnectionRefusedError`, errno111, in6 ms. Curl independently fails to connect
with no HTTP response or TLS establishment. This corroborates current public
endpoint unavailability. The native MIO adapter's synthesized pre-establishment
status passes through a generic errno mapping, so its `io_other` does not retain
the original peer failure reason; this is a separate diagnostic-fidelity gap,
not evidence of a TLS defect or justification for another semantic change.

The final boot/classification evidence and complete artifact identities are in
`bld/codex-stall-repair-20261015/final/native-catalog-diagnostic.{json,log}` and
`TRUEOS-Blueprints/probes/code_mode_probe/validation/native-final-catalog-blocked.{json,log}`.
Earlier `da52b6df` native Code Mode/libc/MCP passes remain recorded separately and
must not be presented as completed Code Mode validation of `dc0bfe30`.

## Final deployment artifacts

Use the published Codex package `e5ba8f6e` with the final kernel bundle at
`bld/codex-stall-repair-20261015/final/TrueOS.lz4` (SHA-256
`e8d23afc59bfb543d36b29e8d4d2d62392e28634998c0fe38fe265b257a49f96`).
The accompanying ISO is `final/trueos.iso`, SHA-256
`d2b8ce4e06880f4dae963f2bfb69fb271fb7edf258dc3722e2ee7c9e0808d958`.
The final runtime ELF hashes to
`dc0bfe30b7e25266aa8d7ecaed0ba7d7bc82745d689de71e4bacd983799633c6`;
its matching full-symbol ELF hashes to
`088d64c38b1654425313aefc88635eb73d7eaeaf5082816f5309779f9fa21e67`.
Both are retained under `final/artifacts/` for exact subsequent symbolization.

`final/acceptance.json` records artifact/source identities, host checks and the
scope of each native run. Both owned QEMU instances were stopped after capture.
The physical rig and its boot image were left untouched; physical full-turn
responsiveness remains to be observed with the updated kernel and app together.
