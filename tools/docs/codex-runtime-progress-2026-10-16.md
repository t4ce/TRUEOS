# Native runtime progress in the 81-second Gridpaper run

The screenshot identifies Codex `3d9e41ad` and a successful Gridpaper launch in
1m21s. The frozen, payload-free report is
`bld/codex-performance-20261016/baseline.json`: capture prefix 2,541,950 bytes,
SHA-256 `95eb3234b2011c56fabdd33770c2ab0ba7f43a6c4328a07a4757f533e2f7a66f`.
No live session input or reset was used for this audit.

## Exact kernel binding

The reset receipt and capture identify runtime ELF
`4005b76a3b46aa8d9742403c2ecc2f5fe100ee61c4dd6e84260227afab6e129d`,
73,248,136 bytes. It matches `bld/artifacts/debug-2c5ef4ca1470/TRUEOS.elf`.
Running the Makefile's `strip -s` operation on that directory's full ELF
reproduces the runtime hash exactly. The full ELF SHA-256 is
`b74f82a8f731f9659e38d93ca4dd12d5f3c3ac8d9048e16c5ec339bdc8fb2a34`.
BUILD_INFO records debug mode, build ID `2c5ef4ca1470`, timestamp
2026-10-16T14:43:00Z. The matching symbols, rather than another release build,
were used for the following return-address attribution.

## Observed progress and its limits

| Observation | What it establishes |
| --- | --- |
| HTTP tools/list sequence 8 receives 65,536 bytes by 76 ms, then ends cancelled at 98,304 bytes after 34,285 ms of body time. | This is a partial-body stall, following HTTP 200, not an initial DNS/connect failure. |
| Sequence 13 receives 267,264 bytes during its first 1 ms of body time, then ends cancelled after 18,561 ms. Headers already used 11,438 ms. | The remaining operation deadline expires after an initial burst; average throughput alone hides this pattern. |
| Follow-up sequence 17 receives the complete 608,041-byte body in 109 ms; JSON decode takes 1,278 ms. | The same platform can complete a large response. A permanent size cap or universally slow decoder does not explain the earlier cancellations. It does not prove that the earlier server responses were identical. |
| Worker 2147483650 on carrier 19 has synchronous continuation intervals of 7,607, 6,424, 9,414 and 6,324 ms. | Other continuations on that carrier cannot run during these intervals. The executor is cooperative. |
| During the 7,607-ms interval, the worker stays at guest-enter sequence 126516 while ABI entered counts advance from 1,425,190 to 1,619,947 in two observations five seconds apart. | The worker is making ABI progress, including observed allocation/deallocation boundaries; it is not frozen in one unchanged ABI call. The entered count includes all instrumented ABI kinds, not only allocations. |
| Sampled filesystem consumer delay is at most 909 ms here, versus 10,606 ms in the previous capture. | The prior extreme consumer-delay pattern improved. Sampled maxima are neither complete operation counts nor additive costs. |

The 7,607-, 6,424- and 4,335-ms wait-ending slices all finish through
`mkdir` -> `mkdirat` -> `request_broker::stat` -> `submit` -> parked WaitQueue
-> `threads::wait`. This identifies the final suspension boundary. It does
**not** attribute the preceding interval to stat, mkdir, or filesystem service.
The yield-ending slices finish through `trueos_cabi_poll_once`.

All Codex native/std/Tokio continuations are intentionally admitted to LastAP
by `CODEX_EXECUTOR_EXPERIMENT_ENABLED`. Source locations are
`src/r/threads.rs` admission and `src/workers.rs` application placement. This
helps localization but also means a long synchronous task can delay the
reactor, UI and timers together. That is a demonstrated progress consequence,
not proof that changing placement is the right repair. No placement change,
preemption, timer adjustment or timeout increase was made in this pass.

## Bounded allocator measurements for the next capture

The outstanding first-fit candidate needs actual search-work counts.
`FreeList::alloc` now invokes the unchanged selection algorithm with a local
candidate counter, then commits four saturating counters under its existing
lock:

- `alloc_calls`: completed allocation attempts, including failed attempts.
- `alloc_nodes`: cumulative attempted first-fit candidates, including an
  invalid candidate rejected before dereference.
- `alloc_max_nodes`: largest candidate count in one completed attempt.
- `alloc_failures`: attempts returning null.

There are no per-node atomics, clocks, formatting or additional locks. The
local count cannot overflow: validated nodes are ordered, disjoint and each
occupies at least the minimum block size; at most one invalid candidate is
visited before returning failure. A running attempt is committed only when
selection returns. This is work measurement, not a timing optimization.

`hv_guest_allocator_work_snapshot(vm_id)` uses `try_lock` and copies only these
four integers. It never initializes a heap or walks its nodes. An absent,
uninitialized or busy heap returns None and must not be reported as zero.
Counters reset on heap installation, restore-index rebuild and whole-heap
release. Raw counter drops therefore indicate reset; the reporter must not
infer a negative rate or join across heap lifetimes. Allocation tags, first-fit
choice, free-node/image layout and the shared 4-KiB state-page bound are
preserved.

The existing 14 production-extracted insertion-hint/interval-model tests pass.
A second suite runs 13 tests, including four new checks for exact fragmented
search counts, empty/invalid failure accounting, saturation/reset and a
nonblocking snapshot over an intentionally invalid head pointer. The snapshot
test would fault if the observer walked the free list.

The companion numeric network observations should distinguish three cases:
bytes stop arriving at the adapter; admitted socket bytes remain unconsumed;
or socket consumption advances while the HTTP body collector does not. These
counter observations, together with carrier progress, are the next decisive
evidence. The separate QEMU App Store/DNS hypothesis is not treated as the
cause of these established physical HTTP response-body stalls.
