# BCS0 scheduling opportunities

Static audit, 2026-11-01, source baseline `dc24df914`, followed by the Shell3
implementation described below. The audit findings refer to that baseline.
No physical GPU measurements have been taken for this change.

There are useful opportunities for the existing workload. The strongest is
reducing CPU preparation and submission/retirement boundaries. The two BLT
backends also justify different policies for small glyph work and large copies.
Their presence does not establish two independently schedulable copy engines.

## Implemented Shell3 pilot

The kernel-internal Shell3 UI4 copy backend now uses
[`BcsPlan`](../../src/intel/copy/bcs_plan.rs). Its glyph route retains legacy
SRCCOPY, while cached viewport and resize transfers prefer aligned fast copies
and fall back to legal legacy copies without changing coverage.

- Contiguous GPU fills and glyph copies share ordered batches. An initial
  full-frame clear can join the first GPU batch; CPU stores still require
  preceding GPU work to retire and later GPU work to acquire published pixels.
  Shell3 keeps its existing fill-fragmentation fallback to the original glyph
  copy stream, avoiding additional backend-transition drains for that workload.
- Batches contain at most 160 operations and fit the 8 KiB command buffer,
  including internal dependency barriers and the existing 20-DWORD envelope.
- Fast-source/color changes get explicit barriers; legacy/fast transitions
  retain the hardware-implied flush. Only the migrated plan routes gain this
  dependency/alignment policy; other existing fast-copy APIs are unchanged.
- Unique source mappings are collected before submission. The shared normal
  copy lane now uses [`BcsMappings`](../../src/intel/copy/bcs_mappings.rs):
  topology/control aliases initialize once, changed transient leaves are cleared
  and replaced, and only merged dirty PTE cache lines are published. Identical
  mapping sets need no PTE writes or page-table publication. Boot/probe full
  initialization invalidates this metadata before rebuilding the image.
- Existing small-operation CPU budgets, CPU-local emulator text, exact marker
  plus saved-HEAD retirement, and cancellation/quarantine ownership remain.
  GPU completion still uses the existing polling path and one inflight lane.

The old standalone legacy-copy submission wrapper is removed because all its
callers now use plans. Legacy command encoding remains in the plan encoder.

Host checks:

```sh
python3 tools/testpy/test_bcs_plan.py
python3 tools/testpy/test_bcs_text_paint.py
python3 tools/testpy/test_bcs_driver.py
cargo check --locked --offline
```

These exercise real production planning, command words, page-table updates,
pixel replay with alpha/guards, and async ownership with hardware boundaries
stubbed. They establish correctness of the host logic, not hardware execution.

For the first physical check, use Shell3's UI4 copy backend on the Intel rig:
large warmed text repaint, retained-text scrolling, and clear followed by
substantial text painting. Compare cold initialization separately from warm
repetition. The existing `allcaps::text_blit::DIAGNOSTICS` switch enables the
`text-paint` line with actual BCS batches, family participation, explicit
barriers, source mappings and encoded command bytes. It remains disabled by
default; mixed batches contribute to both family counts but only once to
`bcs_batches`. Small updates may correctly stay on the CPU. Run the same workload
before/after and inspect pixels, frame latency, CPU preparation and Busy retries
before changing thresholds. No rig reset or deployment is part of this change.

## Hardware evidence and scope

