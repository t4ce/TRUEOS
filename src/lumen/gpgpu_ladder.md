# Lumen GPGPU Ladder Notes

This note records the current left-to-right ladder from the tiny TRUEOS
baremetal GPGPU harness toward the Lumen `burn_baba` backend.  It separates the
things that are proven from the things that are only shaped or prepared.

## Current Proven Steps

### 1. RCS command submission and readback path

Status: proven.

The MI-only preflight proves the RCS batch path, result buffer mapping, CPU
readback, and recovery loop.  It does not prove EU thread execution.

Latest stable markers:

- `gpgpu-preflight batch-submit-proof completed=1`
- `marker_observed=0xC0DE7731`
- `dot_observed=300`
- `sum_a_observed=10`
- `sum_b_observed=100`
- `lanes_observed=4`

Meaning: the command streamer can execute our batch and write known result
slots.  This is the floor for all higher GPGPU experiments.

### 2. Minimal GPGPU walker thread lifecycle

Status: proven earlier, superseded by step 3.

The first good milestone was one GPGPU group with one SIMD8 hardware thread.
That means eight lanes were dispatched by one thread payload, not eight
separate host-visible threads.

Important distinction:

- `right_mask=0x000000FF` means the SIMD8 lanes are enabled.
- `threads_started=8` / `starts_delta=8` is the public TS counter expressed as
  SIMD8 lane dispatch.
- `post_walker_marker=1` proves the command streamer continued after the worker
  completed.
- `eot_retired=1` means the worker EOT retired enough for the walker to finish.

Meaning: we proved the minimum empty/EOT EU lifecycle.  This gave the first
clean "worker ran and said done" baseline.

### 3. Static DP4A plus stateless HDC store plus EOT

Status: proven on 2026-05-10 08:31 boot log.

Active artifact:

- `gfx12-static-dp4a-hdc1-stateless-store-then-ts-eot`

The latest run is materially stronger than the empty EOT baseline.  It proves a
large walker, EU instruction decode through static immediate setup and DP4A
shape, a dataport/HDC store to the result buffer, and EOT retirement.

Key proof lines:

- `program_source=gfx12-static-dp4a-hdc1-stateless-store-then-ts-eot`
- `expects_store=1`
- `binding_table_present=1`
- `surface_state_present=1`
- `curbe_present=1`
- `surface_base=0x860000`
- `dynamic_state_base=0x860000`
- `instruction_base=0x863000`
- `walker_cmd=0x7105000D`
- `x_dim=186`
- `group_count=186`
- `right_mask=0x000000FF`
- `expected_lane_dispatch=1488`
- `ts_delta=1488`
- `post_walker_marker=1`
- `shared_ram_value=0xC0DE7733`
- `store_hits_mask_lo64=0x0000000000400000`
- `store_landed_anywhere=1`
- `eot_retired=1`
- `failure_class=shared-ram-write-proven`

Interpretation:

- `group_count=186` with SIMD8 lanes gives `186 * 8 = 1488` lane dispatches.
- This is not 1488 independent CPU-style threads.  It is 186 GPGPU groups, each
  launching one SIMD8 worker payload.
- The store value `0xC0DE7733` landed in the intended result slot, so the EU
  program executed enough to issue a side-effecting dataport store before EOT.
- The command streamer saw the walker complete and executed the post-walker
  marker.

This is the first wide, side-effecting GPGPU milestone suitable to feed into
`burn_baba` planning.

### 4. Walker scale ladder

Status: measured on the 2026-05-10 08:55 baremetal log drain.

The harness now repeats the same proven artifact with a fine walker X ladder
around the observed cliff:

- `186` groups = `1488` SIMD8 lane dispatches
- `224` groups = `1792` SIMD8 lane dispatches
- `256` groups = `2048` SIMD8 lane dispatches
- `288` groups = `2304` SIMD8 lane dispatches
- `320` groups = `2560` SIMD8 lane dispatches
- `352` groups = `2816` SIMD8 lane dispatches
- `384` groups = `3072` SIMD8 lane dispatches
- `416` groups = `3328` SIMD8 lane dispatches
- `448` groups = `3584` SIMD8 lane dispatches
- `480` groups = `3840` SIMD8 lane dispatches
- `512` groups = `4096` SIMD8 lane dispatches

