# C++/IGC generative demo suite

`cpp_demo_rgba8.clcpp` turns the proven C++ for OpenCL pipeline into a reusable
TRUEOS application surface. It is one offline-compiled kernel, one stable
output ABI, one resident upload, and six scalar-selected workloads:

| Shell2 mode | Kernel mode | Foundation exercised |
| --- | ---: | --- |
| `gallery` | 0 | four live workloads, shared ABI, runtime mode dispatch |
| `aurora` | 1 | vector math, native transcendentals, octave loops |
| `julia` | 2 | bounded iteration, branching, complex arithmetic |
| `sdf` | 3 | signed-distance geometry, composition, antialiasing |
| `voronoi` | 4 | integer hashing, neighbour search, procedural cells |
| `retro-sun` | 5 | layered synthwave scene, animated cutout bands, reflection, CRT post |

`gallery` is the default and divides one UI4 surface into four panels. The
other commands give each workload the complete resizable window. Retro Sun is
intentionally standalone and is not added to the gallery.
The separate `cpp audio` mode uses its own single audiovisual artifact and the
same resize lifecycle; see
[`CPP_AUDIO_VISUALIZER.md`](CPP_AUDIO_VISUALIZER.md).

```text
cpp
cpp aurora
cpp julia
cpp sdf
cpp voronoi
cpp retro-sun
cpp audio
cpp list
cpp status
cpp stop
```

The longer form controls lifetime and publication cadence:

```text
cpp start [gallery|aurora|julia|sdf|voronoi|retro-sun|audio] [duration_ms] [cadence_ms] [publish_every]
cpp start gallery 0 33 1
cpp start retro-sun 0 33 1
cpp start audio 0 50 1
```

The default lifetime is 30 seconds. A duration of zero runs until `cpp stop`.
Starting a mode replaces the current Shell-controlled GPGPU preview because
the producer uses that service's existing window/session lifecycle.

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
Zebin SHA-256: 75e5a83b3e74e3b5da59756bc5a804cbb742314389bb60559474586050ce66ac
SPIR-V SHA-256: be41fccaaca39e0c1584e5062b5434a17366441bc586b2134135d3664729b3d5
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
cpp julia
cpp sdf
cpp voronoi
cpp retro-sun
cpp stop
```

`cpp status` reports the resident/verified artifact state, GPU address, request
and window handles, submit/retire/publication counts, marker, and last error.
