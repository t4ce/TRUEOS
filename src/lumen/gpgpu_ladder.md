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

Status: proven on 2026-05-10 08:31 boot log, re-proven on 2026-05-25
after the HDC/EOT payload-register correction.

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

2026-05-25 correction: the old store-then-EOT tail reused `g127` for both the
HDC store message payload and the TS EOT payload:

- `mov g127 = store address/header`
- `send HDC store using g127`
- `mov g127 = g0`
- `send TS EOT from g127`

That sequence was observably unsafe on the local Intel path.  The HDC store
landed, but the worker did not retire:

- `target_value=0xC0DE7733`
- `retired=0`
- `post_walker_marker=0`
- `finish_marker=0x00000000`

The A/B artifact that preserved `g127` for the HDC send and used `g126` for TS
EOT retired cleanly.  The canonical `trueos-eu` HDC store-then-EOT artifacts now
use this tail:

- HDC store source/header remains `g127`
- EOT payload is copied into `g126`
- EOT send descriptor is `0x70007E0C`

Fresh proof from `bld/baremetal-logs/latest.log` on 2026-05-25:

- `program_source=gfx12-static-dp4a-hdc1-stateless-store-then-ts-eot`
- `send_w2=0x70007E0C`
- `src0_g=126`
- `store_value=0xC0DE7733`
- `retired=1`
- `post_walker_marker=1`
- `finish_marker=0xC0DE7732`
- `eot_retired=1`
- `failure_class=shared-ram-write-proven`

Interpretation: this was not a lost-EU or bad-HDC-descriptor problem.  It was a
payload lifetime/clobber problem in the local artifact sequence.  The safe rule
for generated HDC store artifacts is: do not overwrite the HDC send source0
message register before thread EOT; use a separate EOT payload register.

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
means roughly `37` GPU-budget hardware threads and `358` target proof rows,
while CPU/AP remains responsible for the real result and the remaining rows.

The runtime status exported to Lumen latches the clean `186` proof.  Later
sidepaths therefore plan from the validated frontier instead of from an
experimental failed scale attempt.

## What This Still Does Not Prove

The current static DP4A artifact still does not prove model matvec.

Not yet proven:

- Re-entering this path from the Lumen `burn_baby` matvec call as the actual
  backend.

Later rungs now prove live staged loads, packed BF16 partial dots, per-lane row
outputs, and row-block compare against CPU.  The missing piece is not "can the
EU run?" anymore; it is bounded result ownership plus scaling from the accepted
prefix toward the full `k_dim` reduction.

## Immediate Backend Ladder

The next `burn_baba` ladder should stay local first:

1. Keep `burn_baby` CPU/AP as the correctness backend.
2. Keep net CPU disabled as a route and only use it later as a separate shadow.
3. Add a local GPU proof/pilot result type that can report:
   `candidate`, `program_name`, `group_count`, `lane_dispatch_count`,
   `expected_store_value`, `observed_store_value`, and `frontier`.
4. Advance the artifact from one global magic store to indexed magic stores:
   one marker per group or per selected group range.
5. Then replace static magic with static ALU-derived values.
6. Then bind the Lumen tile arena for `x`, BF16 weights, and F32 output.
7. Only after that, route selected BF16 matvec chunks through local GPU.

The current correct label for the local GPU side is "proof/pilot", not "matvec
backend".  It now runs against actual Lumen work and writes distinct tile-record
outputs, but CPU/AP still owns the real inference result.

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
GPU proof compare that keeps CPU/AP ownership of the real output, stages one
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
GPU proof compare.  This proves CPU-side staging into the mapped GPGPU tile
record, not a live GPU-load or model-matvec result.

After the T4 live-row record, Lumen now calls
`intel::stage_gpgpu_one_tile_record_probe` and emits:

- `intel/gpgpu: one-tile-stage`
- `lumen-gpu-proof: director-step step=5 mode=gpgpu-actual-work-tile-stage`

The staging layout is record-local:

- `x` vector at record-local `+0x0`
- packed BF16 weights at record-local `+0x2000`
- output tile at record-local `+0x102000`
- record size `0x103000`

The stage copies the live `x` bytes and one BF16 row into the mapped GPGPU
arena, zeros the rest of the weight tile/output tile, flushes those ranges, and
checksums the staged bytes back from CPU memory.  It still logs
`gpu_submission=0`, so this is not a live GPU-load proof.  Its job is to make
the next artifact precise: the GPU kernel should read those staged addresses,
write one proof result, and compare that result to `row0_cpu_expected_bits`.

Runtime checkpoint:

- 2026-05-10 `make iso` produced `bld/trueos.iso` from
  `bld/artifacts/debug-859619db83ff/TRUEOS.elf`.
- The subsequent Lumen inference trace reached step 5:
  `lumen-gpu-proof: director-step step=5 mode=gpgpu-actual-work-tile-stage`.
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
and row BF16 bytes, the proof output tile is still untouched zero state, and
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
bits into the staged one-tile output slot:

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
- `artifact_addressing=tile-record-output-slots`
- `does_not_prove=full_model_matvec`

The latest actual-work run arms three tile-frontier rows from the live Lumen
matvec:

- `tile_index=0`, row `0`
- `tile_index=1`, row `256`
- `tile_index=2`, row `512`

All three staged tiles compare correctly for the T5 live4 slice.  The aggregate
proof reports `armed_tiles=3`, `staged_tiles=3`,
`t5_submitted_tiles=3`, `t5_finished_tiles=3`, and
`t5_compare_ok_tiles=3`.  Each tile now owns a separate arena record:

- record-local `x` starts at `+0x0`
- record-local packed BF16 row starts at `+0x2000`
- record-local output starts at `+0x102000`

The T5/T6 artifacts still see the same local offsets, but the runtime binds the
surface base to the selected tile record.  That lets the current proof address
distinct output rows without regenerating the shader.  CPU/AP keeps the real
inference result.

The cleaned-up runtime labels for this stage are:

- `gpgpu-actual-work-tile-stage`
- `gpgpu-actual-work-tile-readback`
- `tile-store-only-control`
- `tile-load-echo`
- `t5-small-live4-bf16-dot`

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
- The same staged activation vector, staged row, record-local arena surface,
  record-local output slot, and CPU/AP readback ownership are preserved.
