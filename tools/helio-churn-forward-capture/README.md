# Helio Churn forward capture

This host tool records the first multi-instance Helio-origin render program
without modifying the sibling Helio checkout. It uses Helio's real public GPU
types for the 368-byte camera, 208-byte `GpuInstanceData`, and 20-byte
`DrawIndexedIndirectArgs`. One indexed unit cube is instanced across a small
Churn-style ring and submitted by `draw_indexed_indirect` through Helio's
vendored wgpu.

The deliberately slim forward shader keeps the transform path and instance
indirection from Helio's `ForwardLitPass`, but uses a fixed directional light
and four constant material colors. This leaves one render pipeline and three
bindings as the smallest useful bridge below Helio/wgpu:

- group 0, binding 0: read-only camera storage buffer (stride 368)
- group 0, binding 1: read-only instance storage buffer (stride 208)
- group 0, binding 2: read-only compacted-instance `u32` buffer

The capture is Churn-only and is not an additive mutation of the working
SimpleCube artifact. It emits a frontend HELIOA container with these sections:

- `render/churn-forward.wgsl`
- `scene/churn-forward-v1.bin`
- `wgpu/*`
- `capture/adapter.txt`

Run it from the sibling Helio checkout. Starting Cargo there keeps TRUEOS's
kernel-only Cargo config out of this native host build and also reuses Helio's
existing wgpu build cache:

```sh
(
  cd ../Helio
  cargo run --release \
    --target x86_64-unknown-linux-gnu \
    --manifest-path ../TRUEOS/tools/helio-churn-forward-capture/Cargo.toml -- \
    ../TRUEOS/tgt/helio-churn-forward/churn-forward.trueos.helio
)
```

The Intel packaging stage consumes this frontend independently and publishes
`picasso/churn-forward.trueos.intel.helio`; it must not replace
`simple-cube.trueos.intel.helio`.
