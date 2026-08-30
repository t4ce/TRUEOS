# Semi-persistent GPU-font producers

## Decision

The next font-service architecture is a set of **semi-persistent producer
leases**, not a larger collection of one-shot font submissions.  A service
registers a producer once with one static font tier/size and fixed row
geometry.  That registration retains the GPU resources and producer identity
until the service releases the lease.  To render at another tier, a service
releases and acquires another producer; a service which needs several tiers at
once holds several leases.

Each produced surface represents exactly one row of text containing `1..N`
characters.  The row is the producer's unit of capacity and handoff; it is not
an arbitrary full-frame canvas.

"Semi-persistent" is intentional.  The CPU performs control-plane work at
registration, ACK, release, error recovery, and final retirement.  Between
those boundaries, retained GPU state cycles rows through production and
presentation without per-row allocation, mapping, descriptor construction, or
CPU pixel access.  GPU-side reuse/retirement means the producer may make an
ACKed row runnable again itself; it does *not* mean hardware autonomously
unmaps or frees kernel-owned DMA memory.  Final unmap/deallocation remains a
CPU-observed retirement action.

## Implementation status

The first kernel/GPU slice is now present:

- `src/r/font_producer_service.rs` owns one global 32-slot registry,
  generation/sequence-safe row tokens, bounded credits, release/retirement,
  reversible pre-submit cancellation, and permanent quarantine for ambiguous
  GPU ownership.
- `src/r/font_kernel_service.rs::register_gpu_font_producer` allocates a
  bounded fixed-geometry RGBA8 row ring once at registration. Its row API
  rejects a different face, tier, extent, multiline payload, or character
  overflow and reuses the retained GPU virtual ranges.
- `register_ui4_gpu_font_producer` uses an ordinary UI4 `FontScene2d`
  Dirty/Double Frame as that retained ring instead. `submit_ui4_row` reserves
  the producer token for the acquired UI4 buffer's exact index and writes the
  glyph directly into that allocation; there is no staging surface, CPU pixel
  write/readback, or frame copy.
- registered rows enter the existing Font FIFO and exclusive RCS lane. The
  worker credits `PRODUCED` only after the returned release fence matches the
  exact row allocation; a reversible failure restores the reservation, while
  `SubmittedIncomplete` pins the row and its complete lease generation.
- `FontProducedRow::mark_surflive` records that the row became display-live
  but deliberately does not restore its credit. Only
  `acknowledge_display_release`, called after a later replacement becomes
  SURFLIVE and releases this exact display lease, performs the token-checked
  `SURFLIVE -> ACKed/idle` transition. Dropping that capability without the
  acknowledgement abandons and pins the generation instead of manufacturing
  a reusable row.

`cpp font rush` and `cpp font rush2` are the UI4 consumers. Rush stages up to
32 producer leases (eight per active plane) while retaining its original
showcase sequence. Its terminal producer storm reactivates every available
canvas in lockstep, mapping each plane rank to one row of the shared 8x4
producer grid. Rush2 settles four Frames/windows on application planes 0..3,
keeps slot 4 reserved, registers two producers per plane, and activates them
in a 1/2/4/8 ladder. Each lease keeps one of four static font sizes while
SoftRng changes its row payload, color, and alpha. A published row retains its
capability by exact double-buffer index; reacquiring that index supplies the
CPU ACK. Every 30 seconds Rush2 drains both exact frame generations, releases
the current eight leases and their caches, then freshly registers the same
tiers for `font`, `noto-sans-sc`, and `inconsolata` in rotation. On shutdown,
the capability is retained until the whole UI4 Frame is destroyed, then an
exact completion-checked retirement ACK releases the producer generation.

The Font RCS runtime still remains one job slot. Both Rush variants therefore
demonstrate independently backpressured producer queues and persistent UI4
rings, but do not claim 32 parallel GPU submissions or a one-producer-per-EU
mapping.
Abandoned ACK capabilities and ambiguous GPU retirement currently pin their
complete producer generation until reboot; a future device-reset recovery path
must reclaim those quarantined generations only after it proves the old
context can no longer address them.