- T6 only runs after the T5 compare for that staged tile succeeds, so T5 remains
  the boot-green guardrail.

Runtime shape:

- `t5-small-live4-bf16-dot` remains step 9.
- `t6-small-live8-bf16-dot` is step 10.
- `t6-1-live16-bf16-dot` is step 11 and widens the same one-row proof to
  live16.
- `t6-2-row-block-plan` is step 12 and declares the software row-block
  dispatch scheme for the tile.
- `t6-2-row-block-stage` is step 13 and restages one chosen 8-row global block
  into the artifact-visible row prefix.
- `t6-2-row-block-live16-partial` is step 14 and writes one output slot per
  SIMD8 lane/row for that staged block.
- `t6-3-accum16-hi-row-block-live32-partial` is step 15 and reuses the same
  row-block view while widening the per-row prefix reduction to live32.  It is
  intentionally a two-pass proof: T6.2 writes the low live16 partial, then the
  T6.3 accumulator reads that value and adds lanes 16..31.
- The first tile/first row-block T6.3 run also emits a one-shot
  `t6-3-first-tile-output-detail` scan over the whole 256-dword output tile.
  This is diagnostic only: it answers whether the live32 artifact wrote
  anywhere else in the result tile before we change the artifact again.
- `t6-3-actual-work-row-blocks` is step 16 and reports separate T5, T6, T6.1,
  T6.2, and T6.3 submitted, finished, compare-ok block counts.
- The aggregate next marker is now
  `next=promote-row-block-owner-or-scale-live-k`.

Current T6 scale cap:

- `GPGPU_T6_LIVE8_GROUP_X_DIM_LADDER` starts at `[4096]`, matching the clean T5
  retire frontier until T6 has its own boot log history.
- Do not tune this cap as a throughput knob.  Revisit only when the kernel,
  CGP queueing model, row-count semantics, or retire logic changes.

Next meaningful direction: promote the row-block owner so the block can become
durable tile output, then continue scaling live-k toward the full 2048-row
reduction.  Raising group count alone still does not turn the proof into full
matrix-vector work.

## T6.1 Live-K Tier Naming

`T6.1` is now the generated live16 artifact:
`gfx12-t6-1-live16-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`.

It keeps the T5/T6 tile-record layout and one-row output slot, but widens the
packed-BF16 reduction from 8 lanes to 16 lanes.  The oracle contract is recorded
next to the generated sources in
`.codex_tmp/t6_1_live16_packed_bf16_artifact_contract.md`.

## T6.2 Lane-Indexed Partial Tile

`T6.2` is the lane-indexed live16 artifact:
`gfx12-t6-2-lane-indexed-live16-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`.

It changes the shader contract in the smallest useful way:

- `gl_LocalInvocationID.x` selects the row inside the staged tile record.
- The same value selects the output dword inside the tile output region.
- One SIMD8 workgroup computes eight live16 packed-BF16 partial dots.
- The first runtime target is 8 rows, so the proof compares output slots
  `[0..7]` against CPU live16 references.

The first `gl_WorkGroupID.x` row-indexed artifact is preserved in `.codex_tmp`,
but hardware logs showed the legacy walker path retired it with zero visible
outputs.  That matches the older metadata clue that workgroup-id payloads were
not trustworthy in this shell, so T6.2 moves row selection onto SIMD lanes.

This is not full GEMM yet.  It proves that a tile can carry multiple staged
rows and that multiple workers can produce distinct row partials inside the
same output tile.

## T6.2 Row-Block Dispatch

The current row/block scheme deliberately does not depend on
`gl_WorkGroupID.x`.  CGP treats the T6.2 shader as an 8-row lane block:

- Software selects a global row block.
- The selected rows are restaged into the tile-record row prefix.
- The unchanged lane-indexed artifact computes rows `[0..7]` of that view.
- Logs carry `row_block`, `global_row_start`, and `tile_row_start`, so the CPU
  comparison knows which matrix rows the eight output words represent.

The current cap is eight row blocks per actual-work tile, so one tile can now
prove up to 64 row partials as eight explicit 8-row dispatches.  Across the
current three armed tiles, that gives a 192-row checked prefix while the legacy
walker row payload remains untrusted.

## T6.3 Live32 Row-Block Tier

`T6.3` is the lane-indexed live32 accumulator artifact:
`gfx12-t6-3-accum16-hi-live32-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`.

It deliberately does not change the row-block dispatch scheme:

- Software still restages one 8-row block into the tile-record row prefix.
- `gl_LocalInvocationID.x` still selects row/output slot `[0..7]`.
- T6.2 computes and stores the low live16 partial first.
- T6.3 then reads that row output, adds packed-BF16 lanes `16..31`, and stores
  the live32 result in the same row slot.
- T6.3 only runs after the T6.2 live16 compare for that row block succeeds, so
  the output it reads is known-good live data rather than a CPU-seeded fixture.
- The monolithic live32 artifact remains preserved as
  `gfx12-t6-3-lane-indexed-live32-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`,
  but hardware logs showed it retired with zero row outputs and a full first
  output-tile scan found no misplaced stores.  Its native store payload uses
  high GRFs around `g106/g109`; the accumulator stays close to the green T6.2
  register shape.

The oracle contract is recorded in
`.codex_tmp/t6_3_accum16_hi_live32_artifact_contract.md`.  This is still a
partial matvec proof, not full GEMM: it proves 8 row outputs for a 32-lane
prefix, and the CPU/AP remains the output owner.

Runtime proof, 2026-05-11:

- `t6-3-accum16-hi-live32-partial` finished with `compare_ok=1` for all 12
  dispatched row blocks.
- The director aggregate reported `t63_compare_ok_blocks=12` and
  `t63_compared_rows=96` across the three armed tiles.
- The one-shot first-tile scan reported `nonzero=8`, `expected_hits=8`, and
  `expected_misplaced_hits=0`, so the live32 row-block output landed only in
  the intended first eight output slots.

Runtime proof, 2026-05-25:

