# Cursor Lost + Found

This note captures the recovered history of the old hardware cursor path around the virgl backend, with enough detail to salvage it later without redoing the archaeology.

## Short Version

- The current checked-out tree does not have a hardware cursor plane path in the virgl backend.
- The current cursor path is a software overlay in `src/r/io_cursor.rs` that draws RGB cross markers.
- A real virtio-gpu hardware cursor path existed briefly in early March 2026.
- That path used the virtio cursor queue, `UPDATE_CURSOR`, and `MOVE_CURSOR` in `src/gfx/virtio_gpu_3d.rs`.
- The cursor texture changed once while that path existed.
- The old path was then removed wholesale.

## Current State

Current checked-out state uses:

- `src/r/io_cursor.rs`
- `src/gfx/cursor.rs`
- `src/gfx/virtio_gpu_3d.rs`

What is there now:

- `src/r/io_cursor.rs` implements `kernel_cursor_overlay_tick()` and emits software cursor overlay draws.
- The overlay currently builds colored RGB cross shapes, not a textured pointer sprite.
- `src/gfx/cursor.rs` is currently just a thin forwarding helper to the overlay tick path.
- `src/gfx/virtio_gpu_3d.rs` no longer contains cursor queue support or virtio cursor commands.

What is not there now:

- No `VIRTIO_GPU_CMD_UPDATE_CURSOR`
- No `VIRTIO_GPU_CMD_MOVE_CURSOR`
- No `QUEUE_CURSOR`
- No `VirglCursorPlane`
- No `hw_cursor_define_bgra()` implementation in the current virgl backend
- No `hw_cursor_move()` implementation in the current virgl backend

## Commit Chain

These are the commits that matter for recovering the path.

### 1. `b5d114ba` - initial real hardware cursor path

This is the earliest important commit in the recovered path.

Relevant files:

- `src/gfx/virtio_gpu_3d.rs`
- `src/v/spawn_service.rs`

What it added in `src/gfx/virtio_gpu_3d.rs`:

- `VIRTIO_GPU_CMD_UPDATE_CURSOR = 0x0300`
- `VIRTIO_GPU_CMD_MOVE_CURSOR = 0x0301`
- `QUEUE_CURSOR = 1`
- `cursorq: Option<VirtQueue>` in `VirtioGpu3d`
- `CursorPos`
- `CmdUpdateCursor`
- `has_cursor_queue()`
- `update_cursor(...)`
- `move_cursor(...)`
- `cursor_submit_bytes(...)`
- `VirglCursorPlane`
- `cursor_plane: Option<VirglCursorPlane>` in `VirglGfxBackend`
- `define_hw_cursor_bgra(...)`
- `move_hw_cursor(...)`
- `hw_cursor_supported()`
- `hw_cursor_define_bgra(...)`
- `hw_cursor_move(...)`

This was a real hardware cursor path, not a software overlay.

The backend behavior was:

1. Allocate/recreate a 2D resource for the cursor image.
2. Attach backing memory.
3. Copy BGRA cursor pixels into backing.
4. `transfer_to_host_2d()`
5. `resource_flush()`
6. `update_cursor(...)`
7. Later move with `move_cursor(...)`

The old implementation block lives around these lines in that commit:

- `src/gfx/virtio_gpu_3d.rs`: `VirglCursorPlane` around line 1612
- `src/gfx/virtio_gpu_3d.rs`: cursor define/move code around lines 3199 to 3299

### 2. `b5d114ba` - first cursor texture: white arrow-like bitmap

In the same commit, the cursor image generator lived in:

- `src/v/spawn_service.rs`

Function:

- `build_default_cursor_shape_bgra(width, height)`

This first version generated a simple white arrow-like sprite by writing opaque white pixels into a small triangular / pointer-ish shape near the top-left of the 64x64 image.

The important characteristics were:

- White opaque cursor shape
- Shape anchored near top-left
- Hardware hotspot passed as `(0, 0)`

This is the closest thing to the classic pointer texture in the recovered history.

Relevant call site in that commit:

```rust
ctx.hw_cursor_define_bgra(CURSOR_W, CURSOR_H, 0, 0, cursor_pixels.as_slice())
```

That means:

- sprite was treated like a top-left anchored pointer
- hotspot was not centered

### 3. `8d156538` - texture changed from arrow-like sprite to centered ring/disk

This is the main visual change worth remembering.

Relevant files:

- `src/v/spawn_service.rs`
- `src/gfx/cursor.rs`

This commit changed the hardware cursor sprite generator in `src/v/spawn_service.rs`.

Before this commit, the generator made a white arrow-like bitmap.

After this commit, it made:

- black outer ring
- white inner disk
- centered inside the 64x64 texture

It also changed the hotspot from top-left to centered:

```rust
let hot_x = CURSOR_W / 2;
let hot_y = CURSOR_H / 2;
```

And the define call changed from:

```rust
ctx.hw_cursor_define_bgra(CURSOR_W, CURSOR_H, 0, 0, cursor_pixels.as_slice())
```

to:

```rust
ctx.hw_cursor_define_bgra(
    CURSOR_W,
    CURSOR_H,
    hot_x,
    hot_y,
    cursor_pixels.as_slice(),
)
```

This is almost certainly the cursor texture change that was being remembered.

This commit also:

- introduced a `gfx_virgl_cursor_overlay_task()`
- added a new `src/gfx/cursor.rs` file, but at this point it was only a tiny forwarder
- disabled the `gfx-hw-cursor` task in the task table

So the hardware cursor still existed in code, but the system direction was already drifting toward overlay composition.