Each scale emits `walker-scale-proof` with requested groups, expected hardware
thread payloads, expected lane dispatches, observed lane dispatches, store
status, post-walker marker status, and EOT retirement.  The ladder stops at the
first non-clean proof so the last good scale is visible in the same log.

Observed result:

- `186` groups is clean: `expected_lane_dispatch=1488`,
  `observed_lane_dispatch=1488`, `retired=1`, `post_walker_marker=1`,
  `store_seen=1`.
- `224` groups is already non-clean: `expected_lane_dispatch=1792`,
  `observed_lane_dispatch=304`, `retired=0`, `post_walker_marker=0`,
  `store_seen=1`.
- The first failing run still writes `0xC0DE7733`, so it reaches enough EU code
  to perform the HDC store, but it does not retire the full walker.
- The `304` count is a partial dispatch delta from the second scale attempt,
  not a clean 224-group proof.  It means the cliff is being hit during/after
  launch, before all `224 * SIMD8` lanes retire.
- The current proven upper rung for this artifact remains 186 SIMD8 payloads,
  or 1488 SIMD8 lane dispatches.

This narrows the cliff to the range `(186, 224]` for the current static
DP4A/HDC-store/EOT artifact shape.

Current hold setting:

- Keep the static walker ladder at the validated `186` group rung.
- Do not spend this phase tuning intermediate walker counts or other launch
  parameters.
- Treat `186` groups as the local GPU backend capacity estimate for planning:
  `186` hardware threads and `1488` SIMD8 lane dispatches.

The Lumen backend planner now budgets only 20% of that validated local GPU
capacity for the GPU side.  For the current `1792` row decode matvec, that
means roughly `37` GPU-budget hardware threads and `358` target shadow rows,
while CPU/AP remains responsible for the real result and the remaining rows.

The runtime status exported to Lumen latches the clean `186` proof.  Later
sidepaths therefore plan from the validated frontier instead of from an
experimental failed scale attempt.

## What This Still Does Not Prove

The current artifact still does not prove model matvec.

Not yet proven:

- Per-lane or per-group unique output indexing.
- Loading live Lumen activation `x` from GPU-visible memory.
- Loading BF16 model weight rows from the tile arena.
- Accumulating a real dot product over `k_dim`.
- Writing one F32 result per output row.
- Re-entering this path from the Lumen `burn_baby` matvec call as the actual
  backend.

The latest milestone proves that the door is open: the walker can launch wide,
execute a non-empty EU artifact, touch the result buffer through HDC, and retire.

## Immediate Backend Ladder

The next `burn_baba` ladder should stay local first:

1. Keep `burn_baby` CPU/AP as the correctness backend.
2. Keep net CPU disabled as a route and only use it later as a separate shadow.
3. Add a local GPU shadow result type that can report:
   `candidate`, `program_name`, `group_count`, `lane_dispatch_count`,
   `expected_store_value`, `observed_store_value`, and `frontier`.
4. Advance the artifact from one global magic store to indexed magic stores:
   one marker per group or per selected group range.
5. Then replace static magic with static ALU-derived values.
6. Then bind the Lumen tile arena for `x`, BF16 weights, and F32 output.
7. Only after that, route selected BF16 matvec chunks through local GPU.

The current correct label for the local GPU side is still "shadow/probe", not
"matvec backend".

## T4 Live-Input Sidepath

Status: runtime-observed as a CPU-authoritative live-row probe.  This is ready
to feed the next artifact, but it is not yet a live GPU-load or model-matvec
proof.

The old T3 artifact is preserved as the active GPU proof:
`gfx12-static-dp4a-hdc1-stateless-store-then-ts-eot`.

A T4 catalog rung now exists in `trueos-eu`:
`gfx12-t4-live-x-static-dp4a-requirement-hdc1-store-then-ts-eot`.
It deliberately reuses the proven T3 EU shell for now, but has its own artifact
kind so the next live-load kernel can be switched in without losing the known
good static DP4A/HDC/EOT baseline.

