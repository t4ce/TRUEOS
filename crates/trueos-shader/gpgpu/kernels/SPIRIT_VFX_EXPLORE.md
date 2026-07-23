# Spirit VFX two-layer contract

Spirit replaces the Lab256 visual with two bounded OpenCL artifacts while
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
   supports UI background IDs 1 (`Radial aura`), 4 (`Nebula smoke`), and 6
   (`Portal vortex`), followed by the media-state/HDC dependency before the
   sprite walker.
3. The sprite pass supports shader IDs 0 (`Original / clean`) and 1
   (`Aura bloom`); unsupported sprite effects deliberately use the clean pass.
4. The GuC post-sync marker releases Spirit's GPU producer latch. Only then can
   the worker program `CUR_BASE`; `CUR_SURFLIVE` remains the final display proof.

There is no UI4 publish, framebuffer composition, CPU pixel pass, or synchronous
issuer spin in this chain.

## Preview-compatible control interface

`src/spirit/Spirit_VFX.rs` mirrors all preview labels, ranges, defaults, effect
IDs, background IDs, and particle selections. `SpiritVfxUiConfig` also mirrors
the exact `preview.html::configObject()` object shape:

```text
version, sourceLayout, sprite
transform { scale, x, y, rotationRadians, alphaCutoff, sampling }
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

Lilly's resident frames are 128x128, so the default transform scale is `0.5`
of the 256x256 cursor allocation. That preserves one source pixel per displayed
pixel while retaining the complete generic scale API for later presentation.

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

The GPU reads one 32-dword, versioned control block at PPGTT `0x090A0000`.
It carries frame time, selected implemented modes, background parameters and
colors, transform, sampling, alpha cutoff, sprite parameters and colors,
resident-source geometry, destination pitch, UI revision, the half-second
presentation-rate estimate, and final edge-feather width. Both kernel entry
points validate magic/version and their own surface dimensions.

## Baked ADL-S artifacts

`ocloc validate` decodes each artifact successfully and reports one kernel:

| Artifact | Bytes | BTIs | Cross-thread | Per-thread | SHA-256 |
| --- | ---: | ---: | ---: | ---: | --- |
| `spirit_vfx_background_rgba8.bin` | 48,056 | 2 | 64 | 96 | `d21a1ea62f9ab6f1c869ffd35d1a598988acc6905cabbe163e4c2082188f0548` |
| `spirit_vfx_sprite_rgba8.bin` | 73,728 | 3 | 96 | 96 | `7baa6b3613d9656ea1920f3eb4e28eeba88d939f54e0f6fbc7373ff163710b33` |

Both artifacts use text offset `0x40` within their own zebin. The clean default
maps only the sprite at `0x0D440000`, uses that mapping as the instruction base,
and addresses its entry at relative offset `0x40`. With a procedural background
enabled, the runtime additionally maps it at `0x0D430000`, selects that mapping
as the shared instruction base, and addresses the sprite at relative offset
`0x10040`.