- The HDC/EOT `g126` tail correction allowed the canonical static DP4A proof to
  retire cleanly during the Lumen selftest path, so the local GPU proof now
  promotes immediately to live row work:
  `action=promote-to-live-row-proof`.
- The staged actual-work run armed three tiles from the live Lumen matvec and
  advanced through T5, T6, T6.1, T6.2, and T6.3.
- T6.2 reported `t62_submitted_blocks=24`, `t62_finished_blocks=24`,
  `t62_compare_ok_blocks=24`, and `t62_compared_rows=192`.
- T6.3 reported `t63_submitted_blocks=24`, `t63_finished_blocks=24`,
  `t63_compare_ok_blocks=24`, and `t63_compared_rows=192`.
- The aggregate step 16 frontier is now:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=192`,
  `cgp_prefix_live_k=32`, `action=offer-accepted-prefix`,
  `next=cpu-suffix-finish-or-scale-live-k`.

Interpretation: the local GPU path is past the old sentinel/retire frontier.
It has a CPU-reference-checked row-block prefix for real Lumen data.  The next
meaningful promotion is bounded prefix contribution/ownership while CPU/AP
finishes the suffix, or scaling live-k beyond the accepted live32 prefix.

Follow-up correction, 2026-05-25:

- The accepted-prefix handoff now queues CPU suffix jobs instead of completing
  the suffix inline on the Lumen/AP path.
- Fresh boot proof:
  `cgp accepted-prefix suffix-submit ... rows=192 ... live_k_dim=32
  k_dim=2048 suffix_jobs=12`.
- The AP compute lane then reports `submitted=74`: 12 prefix-suffix jobs plus
  the remaining full-row jobs.
- Completion proof:
  `cgp accepted-prefix complete ... rows=192 ... suffix_jobs=12
  total_cpu_jobs=74 first_output_bits=0xBD25C5EA
  last_output_bits=0x3D70DD26`.

This is still not a major speed path because the GPU prefix is only 32 lanes of
2048 and only covers 192 rows, but it removes the misleading serial suffix and
makes the hybrid ownership contract real: GPU-checked prefix, AP worker suffix,
and no full AP recompute for accepted rows.

## T6.4..T6.33 Windowed Live512 Row-Block Tier

`T6.4` through `T6.33` are the current live-k scaling rung:

- `gfx12-t6-4-windowed-accum16-live48-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`
- `gfx12-t6-5-windowed-accum16-live64-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`
- `gfx12-t6-6-windowed-accum16-live80-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`
- `gfx12-t6-7-windowed-accum16-live96-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`
- `gfx12-t6-8-windowed-accum16-live112-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`
- `gfx12-t6-9-windowed-accum16-live128-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`
- `gfx12-t6-10-windowed-accum16-live144-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`
- `gfx12-t6-11-windowed-accum16-live160-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`
- `gfx12-t6-12..33-windowed-accum16-live176..512-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`

These names describe the runtime proof role.  The first implementation reuses
the proven T6.3 accum16 artifact body instead of introducing a larger new EU
program.  Software stages later 16-lane K windows into the artifact-visible
lanes `16..31`:

- T6.3 restores/stages source window `16..32`, then accumulates live32.
- T6.4 stages source window `32..48`, then accumulates live48.
- T6.5 stages source window `48..64`, then accumulates live64.
- T6.6 stages source window `64..80`, then accumulates live80.
- T6.7 stages source window `80..96`, then accumulates live96.
- T6.8 stages source window `96..112`, then accumulates live112.
- T6.9 stages source window `112..128`, then accumulates live128.
- T6.10 stages source window `128..144`, then accumulates live144.
- T6.11 stages source window `144..160`, then accumulates live160.
- T6.12 through T6.33 continue the same 16-lane window ladder through
  `source_window=496..512`, then accumulate live512.

The restore before every T6.3 run is important.  The windowed T6.4/T6.5 stages
overwrite the artifact-visible high lanes of the tile record.  Without restoring
`16..32`, the next row block's T6.3 run compares CPU `x[16..32]` against stale
GPU `x[48..64]`, producing deterministic partial-output mismatches.  The
hardware fix was a CPU/dataflow staging restore, not a shader change:

- `mode=t6-3-accum16-hi-live32-window-restore`
- `source_window=16..32 artifact_window=16..32`

Runtime proof, 2026-05-25:

- `t63_submitted_blocks=24`, `t63_finished_blocks=24`,
  `t63_compare_ok_blocks=24`, and `t63_compared_rows=192`.
- `t64_window_staged_blocks=24`, `t64_submitted_blocks=24`,
  `t64_finished_blocks=24`, `t64_compare_ok_blocks=24`, and
  `t64_compared_rows=192`.
- `t65_window_staged_blocks=24`, `t65_submitted_blocks=24`,
  `t65_finished_blocks=24`, `t65_compare_ok_blocks=24`, and
  `t65_compared_rows=192`.
- The aggregate step 16 frontier is now:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=192`,
  `cgp_prefix_live_k=64`, `action=offer-accepted-prefix`,
  `next=cpu-suffix-finish-or-scale-live-k`.
- CPU suffix handoff accepted the live64 prefix:
  `cgp accepted-prefix suffix-submit ... rows=192 ...
  live_k_dim=64 k_dim=2048 suffix_jobs=12`.
- Completion proof:
  `cgp accepted-prefix complete ... rows=192 ... live_k_dim=64
  suffix_jobs=12 total_cpu_jobs=74 first_output_bits=0xBD2A32C8
  last_output_bits=0x3D81B1A6`.

Follow-up live80 proof, 2026-05-25:

- The direct generated T6.6 `accum32/live96` artifact did not retire
  (`code_words=384`, `completed=0`, `finish_marker=0x00000000`), so the hot
  path moved back to the proven small-window strategy.
- Windowed T6.6 reuses the T6.3 accum16 artifact body and stages
  `source_window=64..80` into `artifact_window=16..32`.
- Runtime proof:
  `t66_window_staged_blocks=24`, `t66_submitted_blocks=24`,
  `t66_finished_blocks=24`, `t66_compare_ok_blocks=24`,
  `t66_compared_rows=192`, and `t66_disabled_after_failure=0`.
