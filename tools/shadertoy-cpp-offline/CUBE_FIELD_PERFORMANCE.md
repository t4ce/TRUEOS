# Cube-field algorithm update

Local check: 2026-09-06. The Blueprint's F2 entry keeps the same animated
column field, moving sphere, camera, height function and lighting. Its input is
`TRUEOS-Blueprints/apps/shadertoy/assets/cube_field/input.glsl`; `original.glsl`
beside it retains the previous source for comparison.

## Where the work went

The previous shader already used a single fullscreen compute dispatch. Inside
each pixel it marched up to 128 steps, calling the procedural scene at every
step, then evaluated the scene six more times for a finite-difference normal.
Those evaluations repeatedly performed the cell hash, sine, power and distance
calculations. The native executable entry contained 792,128 bytes of code.
An OpenCL no-unroll hint was tested; it did not reduce that native entry size.

The new shader intersects the sphere analytically. It treats unaffected columns
as their exact flat plane at y=.225, and traverses only the grid cells in the
animated region. Column tops are bounded to [.175, .275]; their cell centers
are affected only within .5 of the sphere in XZ. Including each cell's .02
half-width gives a bounded traversal region (the implementation uses .521 for
rounding margin). Each visited cell needs one height evaluation and a direct
intersection; its top/side normal follows from the hit face. Sphere normals are
also analytic. This removes empty-air marching and the six extra normal queries.

The traversal count is calculated from cell crossings inside that bounded
region. Its 128-cell cap exceeds the number of possible crossings through the
region; it no longer expands an unconditional 128-step procedural march.

## Measured result

Intel UHD Graphics 770, local Intel OpenCL SPIR-V path. Times are median warm
dispatch-plus-`clFinish` latency, excluding readback and image saving; the first
two launches are discarded. These are host measurements, not bare-metal timings.

| Size | Previous | Analytic/grid | Ratio |
|---|---:|---:|---:|
| 640×360 | 10.741 ms | 0.552 ms | 19.5× |
| 1920×1080 | 93.081 ms | 3.763 ms | 24.7× |
| 360×640 | 11.287 ms | 0.347 ms | 32.5× |

Images were compared at t=0, 2, 4, 6 and 8 seconds in all three sizes. Between
0.20% and 0.43% of pixels changed; mean absolute RGB error was below 0.324 on
the 0–255 scale. The differences cluster around silhouettes and touching box
edges: exact intersections/face normals replace finite marching and epsilon
normal estimates. The result is visually close, not byte-identical. Repeated
t=0 renders within each variant were byte-identical. Portrait width 360 also
exercises a partial SIMD16 group (the host probe rounds the launch width up).

| Artifact | Previous | Updated |
|---|---:|---:|
| Zebin file | 818,920 bytes | 64,488 bytes |
| Executable entry | 792,128 bytes | 36,760 bytes |
| Scratch / SLM | 0 / 0 | 0 / 0 |

Updated Zebin SHA-256:
`04f940ae84746975d6c11033ce7899ccc8307badcaf3091f53a654ca10256f10`.
The locked bakery reproduced BC, SPIR-V and Zebin across two build directories.
The entry remains SIMD16, with 96-byte cross-thread and per-thread data and the
existing output/uniform bindings. The kernel contract and complete package hash
were refreshed; no admission or timeout checks were disabled.

## Reproduce the host measurement

Build `make -C tools/shadertoy-cpp-offline benchmark-cube-field`. Its executable
accepts `kernel.spv output-directory [width height]`; create the output directory
first. Use the same `OCL_ICD_VENDORS` and `LD_LIBRARY_PATH` setup as `run.sh` for
the local Intel runtime. The current SPIR-V is in the Blueprint's cube-field
assets. Rebuild `original.glsl` with the locked bakery to compare the prior
algorithm under the same compiler/runtime.

The recorded logs, comparison images, baseline payload and prototype bakes are
under ignored `bld/shadertoy-cube-perf/`. The C probe is retained as
`benchmark_cube_field.c` beside the native preview. The bare-metal capture also
recorded a Spirit VFX execution-lane retirement timeout immediately after
cube-field selection. That observation warrants a separate runtime check; the
host measurements alone do not establish that shared-lane issue's cause or cure.