## Current baseline and the move

The names below are anchors, not a claim that the current implementation
already has producer leases.

| Current component | Current responsibility | Target responsibility |
| --- | --- | --- |
| `src/r/font_plan_service.rs`: `FONT_PLAN_WORKER_COUNT`, `font_plan_worker_task` | 32 executor tasks build transient, bounded, post-warm glyph plans; a worker claims a cell and yields. | 32 available producer *slots* may host leases.  A bound slot keeps its font-tier and row contract; CPU preparation is retained only where dynamic input requires it. |
| `src/r/font_plan_service.rs`: `FontPlanBatchRequest`, `PreparedGlyphPlan` | A frame/batch plan has transient ownership and crosses once to FontKernel. | `FontProducerRegistration` and a bounded per-producer input ring describe repeated rows for one lease. |
| `src/r/font_kernel_service.rs`: `REQUESTS`, `GPU_LANE`, `IN_FLIGHT`, `font_kernel_service_task` | One FIFO, one GPU permit, and one in-flight request funnel all font work. | A producer registry plus bounded ready/ACK queues; initially still one dispatch lane, later an isolated multi-slot scheduler. |
| `src/intel/gpu_font.rs`: `GpuFontRetainedScene`, persistent-job facilities | Existing retained scene/resource vocabulary and font registration. | Retained per-lease descriptors, text/input storage, row outputs, and generation-tagged control state. |
| `src/intel/gpgpu/operations/primitives.rs`: `allocate_font_instance_rgba8_surface_cleared` | Allocates and maps one owned RGBA8 surface. | Registration-time allocation of a lease's fixed row ring; no allocation in the normal row cycle. |
| `src/intel/gpgpu/rcs/runtime.rs`: `FONT_RCS_GPU_VA`, `font_rcs_state_once`; `src/intel/gpgpu/runtime_state.rs`: `FONT_RCS_SUBMIT_LOCK`, `FONT_RCS_SUBMIT_RUNTIME` | One Font RCS state with `job_slots: 1`, one batch/result window, and a serialized pending submission. | First preserve that safety boundary; later provide independently owned slots/ranges and completion records before admitting concurrent producer submissions. |
| `src/ui4/compositor_service.rs`: `release_replaced_direct_lease`; `src/ui4/window_broker.rs`: `acknowledge_window_frame` | UI4 observes SURFLIVE, acknowledges the published window frame, and releases the replaced display lease only after the replacement is live. | The producer consumes the corresponding exact-row ACK as its reusable-row credit. |

The 32 tasks in `FONT_PLAN_WORKER_COUNT` are CPU executor workers.  They are
not 32 iGPU execution units, threads, contexts, nor a reservation of the UHD
770's EUs.  The design can feed GPU work efficiently and expose GPU pressure,
but it must never equate pool width with GPU hardware parallelism.  GPU
parallelism is a separately measured/scheduled property of the Font RCS
context, kernel dispatch shape, and hardware occupancy.

## Target code placement

Keep the control plane, retained GPU resources, execution scheduler, and
display acknowledgement separate:

- `src/r/font_plan_service.rs` remains the bounded CPU preparation facility.
  It may prepare row input, but it must not own row surfaces, GPU mappings, or
  display credits.
- a new `src/r/font_producer_service.rs` should own the 32-slot lease registry,
  producer generations, per-producer bounded input/ready/ACK queues, credits,
  and register/release APIs.  This is the kernel font sidepath exposed to
  services; it is not part of the general 3D or Spirit ownership model.
- `src/r/font_kernel_service.rs` should first act as the compatibility adapter
  and serialized dispatcher for registered producers.  Its one-shot request
  surface can shrink as callers move to leases, while its existing exclusive
  GPU lane remains the stage-one safety boundary.
- `src/intel/gpu_font.rs` and `src/intel/gpgpu/operations/` own the retained
  per-lease font representation, descriptors, input storage, row allocation
  ring, exact release proof, and GPU-side clear/rearm operation.
