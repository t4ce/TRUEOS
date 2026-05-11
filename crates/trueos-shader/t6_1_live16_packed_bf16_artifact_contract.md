# T6.1 live16 packed-BF16 artifact contract

Program name:

`gfx12-t6-1-live16-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`

Preserved generated artifacts:

- `.codex_tmp/t6_1_live16_trueos_arena_bf16_unpack.comp`
- `.codex_tmp/t6_1_live16_trueos_arena_bf16_unpack/t6_1_live16_trueos_arena_bf16_unpack.comp.spv`
- `.codex_tmp/t6_1_live16_trueos_arena_bf16_unpack/t6_1_live16_trueos_arena_bf16_unpack.comp.spvasm`
- `.codex_tmp/intel_userland_oracle/t6-1-live16-trueos-arena-bf16-unpack/log.txt`

T6.1 keeps the T5/T6 TRUEOS tile-record contract:

- one SSBO/HDC surface bound to the active TRUEOS GPGPU tile record
- `x` f32 words at record-local `+0x0`
- packed BF16 row words at record-local `+0x2000`
- output words at record-local `+0x102000`

What changes from T6:

- `live_k_dim` grows from `8` to `16`.
- The shader unpacks packed BF16 row lanes `[0..15]` from eight row dwords.
- The oracle input uses `x = [1..16]` and row BF16 lanes `[1..16]`.
- The expected result is `1496.0f`, bits `0x44BB0000`.
- The T6.1 sentinel is `0xC0DE7616`.

Generation/verification note:

- Vulkan oracle passed on 2026-05-11:
  `verified=1 expected_bits=0x44BB0000 observed_bits=0x44BB0000 live_k=16 sentinel=0xC0DE7616`.
- Mesa reported `SIMD8 shader: 46 instructions`, `4 sends`, and
  `Compacted 752 to 624 bytes`.
- The extracted native EU program is preserved in
  `crates/trueos-eu/src/gfx12.rs` as
  `T61_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT`.
