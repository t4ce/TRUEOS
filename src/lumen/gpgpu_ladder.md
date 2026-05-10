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

Status: measured on 2026-05-10 08:47 boot log.

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
- The current proven upper rung for this artifact remains 186 SIMD8 payloads,
  or 1488 SIMD8 lane dispatches.

This narrows the cliff to the range `(186, 224]` for the current static
DP4A/HDC-store/EOT artifact shape.

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

Status: implemented as a CPU-authoritative probe scaffold.

The old T3 artifact is preserved as the active GPU proof:
`gfx12-static-dp4a-hdc1-stateless-store-then-ts-eot`.

A T4 catalog rung now exists in `trueos-eu`:
`gfx12-t4-live-x-static-dp4a-requirement-hdc1-store-then-ts-eot`.
It deliberately reuses the proven T3 EU shell for now, but has its own artifact
kind so the next live-load kernel can be switched in without losing the known
good static DP4A/HDC/EOT baseline.

At the Lumen callsite, `burn_baby::matvec_rowmajor_bf16` now emits a one-shot
`lumen-gpu-shadow: director-step step=4 mode=t4-live-row-probe` record once the
static GPU artifact is proven.  That record includes:

- live `x` pointer, byte size, and checksum
- resolved matrix manifest id/name hash when available
- row-0 BF16 pointer and checksum
- CPU-authoritative row-0 dot bits
- CPU-authoritative `live x[0..4] * static [1,2,3,4]` bits

This proves the exact tuple the first real T4 GPU load kernel must reproduce:
manifest row + live activation vector + expected output.  It still logs
`gpu_submission=0`, so it does not overclaim live GPU memory loads yet.