- `src/intel/gpgpu/rcs/runtime.rs` owns any later change from the singleton
  Font RCS job to isolated submission slots.  Producer registration alone
  must not weaken this runtime boundary.
- UI4 frame/window/compositor code owns the exact published row through
  SURFLIVE and returns only the matching `FontRowToken` ACK.  It must not know
  how a producer prepares glyphs or schedule Font RCS work.

After registration, normal row traffic must not recreate, remap, resize, or
CPU-read the retained output.  Supplying a new text/input descriptor to the
bounded producer queue is payload ingress, not a lifecycle mutation of that
backing resource.  The next CPU lifecycle touch is the exact ACK (or an
explicit release/error boundary); the ACK merely restores a credit and does
not allocate a replacement row.

## Lease contract

Registration is accepted only after the selected face/tier is warm.  The
registration fixes all reuse-sensitive properties:

- face and static font tier/size (and any static raster/hinting parameters);
- pixel format, row height, maximum row width, pitch/alignment, and the maximum
  character count `N`/encoded-input bytes per row;
- a bounded row-ring depth, descriptor layout, and the producer's stable ID;
- output destination class (standalone GPU surface or a UI4-compatible exact
  surface) and the required final release-fence protocol.

The fixed **row geometry**, not font size alone, makes reuse safe.  A variable
length string is allowed only within the registered `1..N` and width bounds;
overflow is rejected, explicitly split by the caller, or routed to a
differently registered producer.  It must not silently resize the retained
buffer.

Proposed control-plane shapes (names are illustrative):

```text
FontProducerRegistration {
  face, tier, static_font_pixels, row_width_px, row_height_px, format,
  max_chars, row_ring_depth
}
FontProducerLease { producer_id, generation, registration }
FontRowToken { producer_id, generation, row_index, sequence }
```

`generation` prevents a late ACK for a released/reacquired producer from
crediting a newly assigned resource.  `sequence` prevents duplicate or stale
ACKs from reopening a row more than once.

## Row ownership and state machine

One row slot has exactly one owner at every point:

```text
REGISTERED_IDLE --submit(row text)--> GPU_OWNED
GPU_OWNED --final GPU release fence--> PRODUCED
PRODUCED --publish exact row surface--> SURFLIVE
SURFLIVE --matching display/compositor ACK--> ACKED
ACKED --GPU reuse/clear is complete--> REGISTERED_IDLE

any non-terminal state --release requested--> RETIRING
RETIRING --all submitted/display ownership has retired--> RELEASED
```

The ACK above is not the notification that this row first became SURFLIVE.
At that instant scanout owns and may still read the row. Its reusable credit
returns only when a later replacement is SURFLIVE and the compositor releases
this row's exact display lease.

The implementation may combine `ACKED` with a GPU-side clear/reuse command,
but must not expose `REGISTERED_IDLE` until the resource is safe for the next
writer.  A producer with no `REGISTERED_IDLE` row has zero credits and accepts
no more text.  This is the desired natural backpressure: a slow downstream
consumer holds the exact row, so its producer stops without a guessed global
queue limit.  Conversely, an ACK pulls capacity back into that producer (the
"pull-vacuum" side).

`PRODUCED` requires the existing style of final cache-draining release proof:
`GpgpuRgba8ReleaseFence` in
`src/intel/gpgpu/types/surfaces.rs` is bound to the exact physical allocation
and byte length.  An address, producer ID, or completion of an earlier command
is never sufficient to publish or ACK a row.  The UI4 integration must retain
the concrete `FrameReadLease`/published-frame ownership until SURFLIVE and
must map its ACK back to the matching `FontRowToken`; preview-global completion
is not a producer credit.

Errors, cancellation, device reset, and ambiguous submission retirement do not
manufacture credits.  They quarantine or retire the affected lease generation
until the normal exact-resource cleanup proof is available.  This agrees with
the present Font RCS policy that an ambiguous pending submission protects its
shared mapping rather than allowing a rewrite underneath it.

## Scheduling boundary