At the Lumen callsite, `burn_baby::matvec_rowmajor_bf16` now emits a one-shot
`lumen-gpu-proof: director-step step=4 mode=t4-live-row-probe` record once the
static GPU artifact is proven.  That record includes:

- live `x` pointer, byte size, and checksum
- resolved matrix manifest id/name hash when available
- row-0 BF16 pointer and checksum
- CPU-authoritative row-0 dot bits
- CPU-authoritative `live x[0..4] * static [1,2,3,4]` bits

The 2026-05-10 Lumen runtime trace observed the sidepath during a real
`model.layers.9.input_layernorm.weight` inference call:

- `rows=1792`
- `k_dim=2048`
- `chunk_rows=27`
- `chunks=67`
- `x_bytes=8192`
- `x_checksum=0x744A3F6EB4387271`
- `row_checksum=0x04E06A8D7736AB16`
- `static4_weights=01020304`
- `static4_expected_bits=0x3E181500`
- `row0_cpu_expected_bits=0xBD25C5EA`
- `gpu_submission=0`
- `next=stage-manifest-row-to-gpgpu-arena`
- `does_not_prove=gpu_live_load_or_model_matvec`

This proves the exact tuple the first real T4 GPU load kernel must reproduce:
manifest row + live activation vector + expected output.  It still logs
`gpu_submission=0`, so it does not overclaim live GPU memory loads yet.

Readiness call: the T4 live-input capture/CPU-reference sidepath is complete
enough to proceed to the next rung.  The next rung should be a guarded one-tile
GPU shadow compare that keeps CPU/AP ownership of the real output, stages one
manifest row plus `x` into the GPGPU arena, submits one tile only, and compares
the GPU-written result against the already logged CPU reference.

## CPU Compute Lane Telemetry

The CPU/AP side now has a sparse runtime heartbeat for BF16 matvec work.  This
is intentionally not a full per-row trace.  It samples the first few matvec
calls and worker chunks, then logs occasional later samples:

- `burn-baby: bf16 compute begin` records the call index, rows, `k_dim`,
  `chunk_rows`, local/remote row split, worker count, and queue counters.
- `burn-baby: bf16 chunk-start` records the AP slot that actually picked up a
  chunk, the row range, row count, `k_dim`, current done count, and queue depth.
- `burn-baby: bf16 chunk-finish` records the same chunk range, elapsed time,
  and done count after the worker completed the chunk.
- `burn-baby: bf16 compute done` records local chunk completion, elapsed time,
  submitted/completed/polled deltas, and final queue depth.

This gives a lightweight answer to "is it calculating right now?" while keeping
the log useful during long prefill runs.  The real result owner is still CPU/AP;
the local GPU path remains proof-only until a later rung explicitly transfers
or compares output ownership.

## T4.5 One-Tile Arena Stage

Status: runtime-observed as a staging-only bridge from T4 to the first one-tile
GPU shadow compare.  This proves CPU-side staging into the mapped GPGPU arena,
not a live GPU-load or model-matvec result.

After the T4 live-row record, Lumen now calls
`intel::stage_gpgpu_one_tile_shadow_probe` and emits:

- `intel/gpgpu: one-tile-stage`
- `lumen-gpu-proof: director-step step=5 mode=one-tile-arena-stage`

The staging layout is deliberately minimal:

- `x` vector at `arena_gpu_base + 0`
- row-0 BF16 weights at `arena_gpu_base + x_bytes`
- output tile at `arena_gpu_base + x_bytes + weight_tile_bytes`

The stage copies the live `x` bytes and one BF16 row into the mapped GPGPU
arena, zeros the rest of the weight tile/output tile, flushes those ranges, and
checksums the staged bytes back from CPU memory.  It still logs
`gpu_submission=0`, so this is not a live GPU-load proof.  Its job is to make
the next artifact precise: the GPU kernel should read those staged addresses,
write one shadow result, and compare that result to `row0_cpu_expected_bits`.

Runtime checkpoint:

- 2026-05-10 `make iso` produced `bld/trueos.iso` from
  `bld/artifacts/debug-859619db83ff/TRUEOS.elf`.
- The subsequent Lumen inference trace reached step 5:
  `lumen-gpu-proof: director-step step=5 mode=one-tile-arena-stage`.
