# Build-produced Picasso renderer assets

`simple-cube.trueos.intel.helio` is generated and validated by
`tools/helio-build/build-simple-cube.sh`. It is kept at this stable path so the
TRUEOS build can embed the exact frontend IR and Intel native shader package.
The same container carries the versioned, pointer-free scene contracts used
by the `helio_churn_trueos` and `helio_portal_trueos` Blueprints.

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

The Portal Rooms contract preserves the six portal frames, themed room
materials, and 74 texture-free furniture/shell objects from Helio's hosted
demo. TRUEOS constructs retained box and octa-sphere geometry, clips it in
homogeneous space to each portal opening, and submits the resulting indexed
batches through the same GuC/UI4 path. UI4 supplies fly-camera input; Tab
toggles the checkerboard portal overlay.
