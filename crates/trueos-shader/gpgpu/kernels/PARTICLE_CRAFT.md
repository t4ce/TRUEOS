# ParticleCraft

ParticleCraft is TRUEOS's first persistent, general-purpose C++/IGC particle
engine. It is not a font or sprite adaptation: one exact-target artifact owns
three C++ for OpenCL entries and the kernel retains compact particle state for
each UI4 instance.

## Arc Forge

The initial preset renders 1024 cyan/violet/hot-white particles in a stable
640x400 simulation coordinate system. The native window shades a 320x200
sampling lattice and expands each sample across a 2x2 destination region
(a 65,536,000-test naive gather). A 2560x1440 maximized window allocates a true
1280x720 half-scanout backing, shades all 1280x720 backing pixels
(a 943,718,400-test naive gather), and uses the Intel direct-plane scaler for
the final exact 2x presentation. This keeps the double-buffer C++ ring near
7.0 MiB and the triple-buffer Blueprint ring near 10.5 MiB instead of
allocating full scanout-sized rings.
Particles have soft cores, velocity-aligned tails, bloom, and bounded
attractor/swirl physics over a procedural dark field. The Blueprint particle
app follows live UI4 pointer input for two seconds after each event, then
returns to the scripted orbit used by `cpp particle`.
The broad-phase particle bounds affect work rejection only. Separate smooth
round masks drive the head sphere and tail capsule to zero before those bounds,
preventing rectangular glow cutoffs during spawn and fade.
The broad phase is materialized once per 32x32 sample tile as a 1024-bit mask.
At native size this costs 71,680 tile/particle tests; at the 1280x720 maximized
backing it costs 942,080. The pixel gather then visits only set bits from its
tile, retaining the exact smooth per-pixel bounds and shading without scanning
all 1024 particles at every pixel.

## Artifact and ABI

`particle_craft.bin` contains exactly:

- `particle_craft_step`: one SIMD16 work-item per particle, updating two
  `float4` records (32 bytes) in place.
- `particle_craft_bin_tiles`: one work-item per 32x32 sample tile, producing
  one deterministic 1024-bit candidate mask without atomics.
- `particle_craft_render_rgba8`: a race-free full-frame pixel gather over the
  retained state and its tile-local candidate mask.

The host inserts explicit cache/phase dependencies between all three walkers
and waits for one final post-sync marker before minting the UI4 release fence.
Each instance owns 32 KiB of state, a 4 KiB control page, and a page-rounded
452 KiB tile-mask arena in a distinct direct-RCS VA range. An accepted marker
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
a3bcb2a4f041e1fb82a0571c5dcc49959061229105112ecc223211ca83c2f491
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
