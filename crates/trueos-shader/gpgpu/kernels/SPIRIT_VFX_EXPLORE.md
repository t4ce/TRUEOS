# Spirit VFX two-layer contract

Spirit replaces the Lab256 visual with two bounded OpenCL artifacts while
retaining the already-proven Embassy, GuC, GPU producer-fence, and Intel cursor
plane path. The worker pool remains `pool_size = 1`; the other three logical
Spirit fence/pipe channels remain reserved for later activation.

## Presentation chain

One 60 Hz Embassy issue produces one detached GuC submission:

1. `spirit_vfx_background_rgba8` writes every pixel of the exact 256x256
   premultiplied-BGRA cursor backbuffer. It supports UI background IDs 1
   (`Radial aura`) and 4 (`Nebula smoke`).
2. A media-state flush plus HDC/cache invalidation makes that output visible to
   `spirit_vfx_sprite_rgba8` in the same batch.
3. The sprite pass transforms and source-over composites the current immutable
   resident Lilly RGBA frame. It supports shader IDs 0 (`Original / clean`) and
   1 (`Aura bloom`); unsupported sprite effects deliberately use the clean pass.
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

## Independent cursor movement

Spirit is an AP1 task pair. The VFX task owns GuC submissions, the producer
latch, and `CUR_BASE`; `spirit_cursor_task` exclusively owns `CUR_POS`. Calls to
`move_to()` and `move_by()` publish into a latest-wins Embassy signal, so motion
bursts coalesce and never wait for a shader frame, buffer flip, UI4 compositor,
or UI4 software-cursor cadence. The returned `SpiritMoveFence` can optionally
prove that request or a newer superseding position has been programmed.

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
| `spirit_vfx_background_rgba8.bin` | 44,672 | 2 | 64 | 96 | `cfe755a9f79f629a277cef05c95bd7a22561cb9b07414ac299ba7490779ac93e` |
| `spirit_vfx_sprite_rgba8.bin` | 73,336 | 3 | 96 | 96 | `18ba9e74adb8adb798ff7d4b73b835c7f657093ca4ceebf759207896630d3bd1` |

Both artifacts use text offset `0x40` within their own zebin. The runtime maps
them at `0x0D430000` and `0x0D440000`, selects the first mapping as the shared
instruction base, and addresses the sprite entry at relative offset `0x10040`.
