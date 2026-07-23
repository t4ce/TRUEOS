# Spirit C++/IGC visual repass

Spirit's C++ repass upgrades the existing visual vocabulary without replacing
its runtime architecture. The two `.clcpp` entry sources compile the retained
OpenCL C compositions through Clang C++ for OpenCL, SPIR-V, and Intel IGC.
`TRUEOS_SPIRIT_CPP_REPASS` enables compile-time-specialized detail layers while
the existing `.cl` sources remain the semantic baseline and exact ABI
references.

The compiler stack is build-time only. TRUEOS embeds the audited SPIR-V and
Zebin images; the running OS contains no Clang, C++ runtime, `llvm-spirv`,
`ocloc`, or IGC.

## What changed visually

The nine stable background IDs retain their original large-form ideas:

- energy ring gains counter-rotating beads and a fine inner corona;
- magic circle gains two rotating rune belts;
- nebula smoke gains sparse, edge-safe star seeds;
- cyber grid gains pulsing circuit intersections;
- portal vortex gains an event-horizon line and travelling spiral sparks;
- speed lines gain independently travelling comet heads;
- bokeh gains crisp specular pins inside soft discs;
- water ripples gain angular caustic breaks;
- pixel burst gains smaller counter-phase chips.

The fifteen non-clean sprite effects retain their authored algorithms and
four-slider controls. C++ templates add restrained secondary filaments grouped
by visual intent: pearlescent aura/dream contours, energized neon/ice/electric
edges, fire/dissolve/impact sparks, quantized hologram/glitch/pixel packets,
spectral ghost/prism/liquid arcs, and a sparse toon-ink cel highlight.
`Original / clean` ID 0 is byte-behavior-preserving at the shader level and
does not receive the secondary layer.

## Runtime contract

The repass preserves:

- kernel names and pointer argument order;
- stable background IDs `2..10` and sprite IDs `0..15`;
- the version-1, 32-dword Spirit control page;
- SIMD16 and 96 bytes of local-ID payload;
- background 2-BTI / 64-byte cross-thread ABI;
- sprite 3-BTI / 96-byte cross-thread ABI;
- clean one-walker and background-enabled ordered two-walker batches;
- GuC post-sync producer release and final Intel cursor `CUR_SURFLIVE` proof.

Both generated manifests record `variant=cpp`, an exact match against the
retained OpenCL C Zebin, exact target `8086:4680` revision `0x0c`, zero
scratch/SLM, all transitive inputs, the reviewed toolchain lock, and
byte-identical two-root reproduction.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `spirit_vfx_background_rgba8.bin` | 98,384 | `de5f6c0837da5d7d0fc52e2a5a97acbdc652d02caf6d853303128d7c562ee848` |
| `spirit_vfx_sprite_rgba8.bin` | 656,728 | `2ee466aa00e631119e8de1eb9fa2d53a1b39d46cc56b4ce2e16ff18f653343ac` |

The background maps at `0x0D430000`; its larger C++ image requires the sprite
mapping at `0x0D450000`. In a two-walker batch the background remains the
instruction base and the sprite entry is relative `0x20040`. A clean batch
uses the sprite mapping as its instruction base and entry `0x40`.

## Bake and review

With the reviewed tools available:

```sh
make intel-gpu-bake-spirit-cpp
make intel-gpu-verify-cpp-artifacts
```

The first command publishes both artifacts reproducibly. The second is
compiler-free and audits all four published C++ artifacts and host regression
tests.

The offline viewers can render both the OpenCL C reference and the published
C++ SPIR-V on the host GPU:

```sh
make -C tools/spirit-vfx-offline render \
  OUTPUT=bld/spirit-vfx-grid-opencl-c.png TIME=2.25

SPIRIT_VFX_BACKGROUND_SPV=crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_background_rgba8.spv \
SPIRIT_VFX_SPRITE_SPV=crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.spv \
make -C tools/spirit-vfx-offline render \
  OUTPUT=bld/spirit-vfx-grid-cpp.png TIME=2.25

SPIRIT_VFX_SPRITE_SPV=crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.spv \
make -C tools/spirit-sprite-vfx-offline render \
  OUTPUT=bld/spirit-sprite-vfx-grid-cpp.png TIME=2.25
```

On the physical TestRig, Shell2 exposes all `9 × 16` combinations without
creating a new renderer or ownership path:

```text
cpp spirit list
cpp spirit
cpp spirit show 3 9
cpp spirit show 9 14
cpp spirit status
cpp spirit clean
```

`cpp spirit` selects the showcase pairing `Magic circle + Electric arc`.
`show` atomically publishes the chosen IDs, their authored parameter defaults,
and their authored palettes into the existing Spirit control panel. `clean`
restores transparent background plus `Original / clean`.