- The aggregate step 16 frontier is now:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=192`,
  `cgp_prefix_live_k=80`, `action=offer-accepted-prefix`,
  `next=cpu-suffix-finish-or-scale-live-k`.
- CPU suffix handoff accepted the live80 prefix:
  `cgp accepted-prefix suffix-submit ... rows=192 ...
  live_k_dim=80 k_dim=2048 suffix_jobs=12`.
- Completion proof:
  `cgp accepted-prefix complete ... rows=192 ... live_k_dim=80
  suffix_jobs=12 total_cpu_jobs=74 first_output_bits=0xBD2A32C8
  last_output_bits=0x3D81B1A6`.

Follow-up live96 proof, 2026-05-25:

- Windowed T6.7 reuses the same small retiring T6.3 accum16 artifact body and
  stages `source_window=80..96` into `artifact_window=16..32`.
- The kernel shape stayed on the known-good small body:
  `code_words=212`, `artifact_kind=T63Accum16HiLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot`.
- Runtime proof:
  `t67_window_staged_blocks=24`, `t67_submitted_blocks=24`,
  `t67_finished_blocks=24`, `t67_compare_ok_blocks=24`,
  `t67_compared_rows=192`, and `t67_disabled_after_failure=0`.
- The aggregate step 16 frontier is now:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=192`,
  `cgp_prefix_live_k=96`, `action=offer-accepted-prefix`,
  `next=cpu-suffix-finish-or-scale-live-k`.
- CPU suffix handoff accepted the live96 prefix:
  `cgp accepted-prefix suffix-submit ... rows=192 ...
  live_k_dim=96 k_dim=2048 suffix_jobs=12`.
- Completion proof:
  `cgp accepted-prefix complete ... rows=192 ... live_k_dim=96
  suffix_jobs=12 total_cpu_jobs=74 first_output_bits=0xBD2A32C8
  last_output_bits=0x3D81B1A6`.
- The first T6.7 run proved the GPU rung but exposed a handoff bug:
  `cgp_prefix_rows=8` because the T6.6 fallback path was still promoting before
  T6.7 and clearing earlier live96 rows.  The director now treats T6.6 as a
  fallback only when T6.7 is disabled or fails, and the corrected boot proves
  `cgp_prefix_rows=192`.

Follow-up live112 proof, 2026-05-25:

- The boot now runs a prefill-only CGP autorun before the interactive prompt
  loop:
  `lumen: cgp autorun schedule session=1 prompt="hi" mode=prefill-only`.
- The autorun uses the raw one-token prompt and resets the AP2+ chat state
  afterwards:
  `lumen: AP2+ inference worker chat state reset session=1
  reason=cgp-autorun-complete`.
- Windowed T6.8 reuses the same small retiring T6.3 accum16 artifact body and
  stages `source_window=96..112` into `artifact_window=16..32`.
- The kernel shape stayed on the known-good small body:
  `code_words=212`, `artifact_kind=T63Accum16HiLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot`.
- Runtime proof:
  `t68_window_staged_blocks=24`, `t68_submitted_blocks=24`,
  `t68_finished_blocks=24`, `t68_compare_ok_blocks=24`,
  `t68_compared_rows=192`, and `t68_disabled_after_failure=0`.
- The aggregate step 16 frontier is now:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=192`,
  `cgp_prefix_live_k=112`, `action=offer-accepted-prefix`,
  `next=cpu-suffix-finish-or-scale-live-k`.
- CPU suffix handoff accepted the live112 prefix:
  `cgp accepted-prefix suffix-submit ... rows=192 ...
  live_k_dim=112 k_dim=2048 suffix_jobs=12`.
- Completion proof:
  `cgp accepted-prefix complete ... rows=192 ... live_k_dim=112
  suffix_jobs=12 total_cpu_jobs=74 first_output_bits=0xBD2A32C8
  last_output_bits=0x3D81B1A6`.
- Autorun timing proof from this boot:
  `lumen: cgp autorun done prompt="hi" prompt_tokens=1 first_token=3865ms
  total=3865ms final_next_token=446`.

Follow-up live128 proof, 2026-05-25:

- Windowed T6.9 reuses the same small retiring T6.3 accum16 artifact body and
  stages `source_window=112..128` into `artifact_window=16..32`.
- The kernel shape stayed on the known-good small body:
  `code_words=212`, `artifact_kind=T63Accum16HiLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot`.
- Runtime proof:
  `t69_window_staged_blocks=24`, `t69_submitted_blocks=24`,
  `t69_finished_blocks=24`, `t69_compare_ok_blocks=24`,
  `t69_compared_rows=192`, and `t69_disabled_after_failure=0`.
- The aggregate step 16 frontier is now:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=192`,
  `cgp_prefix_live_k=128`, `action=offer-accepted-prefix`,
  `next=cpu-suffix-finish-or-scale-live-k`.
- CPU suffix handoff accepted the live128 prefix:
  `cgp accepted-prefix suffix-submit ... rows=192 ...
  live_k_dim=128 k_dim=2048 suffix_jobs=12`.
- Completion proof:
  `cgp accepted-prefix complete ... rows=192 ... live_k_dim=128
  suffix_jobs=12 total_cpu_jobs=74 first_output_bits=0xBD2A32C8
  last_output_bits=0x3D81B1A6`.
- Autorun timing proof from this boot:
  `lumen: cgp autorun done prompt="hi" prompt_tokens=1 first_token=4143ms
  total=4143ms final_next_token=446`.

Follow-up live144 proof, 2026-05-25:

- Windowed T6.10 reuses the same small retiring T6.3 accum16 artifact body and
  stages `source_window=128..144` into `artifact_window=16..32`.
- The kernel shape stayed on the known-good small body:
  `code_words=212`, `artifact_kind=T63Accum16HiLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot`.
- Runtime proof:
  `t610_window_staged_blocks=24`, `t610_submitted_blocks=24`,
  `t610_finished_blocks=24`, `t610_compare_ok_blocks=24`,
  `t610_compared_rows=192`, and `t610_disabled_after_failure=0`.