The first migration must **not** claim 32 concurrent Font RCS submissions.
Today `FontKernelGpuLease` documents exclusive FIFO Font Engine admission and
`FONT_RCS_GPU_VA` has one job slot; `FONT_RCS_SUBMIT_LOCK` and
`FONT_RCS_SUBMIT_RUNTIME.pending` preserve that single in-flight ownership.
Therefore the initial producer implementation may batch/round-robin ready rows
through the existing single submit lane.  This removes repeated allocation and
ties pressure to real surface ownership without weakening current RCS safety.

Only after explicit RCS isolation exists may multiple ready producers submit
independently.  That later scheduler needs separate, non-overlapping batch,
result, descriptor, scratch, and GPU-VA ownership per slot; per-slot completion
and retirement records; fair bounded admission; and a quarantine domain that
cannot let a failed producer overwrite a live one.  Whether its width should
approach 32 is a measurement decision, not an architectural constant: kernel
occupancy, memory bandwidth, display interaction, and other RCS clients decide
the safe width.

## Migration plan and gates

1. **Document and measure the baseline.** Keep the current visual behavior.
   Record plan-pool activity separately from Font RCS lane wait, submit,
   completion, and UI4 display retirement.  Establish no-leak/no-stale-release
   counters for owned RGBA8 surfaces and existing release fences.
2. **Introduce lease types and registry behind the existing API.** Add bounded
   registration, generation IDs, static geometry validation, lease release,
   and per-producer counters.  Do not change Font RCS submission width.  A
   compatibility adapter may translate a one-shot request into a short-lived
   single-row lease while callers migrate.
3. **Retain row resources at registration.** Allocate/map the row ring and
   static descriptors once; submit rows with no normal-path output allocation.
   Route ready rows fairly through the existing one-submit lane.  Prove that a
   full producer only blocks itself and that unrelated producers continue when
   capacity/lane policy permits.
4. **Connect exact SURFLIVE ACK credits.** Carry `FontRowToken` alongside the
   exact release fence and UI4 ownership.  Reopen only the ACKed row after its
   reuse transition.  Verify late, duplicate, mismatched, and post-release ACKs
   are rejected without changing credits.
5. **Replace the singleton Font RCS scheduler only if profiling warrants it.**
   Build multi-slot state and retirement isolation first, then admit a small,
   measured width.  Expand only with hardware validation showing independent
   progress and no regression for compositor, Spirit, 3D, or other RCS users.

Every stage must keep these gates true:

- one row surface maps to one row token, one exact release fence, and one ACK;
- no CPU pixel readback/copy appears on the live row path;
- registration is the only normal allocation/mapping boundary, and release is
  the only final deallocation boundary;
- bounded input, ready, and row rings put an upper bound on retained memory;
- a producer can never change its tier or geometry in place;
- all timeout/device-loss paths preserve ownership and quarantine ambiguous
  resources rather than retrying a possibly executed write;
- the 32 CPU producer slots retain their existing topology/placement checks,
  while GPU saturation is reported from GPU metrics rather than inferred from
  that count.

Validation should include unit tests for state transitions, generation and ACK
matching, credit underflow/overflow, release during every state, and geometry
overflow.  Integration tests should exercise an intentionally stalled UI4
consumer, mixed-tier services holding multiple leases, one-character and
maximum-length rows, repeated register/release cycles, and GPU-reset/ambiguous
retirement handling.  Hardware profiling must report per-producer enqueue,
GPU completion, SURFLIVE, ACK, reuse latency, credit depth, Font RCS lane
contention, and display/compositor impact.

## Non-goals and risks

This design does not make a CPU worker equal an EU, promise all 32 EUs will be
occupied, replace the general 3D/Spirit execution architecture, or authorize
parallel direct-RCS writes before state isolation is implemented.  It also does
not turn variable-size text into an unbounded retained allocation.

The primary risks are stale ACK reuse, a release fence associated with the
wrong row, under-specified geometry, producer starvation beneath an initially
serialized lane, and unsafe concurrent rewrites of the one-slot Font RCS
state.  Generation-tagged exact tokens, bounded credit rings, exact release
proofs, fair admission telemetry, and the staged scheduler replacement are
the required mitigations.
