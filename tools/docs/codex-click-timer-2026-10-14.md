# Click-dependent timer updates and long stalls — 2026-10-14

## What changed in the observation

The user reported that clicks sometimes let the working timer catch up, then
showed it stationary at 5m08s, recovering to 6m37s, with no model reply after
seven minutes. The recovery means an apparently frozen frame cannot by itself
establish a permanent deadlock. It also does not establish that the click
caused recovery rather than being processed when existing work returned.

The `Copy failed` toast comes from application UI handling and demonstrates
input reaching Codex at least during a recovery window. It does not establish
timer delivery or response-stream progress.

No runtime source, timing policy, AP placement or interrupt handler changed
in this investigation. The physical rig was observed through its capture log;
all debugger stops and injected mouse input occurred in the existing QEMU VM.

## Physical progress/stall evidence

The snapshot is
`bld/codex-evidence/timer-stall-20261014-170657/capture.log`, from
`LatestOfThree.logs` resolving to `trueos-baremetal.1.log`.
`summary.json` records its hash and probe excerpts.

Successive samples show different states:

- Earlier AP19 was progressing slowly: 87 executor passes in 50 seconds.
- Later it remained at poll counts `3163978/3163977`, with a Tokio worker's
  `guest-enter` sequence unchanged for more than a minute.
- It subsequently advanced to `3163980/3163979` and a `codex-main` continuation
  was sampled at `guest-enter`, already there for 31687 ms.
- A later live read reached `3163986/3163985`, then sampled a Tokio worker's
  unchanged sequence 30308224 with stage age 290083 ms and two ready tasks.

These observations establish intermittent execution followed by long periods
without returning to the executor. They do not identify a physical instruction
pointer. Transport errors and absence of a completed reply remain separate
observations.

## Scheduled redraws are expected without clicks

The local Codex sources explicitly schedule another frame when the working
timer is active: `tui/src/status_indicator_widget.rs`, `StatusIndicator::render`,
uses 32 ms when animation effects are active or 1000 ms otherwise.
`tui/src/tui/frame_requester.rs` sends deadline requests to a Tokio task that
awaits `tokio::time::sleep_until` and broadcasts a draw notification. Input and
draw subscriptions then enter the main TUI event loop through
`tui/src/tui/event_stream.rs`.

Thus a healthy idle working timer is not intentionally driven only by clicks.
Both scheduling starvation and a separate timer/wake defect can produce the
reported symptom. A rendered elapsed value can jump because elapsed time is
calculated when rendering eventually runs; it is not a counter proving one
successful redraw every second.

## New QEMU debugger lead: plugin decoding and allocation work

The retained run is `bld/codex-debugger/file-wait-fix-20261014-161801/`.
It was resumed for read-only debugger observations. Successive captures land
in `FreeList::dealloc`, `FreeList::allocation_block_is_live`, and the C allocation
registry during malloc. Free pointers differ between stops, while the native
continuation does not return to its outer executor turn.

The allocator's production deallocation path first scans the free list to
validate that the allocation does not overlap an already-free span. It then
walks the ordered list again to find an insertion point. This work occurs under
the heap mutex and has no cooperative handoff.

`timer-stall-heap-count.txt` traverses **65536** ordered, non-overlapping free
nodes before hitting the diagnostic limit; it does not reach the list's end.
The inspected prefix contains 49865720 free bytes. This is a lower bound on
list length, not proof that the entire list is valid or a full-heap free-byte
measurement. The earlier complete 2007-node inspection describes an earlier
heap state and must not be substituted for this later one.

Two module return addresses resolve in the pinned symbol artifact:

| Return address | Caller |
| --- | --- |
| `0xffff80021820711c` | serde_json Value MapDeserializer, PluginSummary field deserialization |
| `0xffff8002182082cc` | serde_json Value MapDeserializer, PluginInterface field deserialization |

`timer-stall-callers.json` preserves the exact symbols and offsets. This ties
the sampled frees to plugin metadata decoding rather than guessing from the
working timer. The original work-directory module was subsequently absent;
resolution uses the pinned `20261014-141846/codex-symbols.o` for the package
running in this QEMU instance. This is not a symbol attestation of the current
physical package.

Both emulator and physical logs also contain the C allocation-tracking-full
record at capacity 65536. The shim explicitly returns the allocation untracked,
so this record is not an out-of-memory result. A later typed emulator ledger
snapshot finds 40427 occupied and 25109 empty slots; saturation is therefore
historical, not continuously asserted at every stop. See
`timer-stall-ledger.txt`.

One optimized debugger frame reported an unreliable `self` argument. A later
capture (`timer-stall-heap-alias.txt`) resolves the active allocator directly to
the host VM0 shared allocator at `0xffffffff846c6008`. Conclusions use that
confirmed allocator and its typed metadata, not the earlier optimized argument.

The QEMU evidence supports a large synchronous plugin decode/free batch that
can starve peer tasks on one cooperative AP. It does not establish the cause
of every physical stall, the response transport error, or absence of a
separate timer problem. Stage ages spanning QEMU's earlier pause include that
pause; they must not be presented as minutes of continuously executing CPU work.

## Mouse comparison was inconclusive

The 15-second idle/mouse/idle comparison is saved in `mouse-171316/`.
It produced generic Shell3 output and no Codex frame markers: the new SSH
session did not reattach the intended application's view. Therefore its equal
output rates cannot establish that mouse input does or does not unblock Codex.
The failed reattachment is recorded rather than counted as a successful test.

## Next discriminating experiment

Measure allocator validation/insertion costs against free-list length using
the same plugin payload and one AP. Separate time spent parsing/dropping the
payload from time spent inside the heap and allocation ledger. Preserve
allocation provenance, overlap checks and DMA pin rules when evaluating a
shorter lookup; do not disable validation merely to speed up the workload.

A separate timer comparison needs a live Codex-owned SSH terminal, a quiet
working period, and marked mouse input while collecting emitted frame bytes
and scheduler probes. Until that succeeds, click causality remains unverified.
No allocator replacement or blanket malloc/free yield was applied here: the
caller may retain locks across an allocation, so inserting a handoff at every
allocation would need its own locking-contract audit.
