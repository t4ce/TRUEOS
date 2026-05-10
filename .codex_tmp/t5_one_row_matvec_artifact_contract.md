# T5 one-row matvec artifact contract

Program name:

`gfx12-t5-one-row-live-bf16-matvec-hdc1-stateless-store-then-ts-eot`

Current small-step program name:

`gfx12-t5-small-live4-bf16-dot-hdc1-stateless-store-then-ts-eot`

Preserved generated artifacts:

- `.codex_tmp/t5_small_live4.comp`
- `.codex_tmp/t5_small_live4/t5_small_live4.comp.spv`
- `.codex_tmp/t5_small_live4/t5_small_live4.comp.spvasm`
- `.codex_tmp/t5_small_live4_trueos_arena.comp`
- `.codex_tmp/t5_small_live4_trueos_arena/t5_small_live4_trueos_arena.comp.spv`
- `.codex_tmp/t5_small_live4_trueos_arena/t5_small_live4_trueos_arena.comp.spvasm`
- `.codex_tmp/t5_small_live4_trueos_arena/oracle_native/mesa_cache_cc_native.bin`

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
- The current arithmetic result is intentionally held at proof status:
  `gpu_value=0xB9BAA564` matches the shader's packed-word BF16 view, while
  `cpu_expected_bits=0x3AAA10F6` is the contiguous BF16-lane CPU reference.
- `live_k_dim=4`
- `requires_live_gpu_load=1`
- `does_not_prove=full_model_matvec`
- Next shader work is `bf16-packed-half-unpack`, not another dispatch-scale
  question.

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
