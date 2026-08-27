# Build-produced Picasso renderer assets

`simple-cube.trueos.intel.helio` is generated and validated by
`tools/helio-build/build-simple-cube.sh`. It is kept at this stable path so the
TRUEOS build can embed the exact frontend IR and Intel native shader package
without reconstructing any scene data. The same container carries the
versioned, pointer-free scene contracts used by Shell2's retained demos,
including `scene/sprite-dig-v1.bin` for example 5.
It also carries `scene/portal-rooms-v1.bin` for example 6.

`churn-forward.trueos.intel.helio` is independently generated and validated by
`tools/helio-build/build-churn-forward.sh`. It preserves the working cube
program while adding Helio's GPU-native camera/instance/compaction/indirect
contract and the matching Intel executable and fixed-function state.

`helio-gbuffer/` is independently generated and validated by
`tools/helio-build/build-gbuffer.sh`. It is the first native compilation of
Helio's unmodified deferred G-buffer shader against its actual Vulkan ABI:
two bind groups, 256-wide bindless image/sampler arrays, 40-byte vertices,
eight color targets, and `D32_SFLOAT` depth. It is compiler evidence and does
not yet replace either launchable TRUEOS retained program.

The Sprite Dig contract preserves the hosted demo's world dimensions,
movement, jumping, three-stage mining, inventory/hotbar selection, and block
placement. UI4 supplies its keyboard, cursor, mouse-button, wheel, focus, and
resize events through the shared winit-shaped bridge.

The Portal Rooms contract preserves the six portal frames, themed room
materials, and 74 texture-free furniture/shell objects from Helio's hosted
demo. TRUEOS constructs retained box and octa-sphere geometry, clips it in
homogeneous space to each portal opening, and submits the resulting indexed
batches through the same GuC/UI4 path. UI4 supplies fly-camera input; Tab
toggles the checkerboard portal overlay.

`sprite-dig-atlas.trueos.rgba` is independently built from the sibling Helio
checkout by `tools/helio-build/build-sprite-dig-atlas.sh`. It contains the
selected native terrain and character frames plus deterministic crack stages.
TRUEOS uploads it once and renders terrain and ordered sprite overlays through
the Bakery C++ `sprite_quad_worklist_rgba8` SPIR-V/Zebin path. The terrain is
one bounded full-frame tilemap walker rather than thousands of quad walkers;
there is no CPU texture rasterization, readback, or frame copy.
