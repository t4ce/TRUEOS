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

Current boot-visible T5 state:

- T5-small is now wired as the hot artifact.
- It binds the SSBO-style HDC surface to the TRUEOS GPGPU tile arena base.
- It expects `x` at arena `+0x0`, BF16 row at arena `+0x2000`, and output at
  arena `+0x102000`.
- Success should show `submitted=1`, `finished=1`, `readback_ok=1`,
  `compare_ok=1`, and `reason=t5-live4-written` or
  `reason=t5-live4-written-no-ts-delta`.
- `live_k_dim=4`
- `requires_live_gpu_load=1`
- `does_not_prove=full_model_matvec`

The first executable form is intentionally `live_k_dim=4`; the full 2048-wide
row comes later after the tiny slice proves real GPU-side input reads in
TRUEOS, not only in the Vulkan oracle.
