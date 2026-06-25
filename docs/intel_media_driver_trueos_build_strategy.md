# Intel Media Driver TRUEOS Build Strategy

The upstream `intel/media-driver` checkout is useful as a spec source, but its
normal build target is not a small standalone decoder library. It builds the
Linux VAAPI driver `iHD_drv_video.so` through CMake.

Observed upstream build shape:

- Root build: `/home/t4ce/REPOS/reference/intel-media-driver/CMakeLists.txt`
- Installed artifact: `media_driver/iHD_drv_video.so`
- External stack: LibVA and GmmLib from the README
- OS/device layer: MOS interface, allocator, command-buffer, cache policy,
  surface, DRM/i915-style resource, and optional firmware/user-setting hooks
- AVC decode packet path depends on `AvcPipeline`, `AvcBasicFeature`,
  `DecodeAllocator`, `PMOS_INTERFACE`, `m_mfxItf`, and platform rowstore/cache
  decisions.

Practical conclusion:

1. Building the whole driver for TRUEOS is a port of the Linux userspace media
   stack, not a quick link step. It would require a TRUEOS implementation of the
   driver OS interface and resource allocator before the AVC packets can run.
2. A C ABI shim can still be useful later, but it should be a packet compiler:
   TRUEOS-owned inputs in, validated command dwords and resource requirements
   out. That shim must not own GPU submission, memory management, or VAAPI.
3. The fastest path for the single playback case is the Rust mechanical port:
   keep translating upstream `SETPAR`/`ADDCMD` into typed Rust params and dword
   encoders, then wire the result into TRUEOS' existing media ring and GGTT code.

Candidate C ABI boundary, if we choose to generate from upstream C++ later:

```c
int trueos_intel_avc_build_idr_picture(
    const struct trueos_avc_picture *pic,
    const struct trueos_avc_slice *slice,
    const struct trueos_avc_resources *res,
    uint32_t *out_dwords,
    uint32_t out_dword_capacity,
    uint32_t *out_dword_count);
```

Rules for such a shim:

- No LibVA handles cross the boundary.
- No MOS/PMOS pointers cross the boundary.
- No allocation or GPU submission inside the shim.
- Every output dword still maps back to an upstream `SETPAR` or `ADDCMD`.

For now, keep the shim idea behind the Rust port. If the Rust dword encoders
start diverging too far from upstream, then generate or compile just the packet
compiler layer.
