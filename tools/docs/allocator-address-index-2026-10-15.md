# Fragmented heap lookup repair, 2026-10-15

The allocator previously proved that an allocation was live by scanning every
free node under the heap mutex. Freeing then scanned the address prefix again
to find the insertion point. `allocation_usable_size`, used by C realloc, and
DMA pin/unpin performed the same whole-list live check. This cost grows with
fragmentation even while hundreds of MiB remain free. The earlier watermark
admission repair did not remove these ordinary allocator scans.

The physical freeze capture independently showed a continuation spending
successive samples in short `dealloc` calls while its ABI entry counter kept
increasing. That evidence supports repeated allocator work, rather than one
blocked free. It does not establish that allocation is the only remaining
source of CLI latency.

## Implementation

`src/allocators.rs` now maintains 256 address fences in the allocator's existing
shared state page. Each fence names the last free node before its address band.
The sorted free list remains authoritative. A live check starts at the nearest
fence, validates its predecessor for overlap, and examines nodes through the
first node after the allocation. Free uses those same validated neighbors for
insertion, avoiding a second walk.

The index is updated on initialization, splitting, exact removal, insertion,
and both coalescing directions. Installing/resetting an arena clears it;
restoring a validated heap image rebuilds it once. It requires no allocations,
new lock, scheduler yield, or external ledger. The allocator still fits its
existing 4 KiB shared page. `FreeBlock` stays 16 bytes, `AllocTag` stays 32 bytes,
and the saved heap image format is unchanged.

Tag bounds, exact returned-pointer checks, domain checks, and GPU/DMA pin
retention remain in place. The inspected free nodes and their next links are
checked for bounds and address order before dereference/following. An invalid
free is rejected before list mutation. The local search relies on the ordered,
disjoint list invariant maintained by allocator operations and validated
restore; it is not a whole-heap corruption scan.

The first-fit allocation loop also previously wrote seven shared diagnostic
atomics per candidate, including rejected candidates. It now records the
selected candidate, or the last rejected candidate when allocation fails. Entry,
success, and malformed-pointer records remain. First-fit pointer traversal
itself remains linear.

## Host verification

Run `python3 tools/testpy/test_allocator_address_index.py`.

The harness extracts the actual production allocator and compares it with an
independent interval/index model. Nine tests passed, including 25,000 mixed
allocation/free/query operations with index and non-overlap checks after every
operation; wrong-domain, interior and double frees; DMA pin retention; index
rebuild/reset; malformed visited links and cached fences; and 15 small or
non-divisible arena geometries. An independent reviewer also checked every
list mutation and the shared-page/restore invariants.

The fragmented fixture reconstructs the previously observed free-node count
and byte totals: 46,920 nodes, 503,319,776 free bytes, largest free block
454,506,320 bytes. Two synthetic address distributions use 535,547,904-byte and
934,682,624-byte arenas. This reproduces geometry, not the physical heap's exact
address ordering.

The previous live checker always visits 46,920 nodes per successful check.
Production indexed node visits per check were:

| Address quantile | Irregular spacing | Page spacing |
| --- | ---: | ---: |
| 0% | 2 | 2 |
| 25% | 872 | 255 |
| 50% | 503 | 194 |
| 75% | 94 | 131 |
| 99% | 422 | 60 |

For 200 checks, old node visits are 9,384,000; indexed visits range from 400
to 174,400. The captured host run took 154–671 ms for the old checker and
0.002–1.11 ms for the indexed checker. Each checker includes an identical
per-visit counter; these elapsed times are host fixture measurements, not a
physical TRUEOS speedup.

A separate first-fit fixture with 46,921 free nodes and 100 allocations compares
the former trace policy with the new policy against the same allocator.
Candidate diagnostic stores fall from 32,844,700 to 700. Single-thread host
elapsed results vary with load and cache behavior: one run was 25.1 versus
26.2 ms, another 90.3 versus 40.8 ms. The store reduction is deterministic;
these timings do not establish a standalone trace speedup.

