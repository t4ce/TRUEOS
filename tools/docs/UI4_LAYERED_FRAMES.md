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

Layered windows currently open and close atomically without the legacy
single-plane scaler puff animation. This avoids splitting paired geometry or
retiring one member during another member's animation. Close checks both
producers for unretired work. ShaderToy marks an operation in flight before
unlocking its surface record, including when called from a native worker.

## Cubes consumer and validation

Cubes uses one layered Frame. Its normal Picasso loop renders the foreground
with transparent premultiplied clear pixels. A separate native worker owns the
Mandelbox background, using authenticated ShaderToy program 16. It consumes a
coherent latest camera/theme/extent command, renders only on change, and has a
10 Hz maximum cadence. The worker drains before the parent Frame closes.

Key 5's 27 worlds use the six palette colors authored in
`Cubes/Cube/cube_tree_builder_world_ramps.html`, with 1–3-theme territories and
magenta Void. Other modes publish a transparent background. The first port is a
bounded Image pass with a fixed fractal origin, camera yaw/pitch and matching
field of view. The reference HTML's cached cubemap optimization is not yet used;
there is no new renderer or unreviewed runtime shader compilation.

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

A host UHD 770 smoke render verifies the baked SPIR-V produces theme-dependent,
repeatable pixels. TRUEOS multi-window drag, slot-0 promotion/demotion, paired
resize and physical SURFLIVE still require a rebuilt kernel/Blueprint rig run.