- The Intel staging proof reported:
  `intel/gpgpu: one-tile-stage staged=1 reason=staged`.
- Staged layout:
  `arena_gpu_base=0x4000000`, `x_gpu=0x4000000`,
  `row_gpu=0x4002000`, `output_gpu=0x4102000`.
- Staged byte counts:
  `x_bytes=8192`, `row_bytes=4096`, `output_bytes=1024`.
- Staged checksums matched the T4 live-row tuple:
  `x_checksum=0x744A3F6EB4387271`,
  `staged_x_checksum=0x744A3F6EB4387271`,
  `row_checksum=0x04E06A8D7736AB16`,
  `staged_row_checksum=0x04E06A8D7736AB16`.
- CPU reference remains:
  `cpu_expected_bits=0xBD25C5EA`.
- The proof still logs `gpu_submission=0` and
  `does_not_prove=gpu_live_load_or_model_matvec`.

Upfront GPGPU trace checkpoint:

- The next captured upfront GPGPU trace still preserves the T3 baseline:
  `186` groups retire cleanly with `observed_lane_dispatch=1488`,
  `post_walker_marker=1`, `store_seen=1`, and `store_value=0xC0DE7733`.
- The known scale cliff is unchanged: the `224` group probe reaches only
  `observed_lane_dispatch=304`, keeps `store_seen=1`, but does not retire the
  walker or post-walker marker.
- The same trace reaches Lumen and observes the T4.5 stage cleanly after the
  static T3 proof is latched.

Readiness call: T4.5 staging is complete, but it is not enough to widen the
worker-count ladder yet.  The next rung is a readback/understanding checkpoint
for the one-worker tile before any new GPU output is claimed.

## T4.6 One-Worker Tile Readback

Status: implemented, awaiting runtime observation.

This rung exists because the one-tile scenario needs its own before-state proof
before we begin adding more SIMD8 worker payloads.  It still submits no GPU
matmul work.  Instead, it reads the just-staged arena state back and emits:

- `intel/gpgpu: one-tile-readback`
- `lumen-gpu-proof: director-step step=6 mode=one-worker-tile-readback`

Expected clean proof:

- `readback_ok=1`
- `staged=1`
- `x_match=1`
- `row_match=1`
- `output_zeroed=1`
- `output_first_bits=0x00000000`
- `output_nonzero_dwords=0`
- `output_expected_hits_lo64=0x0000000000000000`
- `gpu_submission=0`

The important interpretation is narrow: the arena contains the live `x` vector
and row-0 BF16 bytes, the shadow output tile is still untouched zero state, and
CPU/AP still owns the real inference result.  This proves the starting line for
the first actual one-worker read/ALU/write kernel, not GPU output or model
matvec correctness.

Only after this readback is runtime-clean should the next artifact submit a
single one-worker tile kernel.  Scale probing from roughly `12` up to `256`
SIMD8 payloads belongs after the first one-worker output readback is understood.

## T4.7 One-Worker Output Sentinel

Status: runtime-observed as a one-worker output-addressability proof.

This rung opts in the first GPU write to the staged one-tile output arena.  It
does not attempt model math.  It patches a tiny copy of the proven
HDC-store-then-EOT payload so that one SIMD8 worker writes a distinct sentinel
to the staged output tile:

- program: `gfx12-t47-one-tile-output-sentinel-hdc1-stateless-store-then-ts-eot`
- sentinel: `0xC0DE7747`
- target: the T4.5/T4.6 `output_gpu` address, currently `0x4102000`
- groups: `1`
- expected lane dispatch: `8`

Expected clean proof:

- `intel/gpgpu: one-tile-output-sentinel`
- `lumen-gpu-proof: director-step step=7 mode=one-worker-output-sentinel`
- `submitted=1`
- `readback_ok=1`
- `reason=sentinel-written` or `reason=sentinel-written-no-ts-delta`
- `output_first_before=0x00000000`
- `output_first_after=0xC0DE7747`
- `output_hits_lo64=0x0000000000000001`
- `observed_lane_dispatch=8` for a counted dispatch, or `0` when the one-group
  command retires cleanly but the public TS counter does not move
- `output_owner=cpu-ap`

