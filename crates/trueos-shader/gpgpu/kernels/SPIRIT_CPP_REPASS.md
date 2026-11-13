# Spirit C++/IGC visual repass

Spirit's C++ shaders provide the visual vocabulary without replacing its
runtime architecture. The two `.clcpp` entry sources and their headers compile through
Clang C++ for OpenCL, SPIR-V, and Intel IGC. They are the sole maintained
Spirit shader sources; there is no parallel OpenCL-C implementation.

The compiler stack is build-time only. TRUEOS embeds the audited SPIR-V and
Zebin images; the running OS contains no Clang, C++ runtime, `llvm-spirv`,
`ocloc`, or IGC.

## What changed visually

The nine original background IDs retain their large-form ideas:

- energy ring gains counter-rotating beads and a fine inner corona;
- magic circle gains two rotating rune belts;
- nebula smoke gains sparse, edge-safe star seeds;
- cyber grid gains pulsing circuit intersections;
- portal vortex gains an event-horizon line and travelling spiral sparks;
- speed lines gain independently travelling comet heads;
- bokeh gains crisp specular pins inside soft discs;
- water ripples gain angular caustic breaks;
- pixel burst gains smaller counter-phase chips.

Background ID 11, `Magic time circle`, retains the segmented Magic circle
grammar but turns it into a UTC clock face. Twelve broad hour sectors, sixty
smaller minute sectors, and one thin outer seconds sector consume integer
seconds-of-day through the existing time dword. HH, MM, and SS are quantized
before pixel math, so the seconds sector changes exactly once per second.

Background IDs 12 and 13 append the procedural knot studies selected from
`tools/lilly/spirit-knot-vfx-offline/knot.frag`. Their shared geometry lives in
`include/spirit_vfx_knot.hpp`; no logo texture or mask is uploaded.

Terminal activity selects **Spark lattice (12)** while Codex is working and
**Resonance (13)** after a completed turn remains unread. Codex's existing
acknowledgement report releases the ready effect on CLI interaction. Blocked
input keeps Water ripples. These are transient overrides of the persistent panel.

Spark lattice has fixed speed **3**, opacity **0.75**, and scale **1.4**.
Intensity follows a smooth cosine cycle **1 → 4 → 1 over seven seconds** of
wall time, independently of shader speed. Repeated status reports do not restart
it. Resonance retains its preview speed **1** and intensity **2**, with the same
scale and opacity. Entry fades opacity over 650 ms without ramping speed.
On exit, each knot expands from its current scale to **3** over 650 ms, then
fades opacity to zero over another 650 ms before selecting the next background.
Resuming the outgoing status reverses its scale and opacity continuously;
new requests during exit replace the pending effect without restarting exit.

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
- stable original background IDs `2..10`, appended C++ background IDs `11..13`, and
  sprite IDs `0..15`;
- the version-1 layout in dwords 0–31 plus append-only clock dword 32;
- SIMD16 and 96 bytes of local-ID payload;
- background 2-BTI / 64-byte cross-thread ABI;
- sprite 3-BTI / 96-byte cross-thread ABI;
- clean one-walker and background-enabled ordered two-walker batches;
- GuC post-sync producer release and final Intel cursor `CUR_SURFLIVE` proof.

Both generated manifests record `variant=cpp-native` under
`cpp-native-aot-v1`, with no legacy ABI-reference dependency. They retain the
reviewed exact target `8086:4680` revision `0x0c`, zero scratch/SLM, all
transitive inputs, the reviewed toolchain lock, the exact expected kernel set,
and byte-identical two-root reproduction.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `spirit_vfx_background_rgba8.bin` | 224,248 | `af8097d1f52e914091e480f3ee43111cab92b5bba19bd2f73f3f3456e9a506d8` |
| `spirit_vfx_sprite_rgba8.bin` | 656,728 | `a5bca7b15af0bcab952d093a184876fed01fbce4e4c7d55feffaaf55c33ce5f7` |

The background maps at `0x0D430000`; its larger C++ image requires the sprite
mapping at `0x0D470000`. In a two-walker batch the background remains the
instruction base and the sprite entry is relative `0x40040`. Compile-time bounds
checks keep both images disjoint and below the demo window. A clean batch
uses the sprite mapping as its instruction base and entry `0x40`.

Host regression checks for the activity envelope, control page, and submission
encoding:

```sh
python3 tools/testpy/test_spirit_codex_background.py
python3 tools/testpy/test_spirit_vfx_command_frontier.py
python3 tools/testpy/test_spirit_vfx_ppgtt_evidence.py
```

The first tool can also export a 30 Hz working → unread-ready → acknowledged
timeline from the actual Rust state machine with
`--timeline bld/spirit-codex-activity.csv`.

## Bake and review

With the reviewed tools available:

```sh
make intel-gpu-bake-spirit-cpp
make intel-gpu-verify-cpp-artifacts
```

The first command publishes both artifacts reproducibly. The second is
compiler-free and audits all four published C++ artifacts and host regression
tests.

The offline viewers load the published C++ SPIR-V on the host GPU. They contain
no alternate shader implementation or source-compilation fallback:

```sh
make -C tools/spirit-vfx-offline render \
  OUTPUT=bld/spirit-vfx-grid-cpp.png TIME=2.25

make -C tools/spirit-sprite-vfx-offline render \
  OUTPUT=bld/spirit-sprite-vfx-grid-cpp.png TIME=2.25
```

On the physical TestRig, Shell2 exposes all `10 × 16` combinations without
creating a new renderer or ownership path:

```text
cpp spirit list
cpp spirit
cpp spirit show 3 9
cpp spirit show 11 1
cpp spirit show 9 14
cpp spirit status
cpp spirit clean
```

`cpp spirit` selects the showcase pairing `Magic circle + Electric arc`.
`show` atomically publishes the chosen IDs, their authored parameter defaults,
and their authored palettes into the existing Spirit control panel. `clean`
restores transparent background plus `Original / clean`.

Idle mode selects `Magic time circle + Aura bloom` transiently. TRUEOS prefers
NTP and falls back to the bootloader-provided timestamp; its system timezone is UTC.
The sprite walker retains smooth 60 Hz animation time in dword 4, while only
the clock background reads quantized wall time from dword 32.
All live backgrounds share the single `SPIRIT_BACKGROUND_PRESENT_SCALE` value
`1.171875`, mapping the authored `0.32` reference radius to 96 pixels and
leaving a 32-pixel margin in the 256-pixel cursor surface.
