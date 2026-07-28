# Intel GPGPU kernels

TRUEOS has one maintained Intel kernel architecture: C++ for OpenCL, compiled
offline to SPIR-V and then Intel Zebin for Alder Lake S.

## Architecture

Every source in this directory is a `.clcpp` file. Every checked-in GPU
artifact is published below `artifacts/adls/cpp/` as one four-file set:

```text
kernel.clcpp
  -> kernel.spv
  -> kernel.bin
  -> kernel.manifest.json
  -> kernel.contract.rs
```

The runtime embeds the Zebin and SPIR-V and consumes the generated Rust ABI
contract. It never compiles a kernel. There is no OpenCL-C source lane, legacy
artifact directory, Cargo feature switch, or runtime fallback.

The canonical frontend is:

```text
Clang C++ for OpenCL
  -> spir64 LLVM bitcode
  -> llvm-spirv OpenCL Kernel SPIR-V
  -> ocloc/IGC Intel Zebin
```

The pinned ADL-S profile requires device `8086:4680`, revision `0x0c`, SIMD16,
and zero scratch/SLM. The bakery records the complete source/header graph,
toolchain identity, two-root reproducibility result, ELF/`.ze_info` ABI, and
SPIR-V identity before publication.

See [CPP_FOR_OPENCL_ARCHITECTURE.md](CPP_FOR_OPENCL_ARCHITECTURE.md) for the
compiler and ABI boundary.

## Maintained kernel groups

- 2D primitives: copy, fill, gradient, alpha blend, glyph mask, sprite quad,
  Mandelbrot worklists, chart, plasma, skybox sampling, scene AABB, and MSAA
  resolve.
- UI4/video: layer composition, Tile64 NV12 conversion, and RGBA8 to
  linear NV12.
- Font production: analytical Skrifa-outline coverage to persistent R8 masks.
  The former diagnostic outline-mesh kernel and its render-import probe were
  removed.
- Applications: C++ demo suite, live audio visualizer, ParticleCraft, and
  Spirit background/sprite shaders.
- Internal validation: the three-entry `lab256_multiphase` artifact.
- Inference: the two LFM2.5 Q8 projection kernels.

Focused design notes remain beside their sources:

- [CPP_DEMO_SUITE.md](CPP_DEMO_SUITE.md)
- [CPP_AUDIO_VISUALIZER.md](CPP_AUDIO_VISUALIZER.md)
- [PARTICLE_CRAFT.md](PARTICLE_CRAFT.md)
- [SPIRIT_CPP_REPASS.md](SPIRIT_CPP_REPASS.md)
- [LAB256_MULTIPHASE_EXPLORE.md](LAB256_MULTIPHASE_EXPLORE.md)

## Bake and verify

Refresh every checked-in artifact with the pinned host toolchain:

```sh
make intel-gpu-bake-cpp-artifacts
```

The compatibility entry point performs the same complete bake:

```sh
crates/trueos-shader/gpgpu/bake_adls_artifacts.sh
```

Ordinary builds are compiler-free. Verify every artifact, manifest, generated
contract, source mapping, and bakery regression test with:

```sh
make intel-gpu-verify-cpp-artifacts
```

`make kernel` and `make iso` always select this architecture.