### 4. `3339c35e` - moved the hardware cursor task into `src/gfx/cursor.rs`

This commit moved the ring/disk hardware cursor task and sprite generator into:

- `src/gfx/cursor.rs`

This matters because if you want to recover the cleaner separated version of the hardware cursor task, this commit is a better salvage source than the original `spawn_service` version.

The ring/disk version in this commit used:

- centered black ring / white disk sprite
- centered hotspot
- retry / reinit logic if initial move failed
- polling of cursor events and forwarding to `ctx.hw_cursor_move(...)`

### 5. `554d0acf` - removed the hardware cursor path entirely

This is the removal commit.

Relevant files:

- `src/gfx/cursor.rs`
- `src/gfx/virtio_gpu_3d.rs`

What it removed from `src/gfx/cursor.rs`:

- the `gfx_hw_cursor_task()` body
- the cursor sprite generator
- all the hardware cursor task logic

What it removed from `src/gfx/virtio_gpu_3d.rs`:

- `VIRTIO_GPU_CMD_UPDATE_CURSOR`
- `VIRTIO_GPU_CMD_MOVE_CURSOR`
- `QUEUE_CURSOR`
- `CursorPos`
- `CmdUpdateCursor`
- `cursorq`
- `has_cursor_queue()`
- `update_cursor(...)`
- `move_cursor(...)`
- `cursor_submit_bytes(...)`
- `VirglCursorPlane`
- `cursor_plane`

After this commit, the virgl backend no longer had a real hardware cursor implementation.

## The Actual Texture Transition

This is the key visual before/after.

### Before: initial hardware cursor texture

Source commit:

- `b5d114ba`

Source file:

- `src/v/spawn_service.rs`

Characteristics:

- opaque white arrow-like bitmap
- drawn near the top-left of the texture
- hotspot at `(0, 0)`
- behaved like a normal pointer texture

### After: revised hardware cursor texture

Source commits:

- `8d156538`
- `3339c35e`

Source files:

- `src/v/spawn_service.rs`
- later `src/gfx/cursor.rs`

Characteristics:

- black outer ring
- white inner disk
- centered in texture
- hotspot at `(CURSOR_W / 2, CURSOR_H / 2)`
- behaved like a marker centered on the physical cursor location

So the remembered “we somehow changed the cursor texture” is correct.

The concrete change was:

- old pointer-ish white bitmap -> centered ring/disk marker
- top-left hotspot -> centered hotspot

## Key Recovery Locations

If salvaging later, these are the best exact sources.

### Best source for the original pointer-like cursor texture

- Commit: `b5d114ba`
- File: `src/v/spawn_service.rs`
- Function: `build_default_cursor_shape_bgra`

### Best source for the refined hardware cursor task split out into its own file

- Commit: `3339c35e`
- File: `src/gfx/cursor.rs`

### Best source for the virgl backend cursor queue implementation

- Commit: `b5d114ba`
- File: `src/gfx/virtio_gpu_3d.rs`

### Best source for the exact removal diff

- Commit: `554d0acf`
- Files:
  - `src/gfx/cursor.rs`
  - `src/gfx/virtio_gpu_3d.rs`

## Recommended Salvage Strategy

If rebuilding this later, the least confusing route is:

1. Recover the virtio cursor queue support from `b5d114ba` in `src/gfx/virtio_gpu_3d.rs`.
2. Recover the task shape from `3339c35e` in `src/gfx/cursor.rs`.
3. Decide which sprite you actually want:
   - `b5d114ba` for the arrow-like pointer texture
   - `3339c35e` / `8d156538` for the centered ring/disk marker
4. Re-enable whichever task wiring is appropriate in the modern spawn path.
5. Check whether today’s gfx-core traits / context APIs still match the old `hw_cursor_*` methods or need adaptation.

## Why This Disappeared

The recovery trail suggests the project moved away from the dedicated hardware cursor queue and toward a generic overlay path.

Evidence:

- overlay task introduced while hardware cursor path still existed
- hardware cursor task later disabled
- hardware cursor implementation then removed from virgl backend
- current tree keeps only software overlay rendering

So this was not just a small refactor. The old path was intentionally abandoned.

## Practical Notes For Future Restoration

- The old virtio path used a separate cursor queue, not the normal control queue.
- The cursor image format was BGRA in backing memory.
- Cursor resource creation used `resource_create_2d(...)` with `VIRGL_FORMAT_B8G8R8A8_UNORM`.
- Define path required backing copy, transfer, flush, then `update_cursor(...)`.
- Move path used `move_cursor(...)` only.
- The arrow version and the ring/disk version are both 64x64.

## Minimal Fact Table

| Question | Answer |
| --- | --- |
| Did a real hardware cursor path exist? | Yes |
| Was it in the virgl backend? | Yes |
| Did it use virtio cursor queue commands? | Yes |
| Did the cursor texture change while that path existed? | Yes |
| Earliest recovered texture | white arrow-like bitmap |
| Later recovered texture | centered black ring + white disk |
| Was the hotspot changed too? | Yes, from `(0,0)` to centered |
| Is any of that in current HEAD? | No |
| Current cursor implementation | software RGB cross overlay |

## Commits To Revisit First

- `b5d114ba`
- `8d156538`
- `3339c35e`
- `554d0acf`

## Final Take

The “good” lost path was real, but there were actually two good historical variants:

- the earlier pointer-like hardware cursor texture
- the later centered marker hardware cursor texture

If the goal is to recover something pointer-shaped, `b5d114ba` is the key commit.
If the goal is to recover the cleaner later structure, `3339c35e` is the best code organization starting point.