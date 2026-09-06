# ParticleCraft

ParticleCraft is TRUEOS's first persistent, general-purpose C++/IGC particle
engine. It is not a font or sprite adaptation: one exact-target artifact owns
three C++ for OpenCL entries and the kernel retains compact particle state for
each UI4 instance.

## Arc Forge

The initial preset renders 128 cyan/violet/hot-white particles in a stable
640x400 simulation coordinate system. The native window shades a 320x200
sampling lattice and expands each sample across a 2x2 destination region
(an 8,192,000-test naive gather). A 2560x1440 maximized window allocates a true
1280x720 half-scanout backing, shades all 1280x720 backing pixels
(a 117,964,800-test naive gather), and uses the Intel direct-plane scaler for
the final exact 2x presentation. This keeps the double-buffer C++ ring near
7.0 MiB and the triple-buffer Blueprint ring near 10.5 MiB instead of
allocating full scanout-sized rings.
Particles have soft cores, velocity-aligned tails, bloom, and bounded
attractor/swirl physics over a procedural dark field. The Blueprint particle
app follows live UI4 pointer input for two seconds after each event, then
returns to the scripted orbit used by the ShaderToy ParticleCraft view.
The broad-phase particle bounds affect work rejection only. Separate smooth
round masks drive the head sphere and tail capsule to zero before those bounds,
preventing rectangular glow cutoffs during spawn and fade.
The broad phase is materialized once per 32x32 sample tile as a 256-bit mask.
At native size this costs 8,960 tile/particle tests; at the 1280x720 maximized
backing it costs 117,760. The pixel gather then visits only set bits from its
tile, retaining the exact smooth per-pixel bounds and shading without scanning
all 128 particles at every pixel.

## Artifact and ABI

`particle_craft.bin` contains exactly:

- `particle_craft_step`: one SIMD16 work-item per particle, updating two
  `float4` records (32 bytes) in place.
- `particle_craft_bin_tiles`: one work-item per 32x32 sample tile, producing
  one deterministic 256-bit candidate mask without atomics.
- `particle_craft_render_rgba8`: a race-free full-frame pixel gather over the
  retained state and its tile-local candidate mask.

The host inserts explicit cache/phase dependencies between all three walkers
and waits for one final post-sync marker before minting the UI4 release fence.
Each instance owns 8 KiB of state, a 4 KiB control page, and a page-rounded
116 KiB tile-mask arena in a distinct direct-RCS VA range. An accepted marker
timeout quarantines the context, destination, and retained allocation; there is
no CPU fallback after submit.
The private tail of the control page carries the current destination extent,
pitch, and a render divisor accepting 1, 2, or 4. The checked host default is
2 for the native window and switches to 1 for a larger half-scanout backing;
this changes gather work without an ABI revision or artifact rebake. Sample
coordinates are mapped back into the stable simulation space, so
maximize/restore changes render detail without changing particle physics.

Blueprints pass only `ParticleCraftParamsV1`, a versioned 64-byte pointer-free
control block. GPU addresses never cross the ABI. IGC, Clang, LLVM-SPIRV, and
C++ are bake-time dependencies only.

The checked artifact targets `8086:4680`, revision `0x0c`, and has SHA-256:

```text
8b3d026f2129593c9344c01c5f6cd89ecf213dcaa5adf8cd3c843d990783e113
```

Bake and verify:

```sh
make intel-gpu-bake-particle-craft-cpp
make intel-gpu-verify-cpp-artifacts
```

Open the ShaderToy Blueprint and use Left/Right to select ParticleCraft (ID 15).
Escape closes the app. Shell2 `cpp` is retired; `win` runs only the 30-window UI4
demo. The native program and its raw C++ inputs are packaged in ShaderToy, with
all three entry-point contracts checked before registration.

The catalog keeps particle state per window and resets it on re-entry. It uses
the kernel's existing 1/2/4 pixel-block writer to reproduce the old preview's
sample count on a full-size shared ShaderToy surface. At 1440p this is 1280×720
samples. The legacy half-size backing/plane-scaler policy remains available to
other internal consumers; this migration adds no upscale dispatch.
