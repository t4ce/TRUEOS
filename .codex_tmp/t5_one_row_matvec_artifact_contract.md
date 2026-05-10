# T5 one-row matvec artifact contract

Program name:

`gfx12-t5-one-row-live-bf16-matvec-hdc1-stateless-store-then-ts-eot`

T5 is the first GPGPU ladder rung that must prove a real model calculation.
T47/T48 are preserved controls and cannot satisfy T5:

- T47 proves an EU thread can store a sentinel into the staged one-tile output.
- T48 proves the output compare/readback path with a DP4A echo value.
- T5 must load the staged live `x[0..2048]` f32 vector, load the staged BF16 row,
  multiply/reduce the dot product, store the row output, and match the CPU
  reference bits.

Current boot-visible T5 state:

- `submitted=0`
- `reason=eu-live-load-matvec-artifact-missing`
- `requires_live_gpu_load=1`
- `does_not_prove=model_matvec_or_gpu_live_load`

The next artifact should replace the empty T5 EU word slot with a reviewed live
load plus BF16 decode/multiply/reduce/store program. Until that exists, the
driver logs T5 as a held rung instead of reusing the T48 echo path.
