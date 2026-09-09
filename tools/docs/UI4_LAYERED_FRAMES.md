# UI4 layered frame contract v1

One Blueprint window owns a streaming/triple foreground and a visual/double
background. They share owner, session, placement, visibility, focus, hit testing,
resize epoch and close. Each surface has its own write lease, cadence, GPU
submission state, completion receipt and published front. This is two image
layers; the existing double/triple memory-buffer ownership still applies to each.

`trueos_cabi_ui4_scene_frame_open_layered_v1` creates the pair;
`trueos_cabi_ui4_scene_frame_layer_v1` returns a producer target (0 foreground,
1 background). Existing scene and vGPU operations accept that target. Layer
capabilities retain the full window generation in a separate surface namespace;
there is only one WindowRecord and one input route. A background capability can
be sent to a native worker. Closing its parent revokes it; it cannot close a
second window. Existing ABI signatures remain unchanged.

`BackgroundLayer::set_opacity` uses the existing opacity ABI with its producer
target. Its factor multiplies parent opacity on both direct scanout and slot-0
composition; it does not dim the foreground. The factor survives paired resize.

## Plane budget and interaction

Slot 0 is shared composition; slots 1–3 are hardware leases; slot 4 remains
reserved for interaction. A single window costs one lease and a layered window
two. A pair receives adjacent slots or stays entirely on slot 0. One spare slot
cannot admit half a pair. Multiple layered windows can be created; the frame
contract does not promise permanent hardware residency.

Create/close rebalancing packs a complete top suffix in broker z order. Hot
interaction refreshes both leases of a pair. Focus raises the entire group.
Promoting a stacked pair plans sufficient whole-group revocations before making
any change: two idle singles may be demoted, while an actively dragged pair and
single retain their leases under the existing 500 ms grace. Failed admission
leaves every holder unchanged. A demoted pair contributes its last published
background followed by its foreground to the slot-0 painter, under the same
window rectangle; producers need not redraw merely because a lease changes.

Compositor stamps include both window and layer identity. Direct scanout pins
the exact member surface until the display's batched SURFLIVE boundary. A late
acknowledgment cannot clear a newer layer/geometry publication. Input and
informational broker snapshots still contain one window.

## Resize and teardown

Both replacement rings are allocated before either producer changes. The old
pair stays presented at its common old geometry until both replacements have
published. Frame handles, geometry and the first new publications then commit
under one broker lock. Superseded replacements roll back together. Old storage
retires only after all display/compositor readers release it.
The first replacement publication bypasses the normal visual cadence deadline,
while retaining all write/read ownership checks and the two-layer commit barrier.

Layered windows currently open and close atomically without the legacy
single-plane scaler puff animation. This avoids splitting paired geometry or
retiring one member during another member's animation. Close checks both
producers for unretired work. ShaderToy marks an operation in flight before
unlocking its surface record, including when called from a native worker.

## Cubes consumer and validation

Cubes uses one layered Frame. Its normal Picasso loop renders the foreground
with transparent premultiplied clear pixels. A separate native worker owns the
Chroma background through authenticated ShaderToy program 16. Each Key 5 world
selection bakes a complete six-face 1024px cubemap once, with the authored one,
two or three theme colors. World 27 alone uses the Folded Core/void preset;
the other 26 use Box Cathedral from `Cubes/Cube/the_one_cube_chroma.html`.

The window-owned GPU map survives resize. Camera orientation, field of view
and extent changes use only its bilinear sampler; a damped quaternion follower
trails the foreground camera and stops requesting frames once settled. The
background has a 60 Hz ceiling. The previous published front remains visible
during a replacement bake, which uses bounded GPU batches and publishes only
after all faces complete. Background opacity remains 128/255. Keys 1–4 retain
a neutral slate; Key 5 shades both hemispheres. See
[`Cubes/tools/LAYERED_BACKGROUND.md`](../../../Cubes/tools/LAYERED_BACKGROUND.md)
for cache ownership, controls, reference provenance and current validation.

Host checks:

- `python3 tools/test_ui4_layer_contract.py`
- `python3 tools/test_ui4_layered_resize.py`
- `python3 tools/shadertoy-cpp-offline/test_blueprint_packages.py`
- `cargo check --bin TRUEOS`
- `cargo check --manifest-path api/Cargo.toml --features ui4-scene` in Blueprints
- `cargo check --target x86_64-unknown-linux-gnu` and
  `python3 tools/test_background.py` in Cubes
- `python3 tools/bake_mandelbox.py` in Cubes performs a reproducible native bake
  and updates the Blueprint package and kernel's hash/ABI metadata together.

The current Chroma shader was compiled reproducibly and its host logic/admission
checks passed. GPU runtime and TRUEOS multi-window drag, slot-0 promotion/demotion,
paired resize and physical SURFLIVE still require a matching kernel/Blueprint run.