- The aggregate step 16 frontier reached:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=192`,
  `cgp_prefix_live_k=144`, `action=offer-accepted-prefix`,
  `next=cpu-suffix-finish-or-scale-live-k`.
- CPU suffix handoff accepted the live144 prefix:
  `cgp accepted-prefix suffix-submit ... rows=192 ...
  live_k_dim=144 k_dim=2048 suffix_jobs=12`.
- Completion proof:
  `cgp accepted-prefix complete ... rows=192 ... live_k_dim=144
  suffix_jobs=12 total_cpu_jobs=74 first_output_bits=0xBD2A32C8
  last_output_bits=0x3D81B1A6`.
- Autorun timing proof from this boot:
  `lumen: cgp autorun done prompt="hi" prompt_tokens=1 first_token=4442ms
  total=4442ms final_next_token=446`.

Follow-up live160 proof, 2026-05-25:

- Windowed T6.11 reuses the same small retiring T6.3 accum16 artifact body and
  stages `source_window=144..160` into `artifact_window=16..32`.
- The kernel shape stayed on the known-good small body:
  `code_words=212`, `artifact_kind=T63Accum16HiLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot`.
- Runtime proof:
  `t611_window_staged_blocks=24`, `t611_submitted_blocks=24`,
  `t611_finished_blocks=24`, `t611_compare_ok_blocks=24`,
  `t611_compared_rows=192`, and `t611_disabled_after_failure=0`.
- The aggregate step 16 frontier is now:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=192`,
  `cgp_prefix_live_k=160`, `action=offer-accepted-prefix`,
  `next=cpu-suffix-finish-or-scale-live-k`.
- CPU suffix handoff accepted the live160 prefix:
  `cgp accepted-prefix suffix-submit ... rows=192 ...
  live_k_dim=160 k_dim=2048 suffix_jobs=12`.
- Completion proof:
  `cgp accepted-prefix complete ... rows=192 ... live_k_dim=160
  suffix_jobs=12 total_cpu_jobs=74 first_output_bits=0xBD2A32C8
  last_output_bits=0x3D81B1A6`.
- Autorun timing proof from this boot:
  `lumen: cgp autorun done prompt="hi" prompt_tokens=1 first_token=4778ms
  total=4778ms final_next_token=446`.

Follow-up live512 proof, 2026-05-25:

- The implementation stopped hand-wiring one Rust wrapper per rung after T6.11.
  The low-level submit path now has a generic windowed accum16 profile that
  reuses the same proven T6.3 EU body and labels each runtime rung with a
  stable `gfx12-t6-12..33-windowed-accum16-live*` program name.
- Runtime proof at the original row-block cap:
  `twindow_staged_blocks=528`, `twindow_submitted_blocks=528`,
  `twindow_finished_blocks=528`, `twindow_compare_ok_blocks=528`,
  `twindow_compared_rows=4224`, `twindow_disabled_after_failure=0`,
  `twindow_frontier_rung=33`, and `twindow_frontier_live_k=512`.