The important interpretation is again narrow: the GPU can write to the staged
one-tile output buffer and the CPU can read that write back.  This proves arena
output addressability for one worker.  It still does not prove BF16 loads,
one-row dot math, model matvec, or result ownership transfer.

Runtime checkpoint:

- The latest Lumen trace reached `director-step step=7`.
- `submitted=1`, `finished=1`, and the sentinel write read back correctly.
- The current trace before the readback-label cleanup logged
  `readback_ok=0 reason=no-dispatch-delta` because `observed_lane_dispatch=0`,
  even though the output and finish marker were correct.  The runtime condition
  is now classified as `readback_ok=1 reason=sentinel-written-no-ts-delta`.
- `expected_lane_dispatch=8`; current tiny one-group run observed no TS counter
  delta, so `observed_lane_dispatch=0` is preserved as telemetry rather than
  treated as a failed readback.
- `output_first_before=0x00000000`.
- `output_first_after=0xC0DE7747`.
- `output_hits_lo64=0x0000000000000001`.
- `finish_marker=0xC0DE7732`.

This rung deliberately did not require a fresh offline EU artifact because the
semantic question was addressability, not arithmetic.  Reusing the proven
HDC-store/EOT shell and patching only the immediate sentinel plus output GPU
address kept the new variable small.

## T4.8 One-Worker Output Compare Echo

Status: low-level Intel artifact implemented, but deliberately de-wired from
the Lumen BF16 runtime path while backend ownership is being cleaned up.

This rung is the first local GPU result that is surfaced upward as comparison
information rather than a magic addressability sentinel.  It still does not
claim a live model load.  The kernel is a one-worker copy of the proven static
DP4A/HDC-store/EOT shell, patched at runtime so the GPU writes the CPU reference
bits into the staged one-tile output shadow:

- program:
  `gfx12-t48-one-tile-output-compare-dp4a-echo-hdc1-stateless-store-then-ts-eot`
- groups: `1`
- expected lane dispatch: `8`
- output target: the T4.5/T4.6 `output_gpu`
- compare source: CPU reference `row0_cpu_expected_bits`
- output ownership: still `cpu-ap`

Expected clean low-level proof once explicitly exercised:

- `intel/gpgpu: one-tile-output-compare`
- `submitted=1`
- `finished=1`
- `readback_ok=1`
- `compare_ok=1`
- `gpu_value=cpu_expected_bits`
- `reason=compare-written` or `reason=compare-written-no-ts-delta`
- `output_owner=cpu-ap`
- `does_not_prove=model_matvec_or_gpu_live_load`

The important interpretation is narrow but useful: the low-level local GPU path
can emit a comparable output value into the one-tile arena.  It is not currently
called from the Lumen BF16 path; `burn_baba` remains CPU/AP-director telemetry
with local GPU dispatch disabled by selector, and network routing remains off.

The next rung after T4.8 remains the real live-load artifact: one staged-row
load, one activation-vector load, one tiny arithmetic reduction or checksum/dot
surrogate, then one output write, still with a single worker and CPU/AP output
ownership.

## T5 Small Live4 Packed-BF16 Dot

Status: runtime-proven as a live GPU load/math/store proof for a tiny real
Lumen slice, still proof-only and still CPU/AP-owned.

Active artifact:

- `gfx12-t5-small-live4-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`

What it proves:

- the Lumen BF16 matvec path can stage the live activation vector and a real
  model row into the GPGPU arena
- the GPU kernel can load live `x` values and packed BF16 row halves from that
  arena
- the shader unpacks contiguous row lanes `[0,1,2,3]`, not the older dword view
  `[0,2,4,6]`
- the shader computes the four-element partial dot and stores the expected F32
  bits into the output record
- CPU readback sees the GPU value at the intended output offset

Current runtime shape:

- `live_k_dim=4`
- `requires_live_gpu_load=1`
- `output_owner=cpu-ap`
- `artifact_addressing=fixed-slot-reused`
- `does_not_prove=full_model_matvec`

The latest actual-work run arms three tile-frontier rows from the live Lumen
matvec:

- `tile_index=0`, row `0`
- `tile_index=1`, row `256`
- `tile_index=2`, row `512`

