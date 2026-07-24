# Spirit VFX two-layer contract

Spirit replaces the Lab256 visual with two bounded C++ for OpenCL/IGC
artifacts while
retaining the already-proven Embassy, GuC, GPU producer-fence, and Intel cursor
plane path. The worker pool remains `pool_size = 1`; the other three logical
Spirit fence/pipe channels remain reserved for later activation.

## Presentation chain

One 60 Hz Embassy issue produces one detached GuC submission. The default is
`Transparent + Original / clean`:

1. The clean batch omits the procedural artifact, its walker, and the
   inter-walker cache dependency. The sprite pass starts from transparent and
   presents only the current immutable Lilly RGBA frame.
2. If enabled, `spirit_vfx_background_rgba8` writes every pixel first. It
   supports the selected ten-mode UI background range under stable IDs 2
   through 10: `Energy ring`, `Magic circle`, `Nebula smoke`, `Cyber grid`,
   `Portal vortex`, `Speed lines`, `Bokeh field`, `Water ripples`, and `Pixel
   burst`, plus C++ ID 11 `Magic time circle`.
   `Cyber grid` contains only its moving grid and `Portal vortex` only its
   spiral arms; neither mode adds expanding circular bands. `Nebula smoke`
   preserves an unaffected inner 60%, then uses a broad linear allocation-edge
   alpha ramp terminating in four fully transparent border pixels.
   The media-state/HDC dependency then precedes the sprite walker.
3. The sprite pass implements the complete stable ID range 0 through 15:
   `Original / clean`, `Aura bloom`, `Neon edge`, `Fire rim`, `Ice shimmer`,
   `Hologram`, `RGB glitch`, `Dissolve`, `Ghost trail`, `Electric arc`,
   `Rainbow prism`, `Hit flash`, `Pixel wave`, `Toon ink`, `Liquid warp`, and
   `Dream bloom`.
4. The GuC post-sync marker releases Spirit's GPU producer latch. Only then can
   the worker program `CUR_BASE`; `CUR_SURFLIVE` remains the final display proof.

There is no UI4 publish, framebuffer composition, CPU pixel pass, or synchronous
issuer spin in this chain.

Lilly's idle sequencer transiently selects `Aura bloom` (Sprite shader ID 1)
and `Magic time circle` (background ID 11)
without replacing either persistent control-panel selection. The selection
occurs once on idle entry and remains continuous across blink, posture, and
control-poll boundaries. The prior panel sprite returns at the next non-idle
gesture and the prior background returns after Magic time circle's exit fade.
The background reads TRUEOS UTC seconds-of-day from the existing control-page
time dword. It selects one of 12 broad HH sectors, 60 smaller MM sectors, and
60 thin outer SS sectors; the seconds sector changes only on a wall-clock
second boundary. Its
opacity ramps linearly from zero to one over one second on entry and from its
current value to zero over one second on exit. Only Aura bloom's normalized
grid Param 1 moves, following a two-second
`-0.3 -> 0 -> -0.3` loop. The complete normalized grid setting
`[-0.3..0..-0.3, +1, -1, -1]` maps to the kernel parameters
`[radius 9..12..9, strength 2.5, pulse 0, brighten 0]` and uses Aura bloom's
authored palette.

All procedural backgrounds use a fixed live presentation scale of `1.171875`.
The authored reference radius `0.32` therefore lands at 96 pixels in the
256-pixel surface, leaving a uniform 32-pixel outer margin. This applies to the
complete set, including the transient move portal.
The move transition continues to ramp speed and intensity, but no longer grows
its spatial footprint.

## Preview-compatible control interface

`src/spirit/Spirit_VFX.rs` mirrors all preview labels, ranges, defaults, effect
IDs, background IDs, and particle selections. `SpiritVfxUiConfig` also mirrors
the exact `preview.html::configObject()` object shape:

```text
version, sourceLayout, sprite
transform { x, y, rotationRadians, alphaCutoff, sampling }
shader { id, name, params[4], colorA, colorB }
background { id, name, params[4], colorA, colorB }
particles { type, layer, params[4], color, additive }
output { width, height, alpha }
```

`publish_control_panel_ui_json()` validates and atomically publishes one whole
snapshot. `control_panel_ui_json()` exports the current snapshot with the same
field names and units. The particle controls are intentionally present in the
stable interface before a third particle artifact exists.

