# Helio SimpleCube runtime build

`build-simple-cube.sh` is the single paved build-time path for the first Helio
program. It captures Helio's real `build_simple_graph`, lowers its wgpu trace
to the pointer-free `HELIOIR` contract, and emits `render/replay-v1.bin` with
Helio/wgpu's exact 20-byte `DrawIndexedIndirectArgs`. The replay plan carries
artifact resource IDs and the source-IR CRC, never GPU addresses or patched
Intel packets. The same build compiles the captured WGSL through the existing
Intel baker, validates every container CRC, replay/IR link, and native shader
hash, then atomically publishes:

```text
assets/helio/simple-cube.trueos.intel.helio
```

Run from anywhere:

```sh
tools/helio-build/build-simple-cube.sh
```

The default Helio checkout is the sibling `../Helio`. A different checkout
can supply the real capture with `HELIO_REPO=/path/to/Helio`. Set
`INTEL_DEVICE_ID=0x....` only when the Intel Vulkan compiler device must be
selected explicitly.

The final rename occurs in `assets/helio`, so a failed capture, compile, or
validation leaves the previously published runtime artifact intact. To check
the checked-in artifact without rebuilding it:

```sh
tools/helio-build/build-simple-cube.sh --validate-only
```
