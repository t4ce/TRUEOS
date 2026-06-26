# Intel Media Driver TRUEOS Build Strategy

The upstream `intel/media-driver` checkout must be compiled as a Linux-side
oracle before TRUEOS trusts any hand-ported AVC packet stream. Its normal build
target is not a small standalone decoder library; it builds the Linux VAAPI
driver `iHD_drv_video.so` through CMake.

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
2. Building the upstream Linux driver is still mandatory as an oracle. Use
   `tools/build_intel_media_driver_oracle.sh` to compile the pinned
   `a203cfc` checkout and write the TRUEOS AVC recipe trace next to the driver
   artifact under `bld/intel-media-driver-oracle/`.
3. A C ABI shim can still be useful later, but it should be a packet compiler:
   TRUEOS-owned inputs in, validated command dwords and resource requirements
   out. That shim must not own GPU submission, memory management, or VAAPI.
4. The fastest path for the single playback case is the Rust mechanical port:
   keep translating upstream `SETPAR`/`ADDCMD` into typed Rust params and dword
   encoders, but treat the compiled upstream driver/oracle as the reference
   implementation for every packet field before wiring into TRUEOS' existing
   media ring and GGTT code.

Current host gate:

```sh
tools/build_intel_media_driver_oracle.sh
```

The wrapper expects the pinned checkout at
`/home/t4ce/REPOS/reference/intel-media-driver` by default. If LibVA, libdrm,
or GmmLib are not visible through `pkg-config`, it bootstraps them from source
under `bld/intel-media-driver-oracle/`:

- `src/libdrm`, built with optional display/vendor extras disabled
- `src/libva`, built DRM-only with X11/Wayland/GLX disabled
- `src/gmmlib`, built with CMake and installed as `libigdgmm.so.12`

The media-driver build uses that local prefix through `PKG_CONFIG_PATH` and
`LD_LIBRARY_PATH`, and demotes GCC 15's `-Warray-bounds` STL initializer warning
with `-Wno-error=array-bounds`. The expected outputs are:

- Driver oracle:
  `bld/intel-media-driver-oracle/build/media_driver/iHD_drv_video.so`
- TRUEOS packet trace:
  `bld/intel-media-driver-oracle/trueos_avc_recipe_trace.txt`
- Build manifest:
  `bld/intel-media-driver-oracle/manifest.txt`

To force system/package dependencies instead of the local bootstrap, set
`TRUEOS_INTEL_MEDIA_BOOTSTRAP_DEPS=0`. If the checkout lives elsewhere, export
`INTEL_MEDIA_DRIVER_SRC`.

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

For now, keep the shim idea behind the compiled Linux oracle plus Rust port. If
the Rust dword encoders start diverging too far from upstream, then generate or
compile just the packet compiler layer.