Rotation is a complete signed revolution rather than the preview's original
narrow trim control. `set_rotation_degrees()`, `rotate_left_degrees()`, and
`rotate_right_degrees()` accept arbitrary finite turns and canonicalize them
for the shader's `[-2π, 2π]` safety range. Lilly defaults to 180 degrees through
this same transform, correcting its resident-art orientation without a private
flip mode.

Lilly's resident frames are 128x128 and Spirit owns their fixed architectural
mapping into the 256x256 cursor allocation. The former mutable sprite-scale
control is absent from the UI model; control dword 13 remains reserved at
`1.0` solely to preserve the proven kernel ABI.

## Independent cursor movement

Spirit is an AP1 task pair. The VFX task owns GuC submissions, the producer
latch, and `CUR_BASE`; `spirit_cursor_task` exclusively owns `CUR_POS`. Calls to
`move_to()` and `move_by()` publish into a latest-wins Embassy signal. A real
move temporarily overrides only the GPU background with `Portal vortex`, waits
350 ms at the current position, programs the latest requested `CUR_POS`, keeps
the portal behind Lilly at the destination for 150 ms, and then restores the
persistent VFX panel state. The boot-time centered state bypasses the
transition. There is no position interpolation, UI4 dependency, or new movement
API. The returned `SpiritMoveFence` can optionally prove that request or a newer
superseding position has been programmed.

After the first boot-time cursor `SURFLIVE` proves centered Lilly is visible,
the worker issues one ordinary `move_by()` call 512 pipe pixels to the right.
The pixel distance is converted through the active pipe width and therefore
exercises the same Portal transition and latest-wins movement path as any later
caller.

The task is still instantiated only for the active pool limit of one. Its API
is fence/pipe-indexed so increasing the existing Spirit pool limit later creates
the same independent render-plus-motion pair for additional hardware cursor
planes.

## Final edge feather

After background and Lilly have been source-over composed, the sprite kernel
applies one smooth whole-surface alpha feather. The outermost pixel is exactly
transparent and the default 12-pixel transition reaches the untouched composed
result inward. Because the surface is premultiplied BGRA, RGB and alpha are
multiplied by the same factor. `set_edge_fade_pixels()` exposes a bounded 0–16
pixel width, with zero disabling the operation.

## Control page

The GPU reads one 33-dword, versioned control block at PPGTT `0x090A0000`.
It carries selected implemented modes, background parameters and
colors, transform, sampling, alpha cutoff, sprite parameters and colors,
resident-source geometry, destination pitch, UI revision, the half-second
presentation-rate estimate, final edge-feather width, and an append-only clock
extension. Dword 4 always carries smooth animation-frame time for the sprite
walker and background IDs 2 through 10. Dword 32 carries exact integer UTC
seconds-of-day for `Magic time circle` ID 11 only. Both kernel entry
points validate magic/version and their own surface dimensions.

## Offline selection grids

`tools/spirit-vfx-offline` renders all ten procedural backgrounds.
`tools/spirit-sprite-vfx-offline` independently renders the complete 16-mode
Sprite shader set as a 4x4 grid. Both tools can compile the OpenCL C reference
or dispatch the published C++ SPIR-V through `clCreateProgramWithIL` on a host
GPU; neither carries a CPU or duplicate shader implementation.

## Baked ADL-S artifacts

`ocloc validate` decodes each artifact successfully and reports one kernel:

| Artifact | Bytes | BTIs | Cross-thread | Per-thread | SHA-256 |
| --- | ---: | ---: | ---: | ---: | --- |
| `spirit_vfx_background_rgba8.bin` | 109,624 | 2 | 64 | 96 | `4c82592a441b88902491b6d3cf8f99a3524a41178a347928cf5845e0872841f7` |
| `spirit_vfx_sprite_rgba8.bin` | 656,728 | 3 | 96 | 96 | `2ee466aa00e631119e8de1eb9fa2d53a1b39d46cc56b4ce2e16ff18f653343ac` |

Both artifacts use text offset `0x40` within their own Zebin. The clean default
maps only the sprite at `0x0D450000`, uses that mapping as the instruction base,
and addresses its entry at relative offset `0x40`. With a procedural background
enabled, the runtime additionally maps it at `0x0D430000`, selects that mapping
as the shared instruction base, and addresses the sprite at relative offset
`0x20040`.

The full C++ visual map and physical `cpp spirit` selector are documented in
[`SPIRIT_CPP_REPASS.md`](SPIRIT_CPP_REPASS.md).
