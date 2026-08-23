# C++/IGC generative demo suite

`cpp_demo_rgba8.clcpp` turns the proven C++ for OpenCL pipeline into a reusable
TRUEOS application surface. It is one offline-compiled kernel, one stable
output ABI, one resident upload, and seven scalar-selected workloads:

| Shell2 mode | Kernel mode | Foundation exercised |
| --- | ---: | --- |
| `gallery` | 0 | four live workloads, shared ABI, runtime mode dispatch |
| `aurora` | 1 | vector math, native transcendentals, octave loops |
| `julia` | 2 | bounded iteration, branching, complex arithmetic |
| `sdf` | 3 | signed-distance geometry, composition, antialiasing |
| `voronoi` | 4 | integer hashing, neighbour search, procedural cells |
| `retro-sun` | 5 | layered synthwave scene, animated cutout bands, reflection, CRT post |
| `cloud-high-wisps` | 6 | authored cloud preset, analytic high-wisp formation, artistic moon/sky treatment |

Plain `cpp` opens `gallery`, which divides one UI4 surface into four panels.
With that UI4 frame focused, Left and Right cycle through every C++ mode and
Escape closes the gallery. Retro Sun remains a standalone view rather than a
gallery panel. The audio view uses its own single audiovisual artifact and the
same resize lifecycle; see
[`CPP_AUDIO_VISUALIZER.md`](CPP_AUDIO_VISUALIZER.md).

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
grain without introducing a 3D allocation or a multi-pass ABI.

```text
cpp
cpp list
cpp status
cpp stop
```

The interactive session runs until focused Escape or `cpp stop`. Switching a
mode replaces the current Shell-controlled GPGPU preview through the service's
existing window/session lifecycle and restores focus to the replacement frame.

## Runtime boundary

Clang, `llvm-spirv`, `ocloc`, IGC, and the C++ frontend are build-time tools
only. The ISO carries the audited SPIR-V, Zebin, and generated Rust contract.
At runtime TRUEOS:

1. admits the artifact only for PCI device `0x4680`, revision `0x0c`;
2. verifies the embedded Zebin hash and generated ABI contract;
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
Zebin SHA-256: d2f1b3a9ff59605010a7337e7a0f28eb1438117414d0a7dde8fe987aa7041449
SPIR-V SHA-256: 0285abd959c48dd5057546c4bd198e7583ea36cc239b9ea63850912cb41b6e51
target:          8086:4680 revision 0x0c
entry:           cpp_demo_rgba8 at Zebin offset 64
execution:       SIMD16, 128 GRFs, scratch 0, SLM 0
payload:         128 cross-thread bytes + 96 local-ID bytes
binding:         arg0, read/write stateful, BTI 0
```

The kernel parameters are the destination pointer and pitch/extent, a clipped
rectangle, time in seconds, mode, seed, and flags. This keeps animation and
mode selection out of the artifact-selection path.

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

On the physical i5-14500T TestRig (`00:02.0`, `8086:4680`, revision `0x0c`),
boot `bld/trueos.iso` and run:

```text
cpp
cpp status
Left / Right
Escape
cpp stop
```

`cpp status` reports the resident/verified artifact state, GPU address, request
and window handles, submit/retire/publication counts, marker, and last error.
