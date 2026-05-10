# T5 one-row matvec artifact contract

Program name:

`gfx12-t5-one-row-live-bf16-matvec-hdc1-stateless-store-then-ts-eot`

Current small-step program name:

`gfx12-t5-small-live4-bf16-dot-hdc1-stateless-store-then-ts-eot`

Preserved generated artifacts:

- `.codex_tmp/t5_small_live4.comp`
- `.codex_tmp/t5_small_live4/t5_small_live4.comp.spv`
- `.codex_tmp/t5_small_live4/t5_small_live4.comp.spvasm`

T5 is the first GPGPU ladder rung that must prove a real model calculation.
T47/T48 are preserved controls and cannot satisfy T5:

- T47 proves an EU thread can store a sentinel into the staged one-tile output.
- T48 proves the output compare/readback path with a DP4A echo value.
- T5-small must load the staged live `x[0..4]` f32 vector, load the staged BF16
  row values `w[0..4]`, multiply/reduce the four-element partial dot, store the
  output, and match the CPU reference bits for that same four-element slice.

Current boot-visible T5 state:

- `submitted=0`
- `reason=eu-live-load-matvec-artifact-missing`
- `live_k_dim=4`
- `requires_live_gpu_load=1`
- `does_not_prove=model_matvec_or_gpu_live_load`

The next artifact should replace the empty T5 EU word slot with a reviewed live
load plus BF16 decode/multiply/reduce/store program. The first executable form
is intentionally `live_k_dim=4`; the full 2048-wide row comes later after the
tiny slice proves real GPU-side input reads.
