# T6.3 Accum16-Hi Live32 Artifact Contract

Artifact:

- Source: `.codex_tmp/t6_3_accum16_hi_live32_trueos_arena_bf16_unpack.comp`
- Program: `gfx12-t6-3-accum16-hi-live32-packed-bf16-dot-hdc1-stateless-store-then-ts-eot`
- Oracle log: `.codex_tmp/intel_userland_oracle/t6-3-accum16-hi-live32-trueos-arena-bf16-unpack/log.txt`
- Native dump: `.codex_tmp/intel_userland_oracle/t6-3-accum16-hi-live32-trueos-arena-bf16-unpack/dumps/000614_pre_exec_handle_3_off_0x0_len_0x200000.bin`

Contract:

- One SIMD8 workgroup.
- `gl_LocalInvocationID.x` selects one row/output slot `[0..7]`.
- The tile-record layout is unchanged:
  - `x[0..2047]` begins at dword `0`.
  - packed BF16 row data begins at dword `2048`.
  - each row stride is `1024` packed dwords.
  - output begins at dword `264192`.
- T6.2 must run first and write the live16 partial into `output[row]`.
- This artifact reads `output[row]`, accumulates packed BF16 lanes `16..31`,
  and stores the final live32 partial back into `output[row]`.

Oracle proof:

```text
oracle-app: t6-3-accum16-hi-live32-trueos-arena-packed-bf16 verified=1
rows=8 live_k=32 first_bits=0x44040000 last_bits=0x45840000
```

Native shape:

- Native words: `212`
- Native bytes: `0x350`
- Store send dword: `201` (`gpgpu_store_send_desc_words` indexes the final
  exdesc word; the four-word send starts at dword `198`)
- Final store descriptor words: `0xCC022F0C`, `0x009A3B0C`

Why this exists:

The monolithic T6.3 live32 artifact remains preserved, but its hardware run
retired with zero output and the first output-tile scan found no misplaced
stores.  The native store payload lived in high GRFs around `g106/g109`, while
T6.2 stayed in the lower register range and wrote correctly.  This split
accumulator proves the intended live32 row-block math with a low-register second
pass before we ask the walker shell to carry a larger single shader again.