- The aggregate step 16 frontier reached:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=192`,
  `cgp_prefix_live_k=512`, `row_block_cap=8`,
  `action=offer-accepted-prefix`.
- CPU suffix handoff accepted the live512 prefix:
  `cgp accepted-prefix suffix-submit ... rows=192 ...
  live_k_dim=512 k_dim=2048 suffix_jobs=12`.
- Completion proof:
  `cgp accepted-prefix complete ... rows=192 ... live_k_dim=512
  suffix_jobs=12 total_cpu_jobs=74 first_output_bits=0xBD2A32C8
  last_output_bits=0x3D81B1A6`.
- Autorun timing proof from this correctness-heavy boot:
  `lumen: cgp autorun done prompt="hi" prompt_tokens=1 first_token=11424ms
  total=11425ms final_next_token=446`.

Follow-up live512 row-cap proof, 2026-05-25:

- The T6.2/T6.3 row-block dispatch cap was raised from `8` to `32`.  This is a
  CPU-side coordination change: each 8-row block is still restaged into the
  same known-good small artifact body, so no EU instruction change was needed.
- Runtime proof:
  `t62_submitted_blocks=96`, `t62_finished_blocks=96`,
  `t62_compare_ok_blocks=96`, and `t62_compared_rows=768`.
- Each windowed rung through live512 matched the same 96 row blocks:
  `t611_submitted_blocks=96`, `t611_compare_ok_blocks=96`,
  `t611_compared_rows=768`, and `t611_disabled_after_failure=0`.
- The extra live176..live512 ladder reported:
  `twindow_staged_blocks=2112`, `twindow_submitted_blocks=2112`,
  `twindow_finished_blocks=2112`, `twindow_compare_ok_blocks=2112`,
  `twindow_compared_rows=16896`, `twindow_disabled_after_failure=0`,
  `twindow_frontier_rung=33`, and `twindow_frontier_live_k=512`.
- The aggregate step 16 frontier is now:
  `cgp_mode=accepted-prefix`, `cgp_prefix_rows=768`,
  `cgp_prefix_live_k=512`, `row_block_cap=32`,
  `action=offer-accepted-prefix`.
- CPU suffix handoff accepted the 768-row live512 prefix:
  `cgp accepted-prefix suffix-submit ... rows=768 first_row=0 last_row=767
  live_k_dim=512 k_dim=2048 suffix_jobs=48`.
- Completion proof:
  `cgp accepted-prefix complete ... rows=768 ... live_k_dim=512
  suffix_jobs=48 total_cpu_jobs=86 first_output_bits=0xBD2A32C8
  last_output_bits=0x3C1EF722`.
- Autorun timing proof from this intentionally exhaustive boot:
  `lumen: cgp autorun done prompt="hi" prompt_tokens=1 first_token=40567ms
  total=40567ms final_next_token=446`.

Interpretation: local GPGPU has now proved a real Lumen BF16 matvec prefix over
768 rows and 512 K lanes, with GPU output bits matching CPU reference at every
16-lane window rung from live32 through live512.  This is still a prefix proof,
not full model matvec ownership: the CPU/AP suffix completes lanes
`512..2048`, and CPU/AP keeps final output ownership.

Next useful directions:

- Raise row coverage beyond three armed tiles.  The live512 path now consumes
  the full 32 row blocks available inside each currently armed 256-row tile;
  the next row lever is arena/tile arming, not the old row-block cap.
- Revisit true larger live-k artifact generation separately; the direct
  live96/live128 bodies are currently non-retiring on hardware.
- Reduce per-block proof overhead once the rung is stable; the current logs are
  deliberately verbose and correctness-first.

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

The local GPU role is therefore proof/pilot only.  It may run a small set of
actual-work tile records, but it must not receive half the row range or become a
result owner.  The CPU/AP path continues to compute the full local result, while
the GPU path acts like a guarded tile worker whose current job is proving staged
loads, packed-BF16 math, and record-local output writes.

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

The rule for this phase is still CPU-reference-first: the GPU may write proof
results into tile-record output slots, but CPU/AP keeps ownership of real
inference output until a later rung explicitly changes that contract.

## Trusted After Frontier Proof

Follow-up trusted fast-path proof, 2026-05-25:

- The first logged Lumen prompt matvec still runs the full CPU-reference ladder.
  That pass latches the current frontier when it reaches `twindow_frontier_rung=33`
  and `twindow_frontier_live_k=512`.
- Later Lumen prompt matvec calls no longer return an empty GPU contribution
  just because the proof log was already emitted.  They now reuse the latched
  frontier and run a trusted path with output validation disabled after the
  proof pass.
- Runtime proof:
  `lumen-gpu-proof: trusted-window-fast-path ... trusted_rung=33
  trusted_live_k=512 submitted_blocks=3072 accepted_blocks=96
  accepted_rows=768 failed=0 validation=disabled-after-frontier-proof`.
- The CPU/AP suffix still accepts the same contribution:
  `cgp accepted-prefix suffix-submit ... rows=768 first_row=0 last_row=767
  live_k_dim=512 k_dim=2048 suffix_jobs=48`.

Interpretation: this fixes the earlier testing handicap where the GPU prefix
was proved once and then effectively absent from later timed matvec calls.  The
GPU path is now active after proof, but it is still submit-heavy: a single
2048x2048 matvec needs 96 row blocks times the T6.2/T6.3 bootstrap plus the
window ladder through live512, reported as `submitted_blocks=3072`.  The next
speed lever is therefore batching/coalescing the windowed work or generating a
coarser result-owner artifact, not adding more per-window validation.

Follow-up trusted quiet-path proof, 2026-05-25:

- The strict first pass still proves the full frontier:
  `twindow_frontier_rung=33`, `twindow_frontier_live_k=512`,
  `cgp_prefix_rows=768`, and `cgp_prefix_live_k=512`.
- The post-frontier path now uses trusted staging for row/window copies and
  trusted T6.2/T6.3/window submits. It keeps retire, finish-marker,
  lane-dispatch, and output readback, but skips CPU expected-word replay after
  the strict proof.
- Runtime proof:
  `intel/gpgpu: t6-2-lane-indexed-live16-partial ...
  validation=trusted-frontier reason=trusted-frontier-readback
  program_source=gfx12-t6-windowed-trusted-quiet-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`.
- Runtime accepted-prefix proof:
  `lumen-gpu-proof: trusted-window-fast-path ... trusted_rung=33
  trusted_live_k=512 submitted_blocks=3072 accepted_blocks=96
  accepted_rows=768 failed=0 validation=disabled-after-frontier-proof`.
- The quiet submit label suppresses per-submit `execlist-start` and
  `batch-submit-proof` log spam for trusted window work. The remaining repeated
  proof surface is the high-level CPU suffix handoff:
  `cgp accepted-prefix suffix-submit ... rows=768 ... live_k_dim=512`.

Interpretation: validation and log replay are no longer the main trusted-path
handicap. The post-proof path is now a real trusted GPU prefix, but the work is
still fragmented into `submitted_blocks=3072`. The next material speed lever is
to reduce submit count, either by coalescing row blocks/windows into fewer
render submissions or by replacing the window ladder with a coarser retiring
artifact.

Follow-up trusted retire-only intermediate proof, 2026-05-25:

- The trusted path now keeps output-word readback only for the final accepted
  live512 rung of each row block. Intermediate trusted submits check retire,
  finish marker, and lane dispatch, but do not flush/read the output words that
  will be overwritten by the next window.
- Runtime proof:
  `lumen-gpu-proof: trusted-window-fast-path ... trusted_rung=33
  trusted_live_k=512 submitted_blocks=3072 accepted_blocks=96
  accepted_rows=768 skipped_output_readbacks=2976 failed=0
  validation=disabled-after-frontier-proof`.
- Final-readback proof remains present:
  `intel/gpgpu: t6-windowed-accum16-partial ... validation=trusted-frontier
  reason=trusted-frontier-readback ... live_k_dim=512`.
- CPU suffix handoff still accepts the same prefix:
  `cgp accepted-prefix suffix-submit ... rows=768 ... live_k_dim=512`.

Interpretation: this removes the remaining per-window GPU output readback tax
after the frontier is proven. It does not reduce render submissions yet:
`submitted_blocks=3072` is unchanged. The next backend-only step is therefore a
real submit coalescer, most likely batching the same window across the three
armed tile records before attempting wider artifact changes.

Follow-up log-trim proof, 2026-05-25:

- Successful strict-ladder detail is quiet by default behind
  `LOG_STRICT_WINDOW_LADDER_DETAILS=false`; the aggregate `director-step
  step=16` frontier summary remains live.
- Intel-side successful `windowed-accum16` shape/submit logs are suppressed:
  surface state, kernel shape, walker, execlist, batch-submit, and per-submit
  CPU-reference success lines stay quiet unless a submit fails or mismatches.
- Successful `tile-accum16-window-stage` and `one-tile-stage` staging proofs are
  one-shot; failures still log.
- Runtime log-count proof after `make iso`:
  `shape=0`, `director_detail=0`, `partial_cpu_ref=0`,
  `tile_window_stage=1`, and `trusted_fast=1`.
- Runtime frontier proof is preserved:
  `director-step step=16 ... twindow_frontier_rung=33
  twindow_frontier_live_k=512 ... cgp_prefix_rows=768 cgp_prefix_live_k=512`.
- Runtime trusted-path proof is preserved:
  `trusted-window-fast-path ... submitted_blocks=3072 accepted_blocks=96
  accepted_rows=768 skipped_output_readbacks=2976 failed=0`.

Interpretation: the ladder no longer floods the serial log with one line per
window/rung/block. The remaining useful facts are the one-shot staging sanity
line, the aggregate strict frontier summary, and the trusted fast-path summary.
Failures and mismatches are still loud.

Additional trim, same session:

- Successful T6.2/T6.3 strict-rung details are now behind the same quiet ladder
  policy. This suppresses the row-block plan/stage/partial director chatter plus
  the Intel surface/shape/submit/partial-success lines for
  `t6-2-lane-indexed` and `t6-3-accum16-hi`.
- The strict proof still reports the aggregate frontier line:
  `twindow_frontier_live_k=512`, `cgp_prefix_rows=768`,
  `cgp_prefix_live_k=512`.
- The trusted path still reports one summary line:
  `trusted-window-fast-path ... trusted_live_k=512 accepted_rows=768`.
- `one-tile-readback` now treats `cpu_expected_bits=0` as a valid zero-output
  staging case, and expected non-eligible `k_dim=5632` matrices no longer log as
  `fix-one-tile-stage` failures.

Interpretation: after this ISO, a busy prompt should look like actual generation
work plus the frontier/trusted summaries, not like the entire early proof ladder
is being re-debugged every time.

Boot readback after the additional trim:

- `latest.log` reached the strict frontier in 695 lines, with
  `twindow_frontier_live_k=512`, `cgp_prefix_rows=768`, and
  `cgp_prefix_live_k=512` still present.
- `trusted-window-fast-path` is still present with `accepted_rows=768`,
  `trusted_live_k=512`, and `failed=0`.
- The previous successful T6.2/T6.3 surface/shape/submit/director flood is
  gone from the checked log.
- No `k-dim-not-tile-k` or `one-tile-readback readback_ok=0` noise was present
  in that readback.

Interpretation: the current "busy" shell state is ordinary active Lumen work:
hybrid GPU-prefix plus CPU-suffix matvecs are running. It is not evidence of a
lost EU thread, a failed ladder rung, or the full proof ladder restarting from a
broken state.

## T8 Batch2 Row-Block Retire Probe

Status: rung 0 proven on 2026-05-25.

Purpose: before generating a new group-ID row-block artifact, prove that the
current walker path can retire a two-group dispatch under the Lumen matvec
arena path.

Artifact label:

- `gfx12-t8-batch2-rowblock-live16-uses-t62-localid-retire-probe`

This intentionally reuses the reliable T6.2 local-lane body. It is a walker
scale/retire probe, not the final T8 ownership artifact.

Fresh boot proof from `bld/baremetal-logs/latest.log`:

- `groups=2x1x1`
- `expected_hw_threads=2`
- `expected_lane_dispatch=16`
- `observed_lane_dispatch=16`
- `finished=1`
- `finish_marker=0xC0DE7732`
- `readback_ok=1`
- `compare_ok=1`
- `reason=t8-batch2-rowblock-live16-retired`
- `action=advance-frontier`
- `next=t8-groupid-distinct-row-output`

Interpretation: the current command/walker path can submit and retire two SIMD8
payload groups in this Lumen arena shape. This removes the immediate worry that
T8 batching is blocked by the walker itself.

Important caveat: this still does not prove distinct row-block ownership. The
probe body still addresses output rows through local lane identity, so the next
artifact must switch the row/block selection to a true group-ID based scheme and
write separate row-block outputs before it can reduce the `submitted_blocks=3072`
cost of the trusted live512 path.

Follow-up T8 group-ID artifact, same session:

- Embedded the original row-indexed native oracle as
  `gfx12-t8-groupid-live16-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`.
- Applied the proven safe HDC/EOT tail locally: HDC store source remains intact
  and TS EOT uses `g126` / `0x70007E0C`.
- Runtime shape proof:
  `code_bytes=0x320`, `code_words=200`, `send_word_off=186`,
  `artifact_kind=T8GroupidLive16Bf16DotThenHdc1StoreThenThreadSpawnerEot`.
- Runtime walker proof:
  `groups=2`, `expected_lane_dispatch=16`, `observed_lane_dispatch=16`,
  `finished=1`, `finish_marker=0xC0DE7732`.
- Runtime compare result:
  `readback_ok=0`, `compare_ok=0`, `reason=partial-output-mismatch`,
  `output_words=[0x00000000,0x00000000,...]` while CPU expected row 0 and row 1
  were nonzero.

Debug result:

- A full 256-dword output-tile clear plus scan showed `nonzero_dwords=0`.
  Therefore the first mismatch was not "wrong row value landed nearby"; the
  artifact retired but did not write anywhere in the intended output tile.
- The row-indexed native oracle starts by forming the row from the hardware
  group id plus CURBE dword 4:
  `row = WorkGroupID.x + g1.4`.
- TRUEOS was still using the generic dummy CURBE poison pattern for every
  dword: `0x5A5A5A5A`.
- That made the row index enormous, so the artifact computed the right style of
  store from a nonsensical base row and wrote outside the scanned tile surface.

Runtime/payload fix:

- `write_gpgpu_dummy_curbe` now keeps the poison pattern for most CURBE dwords
  but writes dword 4 as zero.
- This preserves the "unknown payload fields are obvious" debugging property
  while making the Mesa-style base workgroup X offset semantically zero.

Re-proof after `make iso`:

- `t8-groupid-live16-distinct-row-partial`
- `submitted=1`
- `finished=1`
- `readback_ok=1`
- `compare_ok=1`
- `groups=2`
- `expected_lane_dispatch=16`
- `observed_lane_dispatch=16`
- `output_words=[0x36B81930,0xB8026954,...]`
- `expected_words=[0x36B81930,0xB8026954,...]`
- `finish_marker=0xC0DE7732`
- `reason=t8-groupid-live16-distinct-rows-written`

Focused output scan from the same boot:

- `scan_dwords=256`
- `cleared_dwords=256`
- `nonzero_dwords=2`
- `first_nonzero=0`
- `row0_hits_lo64=0x0000000000000001`
- `row1_hits_lo64=0x0000000000000002`
- `row0_first=0`
- `row1_first=1`

Interpretation: the T8 group-ID artifact is legitimate.  The mismatch was a
runtime payload contract bug, not a bad shader body and not an EOT/retire
failure.  With CURBE dword 4 fixed to zero, the artifact retires and writes the
two distinct group rows exactly where expected.

Next reasonable targets:

- Scale T8 group-ID row ownership past the 2-row proof toward larger row-block
  ownership.
- Keep the focused T8 scan while this frontier is being scaled; it should stay
  a single proof line, not a return to the earlier ladder log flood.

T8 row-scale follow-up, same session:

Runtime change:

- The T8 group-ID submit now dispatches `row_count` groups instead of a fixed
  two groups.
- The accepted validation shape for this artifact is raised to 8 rows.
- The Lumen-side proof ladder runs the same live16 math contract at 2, 4, and 8
  distinct rows, stopping at the first failing rung.

Boot proof after `make iso`:

- `rung_rows=2`: `readback_ok=1`, `compare_ok=1`,
  `expected_lane_dispatch=16`, `observed_lane_dispatch=16`,
  `nonzero_dwords=2`.
- `rung_rows=4`: `readback_ok=1`, `compare_ok=1`,
  `expected_lane_dispatch=32`, `observed_lane_dispatch=32`,
  `nonzero_dwords=4`.
- `rung_rows=8`: `readback_ok=1`, `compare_ok=1`,
  `expected_lane_dispatch=64`, `observed_lane_dispatch=64`,
  `nonzero_dwords=8`.
- Summary:
  `director-step step=43 ... frontier_rows=8 live_k_dim=16
  last_dispatch_delta=64 failed=0 action=advance-frontier
  next=coalesce-t8-rowblock-submit`.

Interpretation: T8 group-ID row ownership now scales from the minimal 2-row
proof to a full 8-row live16 row block.  This is still only the prefix math
window, but it is the first useful coalescing target: one T8 dispatch can own the
same eight live16 row outputs that previously required per-row/local-lane style
reasoning.  The next useful backend rung is to coalesce trusted row-block
submits around this T8 ownership shape, then decide whether to extend row count
or live-k.

T8 submit-coalescing accounting proof, same session:

Runtime change:

- Added a single trusted-path accounting log after the T8 row-scale proof and
  before the existing trusted-window-fast-path summary.
- The log does not change execution yet; it proves the current submit shape and
  the target reduction before we mutate the dispatch model.

Boot proof after `make iso`:

- `director-step step=44 ... mode=t8-coalesce-submit-accounting`
- `current_model=per-row-block-per-live-window`
- `armed_tiles=3`
- `tile_rows=256`
- `row_block_rows=8`
- `row_blocks_per_tile=32`
- `accepted_blocks=96`
- `accepted_rows=768`
- `submitted_blocks=3072`
- `submit_windows_per_block=32`
- `live_window=16`
- `trusted_live_k=512`
- `live_windows_to_trusted=32`
- `t8_frontier_rows=8`
- `row_blocks_per_t8_submit=1`
- `projected_submits_at_current_t8=3072`
- `ideal_tile_projected_submits=96`
- `ideal_reduction_x=32`

Interpretation: the 3072-submit cost is now explicitly proven as
`3 tiles * 32 row-blocks/tile * 32 live16 windows`.  The current T8 frontier is
only one 8-row block, so it cannot reduce submit count by itself yet; it proves
the row addressing contract needed for coalescing.  The next real submit-count
lever is either raising T8 row ownership above 8 rows, or teaching the trusted
path to issue one coalesced tile/window dispatch shape.  The theoretical target
for the current 768-row, live512 prefix is 96 submits, a 32x reduction in this
specific GPGPU submit loop.

T8 16-row frontier proof, next boot:

Runtime change:

- The T8 row-scale ladder now includes a 16-row rung.
- The partial matvec proof/readback path was widened to keep the first 16 output
  words for the T8 group-ID artifact, while preserving the existing 8-row proof
  path for smaller rungs.
- The trusted-path accounting now records the actually proven T8 frontier rows,
  so later submit projections do not overstate the runtime state.

Boot proof after `make iso`:

- `rung_rows=2`: `readback_ok=1`, `compare_ok=1`,
  `expected_lane_dispatch=16`, `observed_lane_dispatch=16`,
  `nonzero_dwords=2`.
- `rung_rows=4`: `readback_ok=1`, `compare_ok=1`,
  `expected_lane_dispatch=32`, `observed_lane_dispatch=32`,
  `nonzero_dwords=4`.
- `rung_rows=8`: `readback_ok=1`, `compare_ok=1`,
  `expected_lane_dispatch=64`, `observed_lane_dispatch=64`,
  `nonzero_dwords=8`.
- `rung_rows=16`: `readback_ok=1`, `compare_ok=1`,
  `expected_lane_dispatch=128`, `observed_lane_dispatch=128`,
  `nonzero_dwords=16`, `finish_marker=0xC0DE7732`.
- Summary:
  `director-step step=43 ... frontier_rows=16 live_k_dim=16
  last_dispatch_delta=128 failed=0 action=advance-frontier
  next=coalesce-t8-rowblock-submit`.
- Submit accounting:
  `director-step step=44 ... t8_frontier_rows=16
  row_blocks_per_t8_submit=2 projected_submits_at_current_t8=1536
  ideal_tile_projected_submits=96 ideal_reduction_x=32`.

Interpretation: the same T8 artifact now owns two 8-row blocks per dispatch at
the live16 frontier.  This does not yet change the hot-path submit loop, which
still reports `submitted_blocks=3072`, but it proves the next coalescing target:
the current artifact shape can cut the projected trusted-window submit count from
3072 to 1536 before any tile-wide coalescing work.  The next useful runtime
change is to issue the trusted path in this 16-row T8 shape instead of only
accounting for it.
