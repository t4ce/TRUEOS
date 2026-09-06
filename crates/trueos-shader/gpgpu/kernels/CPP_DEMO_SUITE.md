# C++/IGC generative demo suite

`cpp_demo_rgba8.clcpp` turns the proven C++ for OpenCL pipeline into a reusable
TRUEOS application surface. It is one offline-compiled kernel, one stable
output ABI, one resident upload, and seven scalar-selected workloads:

| ShaderToy view | Kernel mode | Foundation exercised |
| --- | ---: | --- |
| `gallery` | 0 | four live workloads, shared ABI, runtime mode dispatch |
| `aurora` | 1 | vector math, native transcendentals, octave loops |
| `julia` | 2 | bounded iteration, branching, complex arithmetic |
| `sdf` | 3 | signed-distance geometry, composition, antialiasing |
| `voronoi` | 4 | integer hashing, neighbour search, procedural cells |
| `retro-sun` | 5 | layered synthwave scene, animated cutout bands, reflection, CRT post |
| `cloud-high-wisps` | 6 | authored cloud preset, analytic high-wisp formation, artistic moon/sky treatment |

Open the ShaderToy Blueprint: F8 selects the four-panel gallery; F9–F12
select Aurora, Julia, SDF and Voronoi. Left/Right reaches Retro Sun and High
Wisps (IDs 13 and 14), as well as live audio (F7) and ParticleCraft (ID 15).
The seven gallery views reuse this one artifact with its original mode values.
See [`CPP_AUDIO_VISUALIZER.md`](CPP_AUDIO_VISUALIZER.md) for the real audio input.

Shell2 `cpp` has been removed. `win` now owns the separate 30-window retained
UI4 demo; `win status`, `win stop` and focused Escape manage that demo.

`cloud-high-wisps` is the fixed Linux/TRUEOS migration of
[`presets/cloud-high-wisps.json`](presets/cloud-high-wisps.json). Its source
save says `mode: draw`, but the original format deliberately omits painted 3D
density. The C++ mode consequently treats the saved pattern-2 **High wisps**
formation as deterministic auto source material instead of rendering an empty
volume. The shader uses the compile-time mirror in
`presets/cloud_high_wisps_preset.hpp`; that header is a hashed source input in
the artifact manifest. This first direct-RCS demo is an analytic 2D
approximation of the WebGPU 3D simulation/raymarch pair, retaining the
authored artistic palette, seed, air controls, moon, bands, outline, and
grain without introducing a 3D allocation or a multi-pass ABI. Its density is
feathered and composited as capped Beer-Lam-style translucency after the same
linear-palette/exposure/gamma transfer as the WGSL artistic renderer. The moon
belongs to the background before this composition, so wisps and painted
strokes can pass in front of it. The generic suite vignette is disabled only
for this mode: the WGSL sky already provides a mild side-falloff, and a second
vignette produced black edge lobes.

The next cloud stage no longer needs to invent a second sampling model.
`include/cloud_volume_sampling.hpp` defines the normalized clamp-to-edge
trilinear contract over linear RGBA16F storage, and
`src/intel/gpgpu/types/volumes.rs` gives that storage an explicit width/height/
depth/row-pitch/slice-pitch host contract. The companion
`tools/intel-texture-probe/cloud_volume_probe.clcpp` bakes the software-buffer
sample and the desired `image3d_t` hardware sample side by side. This lets the
production cloud move to persistent A/B volumes first, then replace eight
software voxel reads with an Intel sampler message without changing the scene
ABI or renderer math.

## Runtime boundary

Clang, `llvm-spirv`, `ocloc`, IGC, and the C++ frontend are build-time tools
only. The ShaderToy Blueprint carries the audited SPIR-V, Zebin, all raw C++ inputs,
and generated Rust contract in an authenticated package. Internal diagnostic
consumers retain their existing artifact copies in the kernel.
At runtime TRUEOS:

1. admits the artifact only for PCI device `0x4680`, revision `0x0c`;
2. authenticates the per-window package and verifies Zebin/SPIR-V hashes and ABI;
3. uploads it once at fixed non-overlapping GPU VA `0x0d600000`;
4. writes the scalar launch payload for the selected mode;
5. submits SIMD16 work through the serialized direct-RCS/GuC service lane;
6. waits for the exact post-marker before giving UI4 a producer-release token;
7. publishes the retired back buffer through the normal double-buffered
   movable-window path on universal plane slot 1.

There is no runtime compiler and no CPU renderer for these modes. A submitted
dispatch that fails to retire is quarantined; its write lease is not recycled
under a potentially late GPU writer.

## Generated contract

The checked artifact is `artifacts/adls/cpp/cpp_demo_rgba8.bin`:

```text
Zebin SHA-256: 6dd432e9666035c5d68b6c9fe71abaec72a4e09dfdf7f2c0e9e07043da4e7ab5
SPIR-V SHA-256: 24471d2de1dfe60b544be51b14ba3d6625ea57b3eef523fe348603483aa3a76f
target:          8086:4680 revision 0x0c
entry:           cpp_demo_rgba8 at Zebin offset 64
execution:       SIMD16, 128 GRFs, scratch 0, SLM 0
payload:         288 cross-thread bytes + 96 local-ID bytes
binding:         arg0, read/write stateful, BTI 0
```

The kernel parameters are the destination pointer and pitch/extent, a clipped
rectangle, time in seconds, mode, seed, and flags. This keeps animation and
mode selection out of the artifact-selection path.

The cloud mode additionally accepts two packed `uint16` vectors containing a
bounded 32-point brush ring and its active count. UI4 captures focused primary
button drags, converts frame-local coordinates after resize/movement, fills gaps
between input samples, and retains the ring for the lifetime of the cloud view.
Secondary-button dragging remains owned by UI4 window movement.

The bakery publishes this as `variant=cpp-native` under policy
`cpp-native-aot-v1`. Unlike the copy-rectangle `cpp` variant, it intentionally
has no legacy ABI-reference artifact. Publication still requires the reviewed
toolchain lock, exact expected kernel set, complete dependency hashes, sibling
SPIR-V identity, `ocloc validate`, and byte-identical bitcode/SPIR-V/Zebin from
two separate build roots.

## Math/compiler finding

Ordinary high-level `sin`, `cos`, `exp`, `log`, and `pow` calls caused this IGC
stack to add an internal callable support kernel, symbol table, and global
constant/private payloads. Those are legitimate compiler outputs, but the
current direct-RCS loader deliberately supports a smaller self-contained
contract. The suite therefore uses the OpenCL `native_*` forms and explicit
vector helpers. The resulting Zebin has exactly the reviewed
`cpp_demo_rgba8` entry and needs no new runtime linker or global-data loader.

This is a source-level constraint, not a parser exception: the bakery still
rejects unexpected entries or payload records.

## Rebuild and verification

Rebaking is an explicit host action:

```sh
make intel-gpu-bake-cpp-demo
```

Ordinary builds do not invoke the compiler stack. Their compiler-free gate is:

```sh
make intel-gpu-verify-cpp-artifacts
```

`make kernel` additionally scans the linked ELF for the complete demo Zebin.
`make iso` extracts `/TRUEOS.elf` from `bld/trueos.iso`, requires byte identity
with the staged runtime ELF, and repeats the demo-presence proof.

For runtime verification, open ShaderToy and cycle all seven gallery views,
including primary-button drawing in High Wisps. Check the app's per-view timing
logs, resizing and Escape cleanup. `win` should independently create exactly 30
retained windows and expose no shader cycling. Local build/host tests do not
substitute for this bare-metal interaction check.
