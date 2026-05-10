# T6.2 row-indexed live16 packed-BF16 artifact contract

Program name:

`gfx12-t6-2-row-indexed-live16-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`

Preserved generated artifacts:

- `.codex_tmp/t6_2_row_indexed_live16_trueos_arena_bf16_unpack.comp`
- `.codex_tmp/t6_2_row_indexed_live16_trueos_arena_bf16_unpack/t6_2_row_indexed_live16_trueos_arena_bf16_unpack.comp.spv`
- `.codex_tmp/t6_2_row_indexed_live16_trueos_arena_bf16_unpack/t6_2_row_indexed_live16_trueos_arena_bf16_unpack.comp.spvasm`
- `.codex_tmp/intel_userland_oracle/t6-2-row-indexed-live16-trueos-arena-bf16-unpack/log.txt`

T6.2 keeps the TRUEOS tile-record layout:

- `x` f32 words at record-local `+0x0`
- packed BF16 row tile at record-local `+0x2000`
- output tile at record-local `+0x102000`

What changes from T6.1:

- `gl_WorkGroupID.x` selects the row inside the tile record.
- `gl_WorkGroupID.x` also selects the output slot.
- Each group computes a live16 packed-BF16 partial dot for one row.
- The first runtime proof uses 8 rows and compares all 8 output dwords.

Generation/verification note:

- Vulkan oracle passed on 2026-05-11:
  `verified=1 rows=8 live_k=16 first_bits=0x43080000 last_bits=0x44880000`.
- Mesa reported `SIMD8 shader: 56 instructions`, `4 sends`, and
  `Compacted 928 to 800 bytes`.
- The extracted native EU program is preserved in
  `crates/trueos-eu/src/gfx12.rs` as
  `T62_ROW_INDEXED_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT`.
