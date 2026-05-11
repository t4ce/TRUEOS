# T6.3 split16 live32 candidate

Candidate source:

- `.codex_tmp/t6_3_split16_live32_trueos_arena_bf16_unpack.comp`

Decision:

- Prefer **live32 split into two live16 accumulations** as the next candidate.

Reasoning:

- A live24 breakpoint would be useful only if live32 cannot compile or cannot
  retire. The current tree already has a T6.3 live32 source, oracle workload,
  trueos-eu constants, and oracle result proof, so live24 is no longer the best
  next discriminator.
- A live32 store-sentinel would isolate row/lane selection plus output store, but
  it would not prove live32 BF16 row-block math.
- The split16 source keeps the T6.3 contract: one SIMD8 workgroup,
  `gl_LocalInvocationID.x` selects one of eight row slots, `x[0..31]` are f32,
  each row is packed BF16, and outputs land at `OUT_BASE + row`.
- Expressing the dot as `dot16(row, 0) + dot16(row, 16)` gives Mesa a candidate
  with shorter live ranges than the monolithic 32-term T6.3 source while still
  proving live32 math if the generated artifact writes the same eight outputs.

Expected oracle workload:

```sh
TRUEOS_ORACLE_SHADER_SOURCE=.codex_tmp/t6_3_split16_live32_trueos_arena_bf16_unpack.comp \
TRUEOS_ORACLE_SHADER_NAME=t6_3_split16_live32_trueos_arena_bf16_unpack \
TRUEOS_ORACLE_WORKLOAD=t6-3-lane-indexed-live32-trueos-arena-packed-bf16 \
TRUEOS_ORACLE_LOG_DIR=.codex_tmp/intel_userland_oracle/t6-3-split16-live32-trueos-arena-bf16-unpack \
TRUEOS_ORACLE_REQUIRE_HW=0 \
bash tools/intel_userland_oracle/run_oracle.sh
```

Expected outputs are unchanged from T6.3:

- row 0: `0x44040000`
- row 7: `0x45840000`
- `verified=1 rows=8 live_k=32`

Do not promote this into `crates/trueos-eu` until the generated native bytes are
inspected. If the split source still compiles to the same high-pressure shape as
the monolithic T6.3 source, the next diagnostic candidate should be a live32
store-sentinel.

## Local check

The source compiled to SPIR-V with:

```sh
glslangValidator -V \
  .codex_tmp/t6_3_split16_live32_trueos_arena_bf16_unpack.comp \
  -o /tmp/t6_3_split16_live32_trueos_arena_bf16_unpack.comp.spv
```

The existing T6.3 oracle workload also accepted it:

```text
SIMD8 shader: 104 instructions. 0 loops. 540 cycles. 0:0 spills:fills,
8 sends, scheduled with mode top-down. Promoted 0 constants.
Non-SSA regs (after NIR): 5. Compacted 1744 to 1472 bytes (16%)

oracle-app: t6-3-lane-indexed-live32-trueos-arena-packed-bf16 verified=1
rows=8 live_k=32 first_bits=0x44040000 last_bits=0x45840000
```

This means the split source is semantically good, but not automatically smaller
than the current monolithic T6.3 constant. Its value is as a lower-live-range
candidate; if native extraction shows it is too large or less friendly for the
TRUEOS walker, use live32 store-sentinel next.
