# ShaderToy compiler and fullscreen dispatch update

2026-09-06. This follows the cube-field and Protean lighting optimizations.
The reported Protean artifact was `438031ad...`; the new one is
`19119f60ffe6a9207bd24d40aecde2b672a27aa253e228f048f36a9c81782696`.

## Confirmed fullscreen failure

The supplied capture records a successfully admitted and submitted 2560×1440
Protean dispatch, then `submit_ms=1006`, no completion marker, system-service
context quarantine, and `render/compute publish failed: Busy`. The one-second
producer retirement limit had expired. The Blueprint treats that error as fatal.
The execution lane running Spirit VFX also timed out earlier in this capture;
both logical contexts share RCS hardware. The log alone cannot establish exactly
how their execution overlapped or whether a late batch eventually finished.

`Busy` here is not an ordinary queue-full rejection. An accepted, unretired
submission may still write its destination and read batch storage. Retrying or
recycling that memory would be incorrect. The existing quarantine and retention
rules remain intact.

1440p contains 3,686,400 pixels: 16 times the initial 640×360 window. Protean
performs a five-octave procedural density evaluation at each of up to 130 ray
steps, plus lighting queries. A short source program can therefore represent
billions of noise-octave evaluations per frame. The EUs execute batches of
pixels; they do not execute millions of pixels simultaneously.

## Compiler finding

The old profile used the OpenCL backend's default math-library precision.
Passing `-cl-fast-relaxed-math` to **ocloc/IGC**, and the same option to host
`clBuildProgram`, removed a large amount of math-library work. Passing that flag
only to Clang did not produce the same improvement in the SPIR-V path.
Protean's native executable entry decreased from 221,528 to 17,008 bytes; its
Zebin decreased from 255,944 to 51,504 bytes. The GLSL and generated C++ are
unchanged by this compiler update, as is the SPIR-V. Backend build options are
therefore essential provenance; SPIR-V alone does not describe the executable.

The new `adls-4680-r0c-shadertoy.json` profile records this choice explicitly as
`cpp.math_mode = relaxed`. Its separate lock has exactly the same compiler and
library fingerprints as the strict lock; only the profile hash differs. The
four reviewed effects Nguyen, Palette Grid, Cosmic Strands and Protean use it.
Mandelbrot and the cube field retain their original profile and executable.
The cube's sine-based hash amplifies small numeric changes into different column
heights, so applying the flag everywhere would be inappropriate.

The native preview uses the same relaxed backend option. Standalone
`compile_shader.py --math-mode strict` retains a strict comparison path.

