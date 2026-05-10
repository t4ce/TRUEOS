# T6 live8 packed-BF16 artifact contract

Program name:

`gfx12-t6-small-live8-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`

Preserved generated artifacts:

- `.codex_tmp/t6_small_live8_trueos_arena_bf16_unpack.comp`
- `.codex_tmp/t6_small_live8_trueos_arena_bf16_unpack/t6_small_live8_trueos_arena_bf16_unpack.comp.spv`
- `.codex_tmp/t6_small_live8_trueos_arena_bf16_unpack/t6_small_live8_trueos_arena_bf16_unpack.comp.spvasm`
- `.codex_tmp/intel_userland_oracle/t6-small-live8-trueos-arena-bf16-unpack/log.txt`

T6 starts from the green T5 contract:

- one SSBO/HDC surface bound to the active TRUEOS GPGPU tile record
- `x` f32 words at record-local `+0x0`
- packed BF16 row words at record-local `+0x2000`
- output words at record-local `+0x102000`

What changes from T5:

- `live_k_dim` grows from `4` to `8`.
- The shader unpacks BF16 row lanes `[0,1,2,3,4,5,6,7]` from four packed
  row dwords.
- The oracle input uses `x = [1,2,3,4,5,6,7,8]` and row BF16 lanes
  `[1,2,3,4,5,6,7,8]`.
- The expected result is `204.0f`, bits `0x434C0000`.
- The T6 sentinel is `0xC0DE7606`.

Generation/verification note:

- Vulkan oracle passed on 2026-05-10:
  `verified=1 expected_bits=0x434C0000 observed_bits=0x434C0000 live_k=8 sentinel=0xC0DE7606`.
- Mesa reported `SIMD8 shader: 30 instructions`, `4 sends`, and
  `Compacted 496 to 432 bytes`.
- The extracted native EU program is preserved in
  `crates/trueos-eu/src/gfx12.rs` as
  `T6_SMALL_LIVE8_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT`.

Runtime policy:

- T6 is now wired as the hot Lumen/GPGPU proof rung immediately after T5.
- T5 remains the guardrail: T6 runs only after the T5 live4 compare succeeds
  for the staged tile.
- Runtime labels are distinct:
  - `t5-small-live4-bf16-dot`
  - `t6-small-live8-bf16-dot`
  - `t6-live8-scale-proof`
  - `t6-actual-work-tiles`
- `GPGPU_T6_LIVE8_GROUP_X_DIM_LADDER` starts at `[4096]`, matching the clean T5
  retire cap until T6 has its own boot-log ladder history.
- Output remains CPU/AP-owned and proof-only; runtime now gives each armed tile
  a distinct arena record and binds the T6 surface to that record base.
- `T6.1` is reserved for the next generated live-k widening artifact.  The
  current distinct-output-row step reuses the preserved T6 bytes.
