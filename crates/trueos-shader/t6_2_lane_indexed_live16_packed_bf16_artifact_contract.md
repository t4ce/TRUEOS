# T6.2 lane-indexed live16 packed-BF16 artifact contract

Program name:

`gfx12-t6-2-lane-indexed-live16-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`

Preserved generated artifacts:

- `.codex_tmp/t6_2_lane_indexed_live16_trueos_arena_bf16_unpack.comp`
- `.codex_tmp/t6_2_lane_indexed_live16_trueos_arena_bf16_unpack/t6_2_lane_indexed_live16_trueos_arena_bf16_unpack.comp.spv`
- `.codex_tmp/t6_2_lane_indexed_live16_trueos_arena_bf16_unpack/t6_2_lane_indexed_live16_trueos_arena_bf16_unpack.comp.spvasm`
- `.codex_tmp/intel_userland_oracle/t6-2-lane-indexed-live16-trueos-arena-bf16-unpack/log.txt`

T6.2 keeps the TRUEOS tile-record layout:

- `x` f32 words at record-local `+0x0`
- packed BF16 rows at record-local `+0x2000`
- output tile at record-local `+0x102000`

Runtime reason:

- The first row-indexed artifact using `gl_WorkGroupID.x` retired but wrote no
  visible output on the TRUEOS legacy walker path.
- Older T5/T6/T6.1 logs had already shown the workgroup-id metadata lane was
  not trustworthy there.
- This replacement uses `gl_LocalInvocationID.x` with `local_size_x = 8`, so one
  SIMD8 workgroup computes eight live16 row partials.

Generation/verification note:

- Vulkan oracle passed on 2026-05-11:
  `verified=1 rows=8 live_k=16 first_bits=0x43080000 last_bits=0x44880000`.
- Mesa reported `SIMD8 shader: 56 instructions`, `5 sends`, and
  `Compacted 944 to 816 bytes`.
- Store send starts at dword `193`; embedded native word count is `204`.