`-cl-fast-relaxed-math` includes finite-math-only and unsafe-math optimizations;
these relax arithmetic semantics as well as library precision, including MAD
contraction, signed-zero handling and denormal handling. Native math functions
have implementation-defined error bounds and input ranges. See the
[Khronos OpenCL C specification](https://registry.khronos.org/OpenCL/specs/unified/html/OpenCL_C.html)
and [Intel's floating-point optimization guidance](https://www.intel.com/content/www/us/en/docs/oneapi/optimization-guide-gpu/2024-1/fp-computations.html).

## Explicit native intrinsics and workgroups

Following the requested deeper check, separate Protean bakes explicitly used
`native_sin`, `native_cos`, `native_exp`, `native_divide`, `native_recip`, and
normalization through `native_rsqrt(dot(p,p))`. All passed the pinned zero-scratch
bake. Against the relaxed backend alone, they produced byte-identical images
in the ordinary and long-time samples, with no measurable additional speedup:
about 14.2–14.3 ms at 640×360. Those source substitutions were not promoted;
the backend option already provides their measured benefit on this compiler.

Host workgroup sizes 16, 64 and 128 also made essentially no difference for
Protean: about 65 ms with strict math, 14 ms with relaxed math. The production
SIMD16 / 16×1 shape and local-ID ABI remain unchanged. This result does not
claim that occupancy tuning is unimportant for every kernel; it rules out that
particular change as the explanation for this workload's large cost.

## Bounded row dispatch

The direct descriptor currently disables thread preemption. A single expensive
fullscreen walker also makes submission latency grow with the entire window.
The renderer now splits a frame into independently retired row batches:

- Nguyen and Protean: at most 131,072 launched pixels per batch.
- The four cheaper shaders: at most 1,048,576 launched pixels per batch, keeping
  their submission overhead small.

The calculation includes SIMD16 width padding. The existing implicit
`global_id_offset.y` selects each batch's first row; resolution, pitch,
destination base, mouse, time and all image-space shader inputs stay unchanged.
This introduces no resampling or missing pixels. Every batch must fully retire
before the next reuses the command/result storage, and the submit lock is
released between batches. Publication occurs only after the entire image is
complete. A failed batch stops the sequence and never publishes partial output.
The one-second retirement limit and accepted-submission quarantine are unchanged.
These boundaries reduce the size of each individual GPU job; they do not turn
an expensive shader into a 30 FPS shader or guarantee hardware retirement under
all other workloads.

## Host results at 2560×1440

Intel UHD Graphics 770, PCI 0xA780, local Intel OpenCL runtime. Production Zebin
remains pinned to ADL-S 0x4680 revision 0x0c. Medians cover 14 launches after two
warmups, at the same time/pointer sequence. Readback and image saving are outside
the timed interval. Updated times include all row batches and their completion
waits. These are host measurements, not bare-metal or compositor frame rates.

| Shader | Previous | Updated | Change |
|---|---:|---:|---:|
| Mandelbrot | 4.42 ms | 4.81 ms | Same shader; small batching overhead |
| Cube field | 9.15 ms | 8.85 ms | Same shader; within run variation |
| Nguyen | 93.44 ms | 26.93 ms | 3.47× faster |
| Palette grid | 6.13 ms | 3.20 ms | 1.91× faster |
| Cosmic Strands | 12.42 ms | 7.04 ms | 1.76× faster |
| Protean Clouds | 971.83 ms | 216.82 ms | 4.48× faster |

Protean's longest observed warm row batch was 8.954 ms; a 1440p frame has 29
such batches. Nguyen's maximum was 1.469 ms. The cheaper effects use four
batches at this size. All six shaders' row-batched images were byte-identical
to their matching full-dispatch images. A separate 360×640 Protean comparison
also matched all 16 frames, exercising a partial SIMD group and a partial final
row batch. Every probe verified animation changes and a bit-identical t=0 repeat.

Against strict math, mean absolute RGB error over the 13 ordinary comparisons
(t=0 through 120 seconds plus three pointer positions) was 0.00053/255 for
Protean, 0.00045 for Nguyen, 0.00164 for Palette Grid and 0.00019 for Cosmic
Strands. Mandelbrot and the cube were identical. These are sampled comparisons,
not universal error guarantees.

Large-time inputs expose the precision tradeoff: in additional 640×360 Protean
tests, mean error was about 0.004/255 at 10 minutes, 0.124 at one hour, 1.010 at
two hours and 10.294 at twelve hours; one ten-hour pointer case reached 11.137.
The procedural pattern drifts relative to strict math as coordinates grow.
Explicit native intrinsics matched the relaxed version exactly in those tests
and did not remove that drift. No time wrapping or silent reset was added.

## Validation and next bare-metal evidence

All six packages pass admission, tamper and stale-source checks. The four new
executables reproduce across separate build directories and retain zero scratch,
zero SLM, SIMD16 and the existing argument contracts. Compiler/profile tests
verify that the option reaches the backend and does not alter strict defaults.
`tools/test_shadertoy_dispatch.py` executes the production row planner, payload
writer and frame completion logic, checking exact coverage, full-image inputs,
partial failure and publication ordering. The adapter suite also passes.

The Blueprint now logs a five-second performance interval per shader/size:
`fps_x100` is successful published frames per second multiplied by 100, while
`render_publish_us_avg/max` measures the render-and-publish call, excluding the
preceding visual-cadence wait. This is wall time including runtime overhead,
not a GPU timestamp query. Failed calls log shader, dimensions and elapsed time.
These records will distinguish a cadence ceiling from an expensive production
call in the next bare-metal run. No GPU power setting was changed.

Rebuild and install the kernel and Blueprint together because four trusted
package hashes changed. A transcribed excerpt of the initially observed timeout (the live capture rotated),
baseline artifacts,
prototype bakes and proofs are retained under ignored
`bld/shadertoy-dispatch-perf/`. Final size measurements and image comparisons are
in `final-1440p/metrics.json`; explicit-intrinsic and long-time probes are nearby.

To reproduce, build `make -C tools/shadertoy-cpp-offline benchmark-protean-clouds`.
The probe accepts `kernel.spv output-directory [width height]` and these variables:

- `SHADERTOY_KERNEL`: kernel name; defaults to `shadertoy_protean_clouds`.
- `SHADERTOY_BUILD_OPTIONS`: use `-cl-fast-relaxed-math` for the relaxed profile;
  leave empty for strict. Match the artifact's manifest command.
- `SHADERTOY_MAX_BATCH_PIXELS`: 131072 or 1048576 for the production policy;
  leave unset or zero for one full dispatch.

Use the local ICD/library environment from `run.sh`, create each output directory
first, and run comparison probes sequentially on the GPU.