The supplied Tiger Lake PRM Volume 10 describes one command streamer dispatching
different instruction families to legacy and fast backends, with a shared flush
boundary (printed pp. 35–38). Its [Intel-hosted copy](https://cdrdv2-public.intel.com/705824/intel-gfx-prm-osrc-tgl-vol-10-copy-engine.pdf)
was checked against the supplied local text.

The local companion Volume 2a adds two material details:

- `XY_FAST_COPY_BLT`, printed pp. 1371–1373: switching between fast and legacy
  instruction families causes an automatic hardware flush before the next BLT.
  Fast-copy source and destination starts have an OWord alignment requirement.
- `XY_FAST_COLOR_BLT`, pp. 1367–1370: it belongs to the fast family too.
- `MI_FLUSH_DW`, pp. 990–992: it drains pending drawing work; its post-sync store
  can publish completion, and its notify bit can request a completion interrupt
  when the relevant interrupt controls are enabled.

Local references:

- [Volume 10 text](../../../G12TL_intel_prm/intel-gfx-prm-osrc-tgl-vol-10-copy-engine_images/intel-gfx-prm-osrc-tgl-vol-10-copy-engine.txt)
- [Volume 2a text](../../../G12TL_intel_prm/intel-gfx-prm-osrc-tgl-vol-02-a-command-reference-instructions_images/intel-gfx-prm-osrc-tgl-vol-02-a-command-reference-instructions.txt)

TRUEOS names Alder Lake and Raptor Lake devices in `src/intel/mod.rs`, and the
existing text threshold comment records an Alder Lake-S/AP5 sweep. Tiger Lake
documentation is evidence for an audit of this Gen12 implementation; exact
platform and stepping applicability still needs checking before changing
hardware policy. No new register values are proposed here.

## Existing consumers

| Consumer | Current route | Useful improvement |
| --- | --- | --- |
| Slot-0 window stack | Fast copies: restore base damage, then windows in broker z order; 160-copy chunks | Correct dependency barriers; unique source mappings; reduce waits between chunks; possibly subtract covered damage |
| Retained text glyphs | Small CPU copies, otherwise legacy RGBA copies in 128-copy chunks | Reuse atlas mappings; coalesce compatible cells; preserve the existing small-operation CPU route |
| Text viewport reuse and resize | CPU for small transfers, otherwise fast copies between distinct allocations | Avoid repeated setup for a few rectangles; alignment-aware routing |
| Ordered terminal fills and glyphs | Fast fills and legacy copies submitted and retired separately | A mixed ordered command list can remove repeated host submissions |
| Explicit BCS sprite route | Separate fast-fill and fast-copy batches; each retires before the next | Combine adjacent BCS operations where no render operation or external release separates them |
| Resident scene output | One fast copy with synchronous polling; compute fallback if BCS rejects before submission | Lower preparation overhead and distinguish contention from unsupported work |
| Display-writeback screenshot | On-demand UC-source fast copy; 1 ms admission/completion polling | Retain the UC source contract; improve wakeup latency if this consumer matters |
| Monochrome expansion | Legacy mono command, used by the diagnostic and opt-in benchmark | No evidence of a current production text caller that warrants redesigning around it |

Sources: `src/intel/display/ui4_bcs.rs`, `ui4_bcs_queue.rs`,
`src/ui4/text_area_raster.rs`, `text_paint.rs`, `blueprint_ui4.rs`,
`src/r/services/vcpy_service.rs`, `src/intel/render/primary.rs`,
`src/intel/media/wd_xyuv8888.rs`. This table records reachable source paths,
not measured frequencies or a claim that every conditional route runs each boot.

## 1. Highest-confidence overhead: page-table preparation

`queue_guc_bcs0_batch` calls `direct_blt_init_ppgtt`, which clears and reconstructs
the whole PPGTT allocation, then flushes it. `guc_blt_map_ui4_surfaces` maps the
destination and each copy source and flushes the whole allocation again.

The allocation is `(3 + 512) * 4096 = 2,109,440` bytes. `dma_flush` uses a
64-byte `CLFLUSH` loop followed by `MFENCE`. A normal copy submission therefore
executes **65,920 CLFLUSH calls across two complete passes**, in addition to
clearing about 2 MiB and writing the PTEs. These are instruction counts/range
sizes, not measured DRAM traffic or time. Mono adds another full table flush.

There is also duplicated work inside a batch:

- Sources are checked against all previous sources: 12,720 pair checks for
  160 copies, even if they all reference one atlas allocation.
- The full source allocation is mapped again for every descriptor.
- `shell3_font_service` already packs 64 styled cells into each retained atlas
  allocation. Repeated copies from that allocation need one mapping, not one
  mapping per glyph. The whole atlas allocation is the relevant mapping unit;
  it is not necessarily a single 4 KiB physical page.

A concrete first helper would be `BcsMappings`: initialize the upper-level
topology and control mappings once, collect unique source mappings, clear the
previous batch's transient leaf entries, install this batch's entries, and flush
only modified PTE cache lines. Perform all mutation under the existing exact
retirement/ownership check. Preserve the entry TLB invalidation initially.

The mapping identity must include GPU range, physical backing, page-rounded
extent and cache policy. GPU VA alone is insufficient because VAs are recycled.
Retain alias validation and the PAT3/UC destination and per-source cache
contracts. Clearing old transient entries preserves the current fresh-address-
space semantics without needing a permanently trusted cache of live allocations.

This benefits both backends and even one-copy submissions. It is the first
performance change to measure, before altering engine choice or CPU thresholds.

## 2. Correctness prerequisite: fast-copy dependencies

Volume 10 p. 38 requires a flush between fast copies from different source
surfaces into overlapping destination storage. It also requires a flush for
copy-after-write dependencies between commands.

`guc_blt_encode_copy_batch` currently emits an entry/TLB flush, all copy commands,
then one final post-sync flush. There are no intermediate barriers. The slot-0
planner deliberately restores a base rectangle and then overwrites it with
window rectangles from different sources. Thus the source contains a concrete
sequence matching the documented dependency restriction. The final flush
publishes completion but cannot substitute for a required earlier barrier.

The API rejects source/destination allocation aliasing, which excludes many
read-after-write cases; it does not exclude overlapping destination writes from
different sources. A first conservative implementation can flush on source-
surface changes for this shared destination. Do not infer that arbitrary
disjoint pixel rectangles waive the surface/cache-line restriction without
platform confirmation. A more selective planner must consider every outstanding
write since the last barrier, including aliases, rather than only adjacent pairs.

Capacity must include barriers. The existing 8 KiB batch fits 160 ten-DWORD
copies: `(12 + 160*10 + 8)*4 = 6,480` bytes. Adding 159 five-DWORD flushes takes
9,660 bytes. A helper must split by encoded bytes including padding/barriers;
keeping the existing rectangle cap alone would overflow it.

## 3. Alignment and backend choice

Volume 2a documents OWord-aligned starts for fast copy, including its source and
destination X1 fields. `guc_blt_valid_copy` checks neither start alignment; it
accepts, for example, RGBA destination X1=1 with a page-aligned base and 256-byte
pitch. Arbitrary window placement and rounded-edge spans can produce such
coordinates. The mapping's base alignment does not prove the BLT start alignment.

Treat this as a PRM compliance gap requiring target-specific confirmation, not
as proof that currently displayed pixels are wrong. An alignment-aware selector
can route small or unsuitable rectangles to legacy SRCCOPY when its signed
coordinate/pitch constraints permit. `XY_BLOCK_COPY_BLT` is another candidate to
investigate: the supplied instruction descriptions specify pixel-aligned starts
for that fast command, but its surface fields, limits and platform support need
an independent encoder and validation. Do not treat it as an opcode substitution.

Splitting into legacy edges and a fast interior only helps if both source and
destination can meet their alignment constraints together and the saved copy
time exceeds the extra commands and backend-switch flushes. It is unlikely to
be the first useful change for tiny glyphs.

The present legacy glyph route is therefore a reasonable specialization. The
current CPU thresholds also have recorded physical evidence. Re-measure after
removing preparation costs; no static analysis establishes a new pixel crossover.

## 4. Scheduling helper: ordered operations in one submission

`GucBcs0BatchKind` admits one operation family per submission. Terminal paint
and the explicit sprite route already have ordered plans but split them into
separate fills/copies and wait after each group. The existing fill encoder
actually emits opcode `0x44` (`XY_FAST_COLOR_BLT`); its internal `XY_COLOR_BLT`
comment is misleading. Fast fill is already used.

A proposed, unimplemented Rust interface is:

```rust,ignore
enum BcsOp {
    Fill(GucBcs0RgbaFill),
    Copy { copy: GucBcs0RgbaCopy, backend: CopyBackend, source_cache: SourceCache },
}

// Planning only; no MMIO or submission here.
fn plan_bcs(destination: GucBcs0RgbaSurface, ops: &[BcsOp], caps: BcsCaps)
    -> Result<BcsPlan, PlanError>;
```

`BcsPlan` should preserve painter order, collect unique mappings, choose legal
backends, account for hardware-implied and explicit barriers, and split into
bounded byte-sized batches. Start with one destination and the existing
single-inflight lane; no multiple-context scheduler is required.

For `fill -> legacy glyphs -> fill`, one submission can replace three host
preparations and retirement waits. The two hardware-implied backend-transition
flushes remain. For `fast clear -> fast copies`, there is no legacy transition
to provide an implicit boundary: establish the required dependency barrier
explicitly. Preserve boundaries around RCS work and external publication.

Do not reorder all legacy commands together merely to reduce switches: z order,
text overwrite order and sprite order carry visible meaning. Grouping by source
is permissible only after proving independence. The slot-0 planner already
coalesces matching adjacent rows and skips fully covered tiles; further damage
subtraction could remove base/lower-window copies hidden by later owners.

## 5. Completion ownership is part of scheduling

All consumers share `KernelClient::Vcpy`, one batch/result/PPGTT storage set and
one pending submission. `poll_guc_bcs0_rgba_copies` both establishes exact
hardware retirement and releases the busy flag. Hardware completion by itself
does not make the software lane available until its owner polls.

Text and compositor paths poll on 1 ms timers; scene/fill helpers and the sprite
route spin. Thus another consumer can receive `Busy` while completed work is
waiting for its next owner poll. Many text callers treat `Busy` as failed
admission, whereas screenshots retry it. This is a latency/fairness opportunity
independent of copy bandwidth; a missed poll can add roughly a timer interval
per serialized chunk, although completion timing and executor delay determine
the actual cost.

A bounded completion pump could retire BCS requests independently of their
callers and wake completion/admission waiters. Retain completed receipts until
the matching token consumes them: the current poll API treats an absent pending
submission as invalid. An interrupt can wake this pump, but it cannot replace
the existing marker plus context-save ownership proof. Both checks are needed
before shared storage reuse. Existing GPU futures wake after software retirement;
awaiting one alone does not drive this backend's hardware retirement.

Preserve cancellation pinning, uncertain-submission quarantine and exact
timeline retirement. Merely changing Vcpy's inflight limit from one to two is
unsafe with its shared mutable storage. The executor's existing two-inflight
Ui4Compositor exception is a different client/storage arrangement.

If consumer-aware admission is later needed, bounded queued requests can carry
consumer identity, byte/command estimates and latency class, with aging or a
burst limit to avoid starvation. Current `RgbaCopyConsumer` accounting identifies
only resident scenes; it is not a scheduling policy for all these users. GuC
still owns physical scheduling. Start with prompt retirement and measurements
before adding priorities or a second copy context.

## Recommended implementation and measurement order

1. Add dependency/alignment coverage and resolve the documented fast-copy gaps.
2. Deduplicate mappings, then replace whole-PPGTT reconstruction with sparse
   leaf updates while retaining the current TLB/ownership contracts.
3. Introduce ordered mixed batches for terminal and explicit BCS sprite work.
4. Decouple retirement progress from caller timer cadence with retained receipts
   and bounded admission/wakeup handling.
5. Evaluate further occlusion elimination, block-copy support and pipelining
   only against measurements from the same workloads.

Measure preparation, alias checks/PTE updates, cache publication, GuC submission,
marker observation, context-save observation and caller notification separately.
Record consumer, unique source mappings, command count, encoded bytes, copied
bytes, explicit barriers, backend transitions and Busy retries. Compare frame
latency and CPU time as well as copy throughput.

The opt-in `text_blit_bench.rs` is useful existing infrastructure, but its copy
sweep uses one large fast-copy rectangle and its mono sweep uses legacy mask
expansion. It does not directly compare legacy RGBA glyph copies with fast RGBA
glyph copies, model many atlas sources, or reproduce production 1 ms polling.
Extend with those shapes, overlapping painter copies, mixed fill/copy plans,
unaligned window positions and concurrent text/compositor traffic. Preserve
readback equality and untouched guards. No new threshold is justified yet.

## Verification performed for this audit

A temporary host harness compiled the actual production validators, PPGTT
functions and copy encoder with ordinary host backing and a recording
`dma_flush` stub. Four baseline checks passed:

- One copy causes two full 2,109,440-byte PPGTT flush requests.
- Two overlapping fast-copy writes from distinct sources are adjacent commands
  without an internal barrier.
- The current validator admits an RGBA destination start four bytes past an
  aligned base.
- A 160-copy batch flushes 6,480 encoded command bytes before added barriers.

These established the baseline source behavior and arithmetic. The initial
harness was temporary; the implementation now has the permanent host runners
listed above. Hardware execution, cache coherence, chipset applicability,
speedups and saturation still require physical validation.
