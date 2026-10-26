# Native Odin compute: historical EU reference

The old handwritten EU experiments are a launch/encoding reference for Odin's
runtime compiler. They are not a replacement compiler or the ShaderToy renderer.
The references below were recovered from local Git history; no legacy production
code was restored and no hardware was contacted for this audit.

## Where the experiments live

- `b949792ef` (2026-05-03), `crates/trueos-eu/src/gfx12.rs`: early raw
  Gfx12/Xe-LP EOT and HDC store artifacts.
- `7f13efeb1` (2026-05-10), `src/intel/gpgpu.rs`: recorded empty-thread milestone.
- `c0ffecbc2` (2026-06-03), `crates/trueos-eu/src/gfx12.rs`,
  `src/intel/compute/{payload,walker_command,descriptors}.rs`, and
  `src/intel/gpgpu/mandelbrot/{programs_and_patches,simd16_submits}.rs`:
  the later complete raw experiment, immediately before its removal in
  `42854c1f4`. Read individual files with `git show COMMIT:path`.

There are two distinct Mandelbrot stages. The early
`PRIMARY_SCANOUT_MANDELBROT8_*` pilot performs 32 scalar HDC stores; its own
comment says the CPU patches each address and color. The later
`patch_mandelbrot16_simd16_fixed10_body` emits actual unrolled EU Q12
multiply, subtract, add, compare, and store instructions. Its CPU routines
provide matching reference arithmetic. The latter is native Mandelbrot math,
but neither stage proves Odin source compilation.

## Reusable launch and instruction facts

- These are legacy `MEDIA_INTERFACE_DESCRIPTOR_LOAD` + `GPGPU_WALKER`
  compute launches on Gfx12/Xe-LP. Thread Spawner SFID is 7 on this path.
  A SIMD8 thread has one 32-byte GRF per vector; SIMD16 needs register pairs.
- The historical row selector reads `gl_WorkGroupID.x` from `r0.1`.
  Its exact SIMD8 broadcast to r49 is
  `[0x80030061, 0x31050220, 0x00000024, 0]`.
  The current IGC `copy_rect_rgba8` kernel independently confirms GroupID Y
  in `r0.6`: its native prologue copies r0 to r3, computes Y from
  `r3.6 * enqueued_local_size.y + local_id.y + global_offset.y`, and the
  checked-in source uses `get_global_id(1)`. Its scalar broadcast source
  encoding is `0x000000c4`.
- `scanout_block/scanout_8px.{asm,hex}` preserves native lane synthesis:
  `mov(8) r1:uw 0x76543210:v`, then widen UW to UD. Combining these lanes
  with `(group_x << 3)` gives eight unique pixel IDs per group. A power-of-two
  image width permits X/Y extraction with AND/SHR without integer division.
- A SIMD8 untyped HDC1 store uses descriptor `0x02026e00 | binding_index`,
  extended descriptor `0x40`, one address GRF and one data GRF, one dword
  written per active lane. Bind the output buffer surface and enable all
  eight walker lanes (`right_mask = bottom_mask = 0xff`). The historical
  SIMD16 body uses two SIMD8 sends with M0/M8 channel offsets.
- For kernels that synthesize local IDs, per-thread local-ID payload is not
  necessary. Cross-thread payload and the descriptor's read lengths still
  have to agree; the existing native scalar path supplies r1/r2/r3.
- EOT is a native SEND emitted by the program; GPU thread retirement and the
  ordered CPU completion marker must both succeed before reading output.

The checked-in `crates/trueos-shader/t5_one_row_matvec_artifact_contract.md`
and `t6_3_accum16_hi_live32_artifact_contract.md` preserve a crucial baremetal
finding: overwriting r127 immediately after an HDC store could write the
expected value but fail thread retirement. Their proven tail keeps the HDC
header in r127 and copies r0 into separate r126 for EOT:

```text
mov r126 <- r0: [0x80030061, 0x7e050220, 0x00460005, 0]
TS EOT r126:  [0x80030131, 0x00000004, 0x70007e0c, 0]
```

Odin's current scalar tail has explicit source synchronization and a dataport
fence before recycling registers. Preserve those dependencies when adapting it;
do not copy an unfenced legacy tail with immediate payload reuse.

## Validation boundary

Historical T5 notes record a correct load/math/store result and clean retirement
through 4096 SIMD8 groups on baremetal, with a non-retiring 6144-group rung for
that particular old artifact. This is historical evidence, not a limit measured
for the new Odin program or a proof of its physical execution.

The local Intel assembler (`iga64 -p=12p1`) independently decodes numeric
conversion instructions needed by a pixel envelope:

```text
mov(8) r114:f  r112:ud = [0x00030161, 0x720502a0, 0x00467005, 0]
mov(8) r114:ud r112:f  = [0x00030161, 0x72050a20, 0x00467005, 0]
```

Assembler/disassembler checks establish encoding consistency only. The required
new proof remains: compile source while TRUEOS is running, submit the emitted
native code, retire it, and compare its GPU output; then change source and repeat
without rebuilding TRUEOS or admitting a precompiled shader hash.