Receipt: `bld/codex-freeze-forensics-20261015/allocator-address-index.log`.
The existing watermark harness also still passes its four tests after fixture
updates for the derived index field.

## Native A/B verification

The same final allocation-contract Blueprint ran on the exact old and patched
TRUEOS runtimes in KVM, with six virtual CPUs, 4 GiB guest RAM, the same debug
`opt-level=1` build profile, and a 128 MiB application heap arena. C and Rust
allocation, alignment, realloc/data-preservation contracts and all workload
levels passed on both runtimes. The comparison uses a compiler-idle baseline
repeat and the patched run; it is a single-run workload comparison, not a
statistical benchmark.

Artifact identities:

- Probe SHA-256:
  `dc8bf05ac8091ef451d0b5981e72de461bef9f63515822eb1d4b98279de0b68f`.
- Old runtime SHA-256:
  `9d61cc238072ab73814e2335bb9f7ae9caa8d309620aca22b87cef3605f02d6a`.
- Patched runtime SHA-256:
  `9969094269f3e5a61b9e1c1f5be4e1112323ecaf46187fd96dea56926b3255ce`.

At the 50,000-live-block level:

| Operation | Old runtime | Patched runtime | Observed ratio |
| --- | ---: | ---: | ---: |
| Allocate 50,000 blocks | 8 ms | 20 ms | 2.5× slower |
| Free 25,000 alternating blocks | 2,712 ms | 37 ms | 73.3× faster |
| Free 25,000 remaining blocks in random order | 1,677 ms | 32 ms | 52.4× faster |
| Entire 50,000-block level | 4,684 ms | 114 ms | 41.1× faster |
| Full probe, including 3,000- and 10,000-block levels | 4,964 ms | 168 ms | 29.5× faster |

The longest 256-operation alternating-free batch fell from 111 ms to 1 ms;
the longest random-free batch fell from 81 ms to 1 ms. Timings have 1 ms clock
granularity. The new index has maintenance cost on the simple fresh-allocation
path; the demonstrated improvement is fragmented free/realloc/churn work.

An earlier old-runtime run measured 12.458 seconds for alternating frees while
the host compiler was active. That run is excluded from the ratios above:
host contention prevents a clean comparison against the compiler-idle patched
run.

Patched receipt:
`bld/codex-allocator-repair-20261015/patched-debug/native-safe.json`.
Compiler-idle baseline receipt:
`bld/codex-allocator-repair-20261015/baseline/native-compiler-idle.json`.
The latter was independently checked against numeric workload records in the
archived `bld/codex-native-runtime/qemu-allocator-baseline-idle-9d61/serial.log`,
including the final alternating-free, random-free and full-probe records at
lines 535, 577 and 583.

The patched runtime also passed the native code-mode probe's C/platform clock
bracket, independent condition-variable timeout, QuickJS modules, nested async
tool calls, session storage, cancellation/cleanup, and repeated timer controls
under one-worker, four-worker, and current-thread Tokio runtimes. Its delegated
`gri` transport remained running at the 30-second observation deadline; this
does not prove that Gridpaper appeared in UI4. The fixed-field receipt is
`bld/codex-allocator-repair-20261015/patched-debug/code-mode-native.json`.
Artifact hashes and the complete acceptance scope are recorded in
`bld/codex-allocator-repair-20261015/acceptance.json`.

## Limits

The fences divide the entire arena into 256 uniform address bands. A live
check examines the relevant band's nodes, rather than all arena nodes, but
fragmentation concentrated within one band remains linear. Sparse buckets do
not provide a universal constant-time bound. This is a bounded metadata index,
not a replacement size-class allocator or balanced tree.

First-fit allocation, full heap-stat requests, and cold snapshot traversal
remain linear. Native target measurements verify a large improvement for the
controlled fragmented-allocation workload above. The authenticated Codex UI on
the physical rig has not yet been verified with the patched kernel, so these
results do not establish that its multi-minute freezes are gone.
