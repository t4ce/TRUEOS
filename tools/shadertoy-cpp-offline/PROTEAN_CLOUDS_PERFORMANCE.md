# Protean Clouds lighting update

Local check: 2026-09-06. F6 uses the optimized
`TRUEOS-Blueprints/apps/shadertoy/assets/protean_clouds/input.glsl`.
`original.glsl` beside it preserves the previously admitted source.

## What changes

The shader already renders in one compute dispatch. Each ray can take 130
steps; every density query evaluates five octaves of procedural noise. Inside
the cloud, the original also runs two complete density queries per step for
lighting. Those extra queries are the optimization target.

`LIGHTING_STRIDE = 4` evaluates both original lighting probes at every fourth
ray step and at the first occupied sample after an empty interval. Between
anchors, lighting is linearly interpolated in ray distance. At cloud boundaries
and the end of a ray, a partial interval uses its last anchor.

The lighting contribution is linear in the probe result and does not affect
opacity. The shader accumulates its transmittance-weighted RGB coefficient and
distance moment, then resolves that interval when the next anchor is available.
This avoids retaining individual samples. Density, five noise octaves, all 130
potential ray steps, adaptive step sizes, opacity termination, fog integration,
camera, output resolution and final color processing are unchanged.

Simply holding lighting for four steps was slightly faster, but produced more
visible lighting contours. Interpolation reduced its mean RGB error from 0.492
to 0.305 at 640×360. Five-step interpolation gave about 1.68× paired speedup in
the exploratory run, but increased error to 0.416, with a worst-frame mean of
1.025. Four steps keeps more of the original lighting while meeting the intended
roughly 1.5–2× improvement. Some small lighting differences remain; this is an
approximation, not a byte-identical rendering.

## Final package measurements

Intel UHD Graphics 770, host Intel OpenCL SPIR-V path. The host device is
PCI 0xA780; the production Zebin remains targeted to ADL-S 0x4680 revision 0x0c.
These timings measure the local host path, not TRUEOS bare-metal dispatch or
end-to-end presentation. Each row is a sequential baseline/optimized pair.
Times are medians of 14 dispatch-plus-`clFinish` measurements after two warmups,
excluding readback and image saving.

| Size | Original | Optimized | Median ratio |
|---|---:|---:|---:|
| 640×360 | 102.858 ms | 65.233 ms | 1.58× |
| 1920×1080 | 902.760 ms | 552.308 ms | 1.63× |
| 360×640 | 114.514 ms | 76.456 ms | 1.50× |

Median paired-frame ratios were 1.61×, 1.67× and 1.54× respectively. Portrait
had one timing outlier with only 1.04× improvement; these short runs do not
establish a minimum speedup for every frame.

The 13 image comparisons per size cover t=0, 2, 4, 6, 8, 12, 20, 35, 60 and
120 seconds, plus three pointer positions. Mean absolute RGB error averaged
0.305, 0.305 and 0.265 respectively on the 0–255 scale. The worst frame mean
was 0.676, with worst PSNR 46.06 dB. Side-by-side frames were inspected. Every
run changed with time and repeated t=0 bit-identically. Portrait width 360 also
exercises the shader's partial SIMD16 group bounds check.

As a compositing control, the same accumulation with stride 1 was rendered
against the original at 640×360. Its mean RGB error was 0.000013, with maximum
channel error 1: the decomposition itself agrees to floating-point rounding;
the visible differences come from sampling lighting less often.

## Artifact and validation

The production bakery reproduced BC, SPIR-V and Zebin in two build directories.
The contract remains SIMD16, 128 GRFs, zero scratch/SLM, 96-byte cross-thread
and per-thread data, and the existing output/uniform bindings. The new Zebin
is 255,944 bytes (previously 250,920); fewer executed lighting queries, rather
than a smaller executable, provide the speed improvement.

Zebin SHA-256:
`438031ad8a14ec646a38ff4bcc5353f395954cfc0a715c36c9dee49254d8b704`.

The Blueprint contains the updated GLSL, generated C++, Zebin, SPIR-V, manifest
and contract in its authenticated `.stpkg`. Kernel contract/package trust
records and F6's diagnostic hash are updated together. Existing package
admission, component-tamper, contiguous-upload and stale-source checks pass;
no trust or dispatch limits were relaxed. The release kernel and local Blueprint
builds pass, and the built Blueprint contains all six current packages. Rebuild
the kernel and Blueprint together, since an older kernel correctly rejects the
changed package.

## Reproduce

Build `make -C tools/shadertoy-cpp-offline benchmark-protean-clouds`. The probe
accepts `kernel.spv output-directory [width height]`; create the output directory
first. Use the local Intel runtime's `OCL_ICD_VENDORS` and `LD_LIBRARY_PATH` as
in `run.sh`. Compare the current Blueprint SPIR-V with `original.glsl` rebuilt
through the same locked bakery, running the GPU probes sequentially.

The probe is retained as `benchmark_protean_clouds.c`. Baseline payloads,
prototype bakes, control frames and logs are under ignored
`bld/shadertoy-protean-perf/`; final packaged results, comparison images and
per-frame error metrics are in its `final/` directory. The original F6 had
already rendered on TRUEOS; this optimization still needs its bare-metal
performance and motion-quality check.
