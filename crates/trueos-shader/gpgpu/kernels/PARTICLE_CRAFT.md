# ParticleCraft

ParticleCraft is TRUEOS's first persistent, general-purpose C++/IGC particle
engine. It is not a font or sprite adaptation: one exact-target artifact owns
two C++ for OpenCL entries and the kernel retains compact particle state for
each UI4 instance.

## Arc Forge

The initial preset renders 128 cyan/violet/hot-white particles at a native
640x400 logical extent. It currently shades 320x200 samples and expands each
sample across a 2x2 destination region, retaining the complete frame and
particle state while reducing the dominant gather from 32,768,000 to
8,192,000 candidate tests per frame.
Particles have soft cores, velocity-aligned tails, bloom, and bounded
attractor/swirl physics over a procedural dark field. The Blueprint particle
app follows live UI4 pointer input for two seconds after each event, then
returns to the scripted orbit used by `cpp particle`.

## Artifact and ABI

`particle_craft.bin` contains exactly:

- `particle_craft_step`: one SIMD16 work-item per particle, updating two
  `float4` records (32 bytes) in place.
- `particle_craft_render_rgba8`: a race-free full-frame pixel gather over the
  retained state.

The host inserts an explicit cache/phase dependency between the walkers and
waits for one final post-sync marker before minting the UI4 release fence.
Each instance owns 8 KiB of state plus a 4 KiB control page in a distinct
direct-RCS VA range. An accepted marker timeout quarantines the context,
destination, and retained allocation; there is no CPU fallback after submit.
The private tail of the control page carries the current destination extent,
pitch, and a render divisor accepting 1, 2, or 4. The checked host default is
2; changing `PARTICLE_CRAFT_RENDER_DIVISOR` adjusts gather work without an ABI
revision or artifact rebake. The bounded gather expands each sample over its
corresponding destination region, so maximize/restore covers the replacement
frame without multiplying particle tests by output resolution.

Blueprints pass only `ParticleCraftParamsV1`, a versioned 64-byte pointer-free
control block. GPU addresses never cross the ABI. IGC, Clang, LLVM-SPIRV, and
C++ are bake-time dependencies only.

The checked artifact targets `8086:4680`, revision `0x0c`, and has SHA-256:

```text
7c82478fa58aea2a74fa3f1f259276bc9caae1ce271aa60ed9a4ac3b24c32778
```

Bake and verify:

```sh
make intel-gpu-bake-particle-craft-cpp
make intel-gpu-verify-cpp-artifacts
```

Run on the TestRig:

```text
cpp particle
cpp start particle 0 33 1
cpp status
cpp stop
```
