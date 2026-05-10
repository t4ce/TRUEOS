# T Artifact Build Memo

This directory is the byte-for-byte reproducibility checkpoint for the current
T1-T4 GPGPU ladder artifacts.

## Tool

Assembler used:

```text
/home/t4ce/.local/share/Trash/files/bak/TRUEOS/.codex_tmp/mesa-brw-build/src/intel/compiler/brw/brw_asm
```

Target platform:

```text
-g tgl
```

Output format:

```text
-t hex
```

## Commands

```sh
BRW=/home/t4ce/.local/share/Trash/files/bak/TRUEOS/.codex_tmp/mesa-brw-build/src/intel/compiler/brw/brw_asm

$BRW -g tgl -t hex -o t1.hex t1_gfx12_eot_g127_send1.asm
$BRW -g tgl -t hex -o t2.hex t2_gfx12_hdc1_stateless_store_eot.asm
$BRW -g tgl -t hex -o t3.hex t3_static_dp4a_hdc1_stateless_store_eot.asm
$BRW -g tgl -t hex -o t4.hex t4_live_x_requirement_alias.asm
```

## Artifact Meaning

T1:

- Source: `t1_gfx12_eot_g127_send1.asm`
- Preserved original source: `.codex_tmp/gfx12_eot_g127_send1.asm`
- Embedded Rust array: `TS_EOT_R0_TO_G127_SEND1_WORDS`
- Purpose: one minimal Thread Spawner EOT worker lifecycle.

T2:

- Source: `t2_gfx12_hdc1_stateless_store_eot.asm`
- Preserved original source: `.codex_tmp/gfx12_hdc1_stateless_store_eot.asm`
- Embedded Rust array: `HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS`
- Purpose: one HDC store plus EOT, proving visible EU side effect.

T3:

- Source: `t3_static_dp4a_hdc1_stateless_store_eot.asm`
- Embedded Rust array: `STATIC_DP4A_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS`
- Purpose: immediate setup, DP4A arithmetic shape, HDC store, EOT.
- Important exact source bit: the `dp4a` line must include `@1`:

```text
dp4a(8) g4<1>D g2<8,8,1>D g6<8,8,1>D g7<1,1,1>D { align1 1Q @1 };
```

Without `@1`, dword 12 becomes `0x00030058` instead of the preserved
`0x00030158`.

T4:

- Source: `t4_live_x_requirement_alias.asm`
- Embedded Rust array: intentionally the same as T3:
  `STATIC_DP4A_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS`
- Purpose: catalog/contract rung for the live-input requirement while preserving
  the proven T3 binary shell.

## Verification Result

See `REPORT.txt`.

Current successful result:

```text
RESULT all_repro_checks=true
```

T1 and T2 match both regenerated hex and preserved `.codex_tmp/*.hex`.
T3 and T4 match the embedded Rust artifact bytes; T4 is intentionally a T3
alias at this stage.

## Nearby Later Rungs

T4.7 and T4.8 are runtime-patched controls, not fresh offline artifacts:

- T4.7 patches the proven HDC store/EOT shell with output address and sentinel.
- T4.8 patches the proven static DP4A/HDC store/EOT shell with output address
  and CPU-reference-derived base.

T5-small live4 is a Mesa ANV oracle-derived artifact preserved separately under:

```text
.codex_tmp/t5_small_live4_trueos_arena/
```

The current T5 store-only control added in `trueos-eu` is a diagnostic catalog
artifact derived from the T5 arena payload/store/EOT shape. Its job is to split
"T5 store/payload/surface broken" from "T5 live-load/math path broken" before
we widen or deepen the live BF16 dot.
