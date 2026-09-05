# Hex Array Pulse local validation

Validated on 2026-09-06 using the user-supplied v2 Image source. The source and
generated candidate files remain under ignored `bld/shadertoy-hex-array-pulse/`.
No new catalog ID or runtime artifact was admitted.

## Dependencies and adapter changes

This is one procedural, texture-free Image pass. The columns, hex prisms,
tori, lighting, shadows and floor reflection are evaluated by ray marching
inside that pass. It needs no `iChannel`, external asset or Buffer A-D input.

The original source exposed two translation gaps:

- `q.xz *= rot(...)` tries to bind an OpenCL vector swizzle to a C++ reference
  when selecting the matrix `operator*=`. The adapter now lowers simple
  variable swizzles to an assignment with a parenthesized multiplication RHS.
  It does not duplicate evaluated/indexed lvalues.
- `gPhase` and `gDrift` are writable GLSL globals belonging to each fragment.
  The adapter now creates a private invocation aggregate for sources with
  uninitialized scalar/vector globals. Helper functions access its fields;
  normal C++ scope preserves local/parameter shadowing. The aggregate is
  initialized for each pixel and adds no public kernel arguments.

These changes preserve the supplied shader's scene, march budgets, reflection,
AO, shadows and animation formulas.

## Host rendering proof

The generated OpenCL Kernel SPIR-V was loaded through the local Intel OpenCL
runtime on UHD Graphics 770, PCI ID `0xA780`. A headless diagnostic uses the
same uniform layout, output packing and `(16, 1)` workgroup shape as the
existing preview. It intentionally tests the candidate SPIR-V despite the
separate ADL-S artifact audit rejecting its scratch requirement; it does not
relax the production bakery or Blueprint admission policy.

| Check | Result |
|---|---|
| Full scene at 640x360, times 0, 2, 4, 6 and 8 seconds | All dispatches completed; PNGs saved and representative images visually inspected |
| Exact RGBA comparison, time 0 versus time 8, fixed mouse state | Byte-identical |
| Mouse orbit at time 0 | Completed and changed the frame |
| Default compilation, warm dispatch plus readback | About 52–59 ms per sampled frame; first frame about 92 ms |
| Disable loop unrolling, same six views | All RGB output bytes identical to the default compilation; loop and mouse checks also passed |

The timings include host readback and are diagnostic samples, not a sustained
frame-rate measurement. This host proof does not establish execution of the
ADL-S Zebin on the bare-metal `0x4680` target, which was not accessed.

Evidence:

- `bld/shadertoy-hex-array-pulse/adapter/host-640/proof.log`
- `bld/shadertoy-hex-array-pulse/adapter/host-640/frame-0.png` through `frame-5.png`
- `bld/shadertoy-hex-array-pulse/no-unroll/host-640/proof.log`
- `bld/shadertoy-hex-array-pulse/headless_probe.c`

## ADL-S admission result

Both variants were compiled twice using the locked local C++ toolchain and
the existing `adls-4680-r0c-cpp.json` profile. LLVM bitcode, SPIR-V and Zebin
were reproducible between build roots. `ocloc validate` accepted the binaries.
Both contain one SIMD16 kernel, 96 bytes of cross-thread data and 96 bytes of
per-thread data, but the TRUEOS audit correctly rejects them:

| Candidate | Required scratch | Production limit | Result |
|---|---:|---:|---|
| Full source with adapter fixes | 8192 bytes | 0 | Rejected |
| Same scene, loop unrolling disabled | 2048 bytes | 0 | Rejected |

The no-unroll experiment is confined to its ignored generated `.clcpp` file;
it is not a changed default for other shaders. Bake logs and `.ze_info` are in
the `adapter/` and `no-unroll/` subdirectories of the session.

Further Blueprint integration requires eliminating the remaining spills or
implementing and validating scratch-backed compute dispatch. Texture-channel
support would not remove this shader's scratch requirement.

## Regression validation

- All 20 adapter tests passed.
- All five existing catalog GLSL sources regenerate byte-identical `.clcpp`.
- A small semantic fixture passed the locked, reproducible zero-scratch bake.
  It was also executed on the host GPU: every pixel matched the expected RGB
  values for private zero initialization, helper mutation, initialized member
  state, parameter shadowing and swizzle/matrix orientation. Results are under
  `bld/shadertoy-hex-array-pulse/adapter-semantics/`.