All three staged tiles compare correctly for the T5 live4 slice.  The aggregate
proof reports `armed_tiles=3`, `staged_tiles=3`, `submitted_tiles=3`, and
`compare_ok_tiles=3`.  This is deliberately not a full tile matvec yet: the
same fixed arena slot and same T5 live4 artifact are reused for each tile-frontier
probe, while CPU/AP keeps the real inference result.

Current T5 scale cap:

- `4096` groups is clean: `expected_lane_dispatch=32768`,
  `observed_lane_dispatch=32768`, `retired=1`, and the packed-BF16 result
  matches the CPU reference.
- `6144` groups is the first non-clean proof: it still writes the correct value
  and reports `observed_lane_dispatch=49152`, but it does not retire cleanly
  (`reason=submit-not-finished`, `retired=0`).

Hold setting:

- Keep `GPGPU_T5_LIVE4_GROUP_X_DIM_LADDER` capped at `[4096]`.
- Do not tune this cap as a throughput knob.  Revisit it only when the T5
  kernel grows beyond `live_k_dim=4`, the CGP queueing model changes, or the
  retire/completion logic becomes more expressive.

## T6 Small Live8 Packed-BF16 Dot

Status: wired hot after the T5 live4 rung, still proof-only and still
CPU/AP-owned.

Active artifact:

- `gfx12-t6-small-live8-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`

What changes from T5:

- `live_k_dim` grows from `4` to `8`.
- The shader unpacks packed BF16 row lanes `[0,1,2,3,4,5,6,7]`.
- The same staged activation vector, staged row, arena surface, fixed output
  slot, and CPU/AP readback ownership are preserved.
- T6 only runs after the T5 compare for that staged tile succeeds, so T5 remains
  the boot-green guardrail.

Runtime shape:

- `t5-small-live4-bf16-dot` remains step 9.
- `t6-small-live8-bf16-dot` is step 10.
- `t6-actual-work-tiles` is step 11 and reports separate T5/T6 submitted,
  finished, and compare-ok tile counts.

Current T6 scale cap:

- `GPGPU_T6_LIVE8_GROUP_X_DIM_LADDER` starts at `[4096]`, matching the clean T5
  retire frontier until T6 has its own boot log history.
- Do not tune this cap as a throughput knob.  Revisit only when the kernel,
  CGP queueing model, row-count semantics, or retire logic changes.

Next meaningful direction: widen the actual math toward the full row or give
each GPU worker a distinct output-row responsibility.  Raising group count alone
does not turn the single fixed-slot proof into full matrix-vector work.

## Backend Selection Boundary

The network backend is intentionally out of scope for the local one-tile GPU
phase.  `burn_net` already proved the architectural separation by carrying BF16
matvec descriptors to another host over TCP, but this rung stays local:

- `ROUTE_BF16_MATVEC_TO_NET_BACKEND=false`
- `SHADOW_BF16_MATVEC_TO_NET_BACKEND=false`
- Runtime confirms `net_cpu_route=0` and `net_cpu_shadow=0`.
- Runtime confirms `lumen-net: shadow bf16 matvec route_enabled=0
  action=no-shadow-frames`.
- Runtime confirms `lumen-net: remote bf16 matvec adapter present
  route_enabled=0 action=local-burn-baby-only`.

The local GPU role is therefore only one shadow tile.  It must not receive half
the row range or become a result owner.  The CPU/AP path continues to compute
the full local result, while the GPU path acts like one extra tile worker whose
first job is proving a single shadow output.

## Iteration Loop

Use the ISO build as the tight proof loop.  A simple `!make iso` builds the
image and starts the baremetal log drain.  After about 40 seconds the ISO should
contain the upfront GPGPU traces needed to extract the current rung state.

For each one-tile iteration:

1. Make one small artifact or staging change.
2. Build with `!make iso`.
3. Extract only the relevant `intel/gpgpu`, `lumen-gpu-proof`, and `burn-baba`
   proof lines from the new drain.
4. Update this ladder with the exact proof or blocker.
5. Only then advance the next rung.

The rule for this phase is still CPU-reference-first: the GPU may write shadow
results, but CPU/AP keeps ownership of real inference output until the one-tile
compare is proven clean.
