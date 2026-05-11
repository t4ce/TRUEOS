# T6.3 lane-indexed live32 packed-BF16 artifact contract

Program name:

`gfx12-t6-3-lane-indexed-live32-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`

Preserved generated artifacts:

- `.codex_tmp/t6_3_lane_indexed_live32_trueos_arena_bf16_unpack.comp`
- `.codex_tmp/intel_userland_oracle/t6-3-lane-indexed-live32-trueos-arena-bf16-unpack/log.txt`
- `.codex_tmp/intel_userland_oracle/t6-3-lane-indexed-live32-trueos-arena-bf16-unpack/dumps/000614_pre_exec_handle_3_off_0x0_len_0x200000.bin`

T6.3 keeps the TRUEOS tile-record layout:

- `x` f32 words at record-local `+0x0`
- packed BF16 rows at record-local `+0x2000`
- output tile at record-local `+0x102000`

Runtime reason:

- T6.2 proved one SIMD8 workgroup can produce eight row-indexed live16 partial
  outputs with `gl_LocalInvocationID.x`.
- T6.3 preserves that row-block dispatch contract and only widens the math
  prefix to live32.
- This keeps the row ownership and live-k growth as separate ladder steps.

Generation/verification note:

- Vulkan oracle passed on 2026-05-11:
  `verified=1 rows=8 live_k=32 first_bits=0x44040000 last_bits=0x45840000`.
- Store send exdesc dword is `357`; embedded native word count is `368`.
- The runtime only runs T6.3 after the matching T6.2 row-block compare succeeds.
