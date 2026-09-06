# TRUEOS ShaderToy C++ preview

This is a small native Ubuntu paste/run tool for ShaderToy Image passes. The
left pane is a source editor, the button (or `Ctrl+Enter`) runs the repository's
C++-for-OpenCL bakery, and the right pane presents the generated kernel at
640x360 and 60 Hz.

It is deliberately the same artifact path used by the Spirit previews, not an
OpenGL/WebGL fallback:

```text
pasted ShaderToy GLSL
  -> explicit-uniform C++ for OpenCL session source
  -> Clang spir64 LLVM bitcode
  -> llvm-spirv OpenCL Kernel SPIR-V
  -> ocloc/IGC Intel Zebin + TRUEOS ABI audit
  -> Intel OpenCL loads the sibling SPIR-V for the Ubuntu preview
```

The pasted source and all generated session files stay under ignored `bld/`.
The tool does not download or check in the linked shader.

## Runtime Blueprint packages

The six admitted shaders and their raw GLSL now live under
`TRUEOS-Blueprints/apps/shadertoy/assets/`. The kernel retains trusted hashes and
ABI contracts; the Blueprint transfers complete authenticated packages at startup.
See [the runtime path](../../crates/trueos-shader/gpgpu/kernels/SHADERTOY.md).
Candidate preview sessions remain under `bld/` and are not automatically admitted.

The native preview and four reviewed visual effects now use a separate
`-cl-fast-relaxed-math` backend profile. Large runtime frames use bounded row
batches, preserving every pixel and the existing retirement checks. See
[runtime performance](RUNTIME_PERFORMANCE.md) for the fullscreen timeout finding,
1440p timings, explicit native-intrinsic tests and precision tradeoffs.

## Ubuntu setup

The host UI follows the existing Spirit tools and uses X11/Xwayland. Install
the small host build dependency and OpenCL loader once:

```sh
sudo apt install build-essential libx11-dev ocl-icd-libopencl1
```

Install the Intel compiler and OpenCL runtime packages locally under `bld/`
(no root and no host package changes):

```sh
make -C tools/shadertoy-cpp-offline toolchain
```

You may instead provide existing tools through `CLANG`, `LLVM_SPIRV`, and
`OCLOC`. Then launch the editor from the repository root:

```sh
make -C tools/shadertoy-cpp-offline run
```

An optional source path preloads the editor:

```sh
make -C tools/shadertoy-cpp-offline
tools/shadertoy-cpp-offline/run.sh /path/to/image-pass.glsl
```

Controls are `Ctrl+V` paste, `Ctrl+A` replace all, `Ctrl+S` save the session,
`Ctrl+Enter` bake/run, and `Esc` close. Pointer input over the preview supplies
ShaderToy-compatible `iMouse` coordinates.

The complete source is selected when the tool opens, so the first `Ctrl+V`
replaces the sample or previous session instead of appending a second
`mainImage`. If two entry points are pasted intentionally, the adapter reports
that directly.

Clipboard input is normalized for shader source: CRLF becomes LF, tabs expand
to four-column spaces, non-breaking spaces become ordinary spaces, and BOM or
zero-width web formatting marks are removed. This prevents X11 control glyphs
and compiler errors from otherwise invisible browser clipboard characters.

## Compatibility boundary

The first slice accepts a single, texture-free `mainImage` pass and supplies:

- `iResolution`, `iTime`, `iTimeDelta`, `iFrame`, and `iFrameRate`
- `iMouse`, `iDate`, and `iSampleRate`
- scalar/vector GLSL constructors and common math helpers
- `mat2`/`mat3` construction and vector/matrix multiplication
- GLSL `reflect`, `refract`, and `faceforward` geometric helpers
- deterministic zero initialization for omitted local scalar initializers
- matrix compound multiplication on simple variable swizzles (`q.xz *= rot(a)`)
- invocation-private uninitialized globals, initialized writable scalars, and
  vector/matrix globals; helpers share that pixel's state and local parameters
  can shadow its fields
- scalar scaling directly on `mat2`/`mat3` constructors
- GLSL RGBA component aliases with the pinned Clang frontend

Shaders with such globals are adapted into a private C++ aggregate created
for each pixel. Sources using only the established constant-global subset keep
their existing generated form. Complex swizzle lvalues such as
`items[index++].xy *= matrix` still need an explicit temporary in the source.

[Hex Array Pulse](HEX_ARRAY_PULSE.md) exercises both additions. It renders
through the local Intel OpenCL runtime, but its scratch requirement still
prevents admission to the Blueprint catalog.

[Protean Clouds and the Aiekick sphere](CANDIDATE_CHECKS.md) exercise the next
compatibility boundaries. The reconstructed Protean source passes the locked
zero-scratch bake and host rendering; the sphere needs channel resources,
cubemap sampling and nonzero mip levels.

F6 now interpolates its expensive lighting probes between four-step anchors,
while retaining the original density, ray steps and resolution. See
[Protean Clouds performance](PROTEAN_CLOUDS_PERFORMANCE.md) for the measured
roughly 1.6× host speedup, image comparisons and reproducible benchmark.
The later backend-math update above provides an additional measured improvement.

The adapter rejects `iChannel*`, samplers/texture calls, sound passes, and
screen-space derivatives with a direct diagnostic. Picasso and QuadTexture
now provide working graphics texture infrastructure, but this compute path
still needs channel resources, image/sampler artifact contracts and dispatch
bindings. Multipass and derivatives require further contracts. See the
[local compatibility audit](COMPATIBILITY.md) for the verified baseline,
compiler probe results and the proposed static 2D channel extension. `mat4` is not
part of this initial adapter. If a source uses a GLSL form
outside the adapter subset, the exact generated `.clcpp` and compiler error
remain in `bld/shadertoy-cpp-offline/session/` for a focused compatibility
addition.

The bakery retains the production ADL-S constraints, including SIMD16 and zero
scratch/SLM. A shader that spills is rejected because TRUEOS cannot currently
submit that artifact with the same ABI.

## Tests

```sh
make -C tools/shadertoy-cpp-offline test
```

## Radial focus rendering

The production Protean package now supports an optional smaller, radially warped
sample image followed by a four-tap GPU reconstruction. The Blueprint defaults
to that mode above 720p and exposes Space for native-resolution comparison.
`TRUEOS-Blueprints/apps/shadertoy/FOVEATED_RENDERING.md` documents the mapping,
local performance/quality results, ownership and reuse in a future Picasso
material. `benchmark_protean_clouds` accepts `FOVEATED_MODE=full|uniform|radial`;
older five-argument artifacts remain usable as full-resolution references.

The adapter's `--foveated` export is opt-in and changes its GPU ABI. Only Protean
is admitted with that layout. Run `python3 test_foveated_mapping.py` to test the
actual coordinate functions on CPU using Clang vectors.
