# T5 one-row matvec artifact contract

Program name:

`gfx12-t5-one-row-live-bf16-matvec-hdc1-stateless-store-then-ts-eot`

Current small-step program name:

`gfx12-t5-small-live4-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`

Preserved generated artifacts:

- `.codex_tmp/t5_small_live4.comp`
- `.codex_tmp/t5_small_live4/t5_small_live4.comp.spv`
- `.codex_tmp/t5_small_live4/t5_small_live4.comp.spvasm`
- `.codex_tmp/t5_small_live4_trueos_arena.comp`
- `.codex_tmp/t5_small_live4_trueos_arena/t5_small_live4_trueos_arena.comp.spv`
- `.codex_tmp/t5_small_live4_trueos_arena/t5_small_live4_trueos_arena.comp.spvasm`
- `.codex_tmp/t5_small_live4_trueos_arena/oracle_native/mesa_cache_cc_native.bin`
- `.codex_tmp/t5_small_live4_trueos_arena_bf16_unpack.comp`
- `.codex_tmp/t5_small_live4_trueos_arena_bf16_unpack/t5_small_live4_trueos_arena_bf16_unpack.comp.spv`
- `.codex_tmp/t5_small_live4_trueos_arena_bf16_unpack/t5_small_live4_trueos_arena_bf16_unpack.comp.spvasm`
- `.codex_tmp/intel_userland_oracle/t5-small-live4-trueos-arena-bf16-unpack/log.txt`

T5 is the first GPGPU ladder rung that must prove a real model calculation.
T47/T48 are preserved controls and cannot satisfy T5:

- T47 proves an EU thread can store a sentinel into the staged one-tile output.
- T48 proves the output compare/readback path with a DP4A echo value.
- T5-small must load the staged live `x[0..4]` f32 vector, load the staged BF16
  row values `w[0..4]`, multiply/reduce the four-element partial dot, store the
  output, and match the CPU reference bits for that same four-element slice.

Current boot-visible T5 state, as of the `make iso` loop ending in
`bld/baremetal-logs/latest.log` on 2026-05-10:

- T5-small is now wired as the hot artifact.
- It binds the SSBO-style HDC surface to the TRUEOS GPGPU tile arena base.
- It expects `x` at arena `+0x0`, BF16 row at arena `+0x2000`, and output at
  arena `+0x102000`.
- The load echo proves the shader reads the staged live operands:
  `load_echo_ok=1`, `x_echo_ok=1`, `row_echo_ok=1`.
- The scale ladder now cleanly retires through groups
  `1,2,4,8,16,32,64,128,186`, with the final rung showing
  `observed_lane_dispatch=1488`.
- The previous word-view artifact is preserved in code as
  `gfx12-t5-small-live4-word-view-bf16-dot-hdc1-stateless-store-then-ts-eot`.
  It proved load/math/store, but read row BF16 lanes `[0,2,4,6]`.
- The new hot artifact unpacks packed BF16 halves so the shader reads row lanes
  `[0,1,2,3]` and compares against the contiguous CPU reference.
- `live_k_dim=4`
- `requires_live_gpu_load=1`
- `does_not_prove=full_model_matvec`
- Source-level Vulkan oracle for the packed-half shader passed with
  `verified=1 expected_bits=0x41F00000 observed_bits=0x41F00000`.
- TRUEOS boot readback for the packed-half native artifact passed:
  `t5-input-summary` showed
  `gpu=0x3AAA10F6 cpu_expected=0x3AAA10F6 cpu_direct=0x3AAA10F6`
  with `gpu_matches_direct=1` and `gpu_matches_legacy_word_view=0`.
- The packed-half scale ladder retired cleanly through groups
  `1,2,4,8,16,32,64,128,186`; final rung:
  `observed_lane_dispatch=1488`, `gpu_matches_packed_bf16=1`,
  `failure_class=t5-live4-packed-bf16-proven`.
- A later high-scale run on 2026-05-11 established the current clean cap for
  this T5 live4 artifact:
  `4096` groups = `32768` SIMD8 lane dispatches, clean retire and correct
  packed-BF16 result.
- The same run found the first non-clean rung at
  `6144` groups = `49152` SIMD8 lane dispatches.  It still wrote the correct
  result, but did not retire cleanly:
  `reason=submit-not-finished`, `retired=0`.
- Keep the runtime T5 live4 ladder capped at `[4096]`.  This is a proof-frontier
  cap, not a throughput tuning knob; revisit it only when the kernel grows,
  the CGP queueing model changes, or completion/retire logic changes.
- The actual-work tile-frontier proof now stages rows `0`, `256`, and `512`
  from the live Lumen matvec.  All three compare correctly for the T5 live4
  slice with `compare_ok_tiles=3`, while `output_owner=cpu-ap`.
- Lumen step 9 now reports
  `submitted=1 finished=1 readback_ok=1 compare_ok=1 reason=t5-live4-written`
  for `gfx12-t5-small-live4-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`.
- Next validation question is scaling live K or row count.

Superseded rerun note:

- One early readback caught a storage/transport timeout before the T5 shader
  rung. A later restart reached T5 and repeated the packed-half proof cleanly,
  including load echo, packed-half input summary, scale to 186 groups, and
  `readback_ok=1 compare_ok=1`. Treat the later clean proof as current.

The first executable form is intentionally `live_k_dim=4`; the full 2048-wide
row comes later after the tiny slice proves real GPU-side input reads in
TRUEOS, not only in the Vulkan oracle.

Dedicated loop for this rung:

1. Run `make iso`.
2. Wait 60 seconds after boot/log drain starts.
3. Read back:
   `rg -n "t5-load-echo|t5-input-summary|t5-live4-scale-proof|t5-small-live4-bf16-dot|lumen-gpu-proof: director-step step=9|prefill progress|first-token" bld/baremetal-logs/latest.log`
4. Treat a clean rung as: load echo OK, all requested lane counts match,
   finish marker `0xC0DE7732`, and no `t5-live4-scale-ladder stop` line.
