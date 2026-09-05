# Clip position + UV shader bake

This compile-only lane targets ADL-S UHD 770, PCI `8086:4680`, through
Mesa ANV's no-op DRM shim. It does not submit rendering work. The tiny GLSL
vertex stage preserves clip-space xyz exactly and exports authored UV; the
linked fragment stage must compile byte-identically to Picasso's existing
SIMD16 sampled fragment executable, which is reused at runtime.

The baker requires `glslc`, `iga64`, a C compiler, and a Mesa build containing
ANV plus `libintel_noop_drm_shim.so`. Apply `mesa-vs-capture.patch` and
`mesa-ps-capture.patch` to that Mesa source and rebuild ANV to expose
compiler-selected stage state. The default
build location is the existing instrumented Mesa under `.codex_tmp`:

```sh
python3 tools/clip-position3-uv-bake/bake.py
```

Use `--mesa-build /path/to/mesa-build` for another build. The checked-in
generated Rust module and files under
`crates/trueos-shader/clip_position3_uv_texture` are produced together.

The captured contract is five floats per 20-byte vertex: xyz at byte 0 and
UV at byte 12. VF component packing is `0x37`, with no SGVS inputs, yielding
g2=x, g3=y, g4=z, g5=u, g6=v. The VS writes its header at VUE slot 0, xyz1 at
slot 1, and UV at slot 2. Three 16-byte VUE slots occupy **one 64-byte URB
allocation**, so TRUEOS's `urb_entry_output_length` metadata is 1. This must
not be confused with the second URB SEND's offset 2, the SBE read offset 1
in 32-byte units, or the independent `3DSTATE_VS` output-length field, which
the captured gfx12 packet leaves zero. VS input read length is 1 at GRF 2.

The reused PS has one perspective varying, consumes perspective pixel
barycentrics in g2..g5, and requires its constant/setup coefficients at g6.
The captured SIMD8 setup start is g4; the SIMD16 setup start is g6. This
field selects where coefficients are delivered, not where barycentrics begin.
Programming g2 for SIMD16 overwrites barycentric data and leaves the g6 UV
coefficients undefined, producing invalid texture coordinates. The shader
samples texture BTI2 through sampler 0 and writes RT0. It has no push
constants or scratch. Its full shader body is
independently decoded with IGA's `12p1` platform, as is the new 224-byte VS.

This proves compilation, extraction, ISA validity, payload layout, and the
existing PS's exact binary equivalence. The metadata deliberately records
that neither host image rendering nor bare-metal rendering was verified by
this bake.

## Hardware verification, 2026-09-05

The separate end-to-end run on ADL-S UHD 770 (`8086:4680`, revision `0C`)
successfully rendered QuadTexture's complete Intel Graphics logo using its
authored UVs and indexed two-triangle path. The user confirmed the live camera
image. The previous flat purple output was resolved by changing the sampled
SIMD16 PS setup start from g2 to the compiler-selected g6; the shader machine
code stayed unchanged. All ten host regression tests passed before deployment.

The verified runtime ELF SHA-256 was
`86ed951bcba465a62d6d70188190a571f9f7807c9a0d7f8f7d15dbcdbbfcbb05`
and the ISO SHA-256 was
`c9434412efdd641ba897c46fe4774ef86f70f8ec4980acadffcfebdbb83726fa`.
The preserved log records `ps_setup_grf=6` and 51 sampled renderer returns,
all complete with matching release tokens and no textured-submit error.
This run did not reproduce the earlier `-32` failure.

Local evidence is preserved outside the rotating captures in
[`bld/quadtexture-uv-validation`](../../bld/quadtexture-uv-validation/):
`setup-grf6-logo-camera.jpg`, `setup-grf6-logo-film.jpg`,
`setup-grf6-success.log`, `setup-grf6-boot-receipt.json`, and
`setup-grf6-verification.json`. The last file records source commit and
artifact hashes. The generated bake metadata retains its compile-only scope.

## Native textured quad integration, 2026-09-05

This initial comparison build used this shader and Intel logo for its default/key **1**
native quad (four indices) and key **3** triangle comparison (six indices).
Key **1** also switches back. The kernel already encoded native QUADLIST;
the missing link was carrying topology through the sampled indexed API and
position/UV mesh upload. The quad perimeter is preserved through submission.
No shader bytes or fixed-function shader state changed.

`IndexedDraw.topology` occupies the former reserved word at byte 64. The wire
record stays 104 bytes; zero preserves legacy triangle-list submissions,
explicit triangle-list `4` and quad-list `7` are admitted, and other values
are rejected. Old kernels reject explicit topology, so deploy both the kernel
and the updated QuadTexture package for this experiment.

Host validation passed: eight topology/ABI/mesh checks, eleven existing UV
shader checks, and four QuadTexture mode/geometry checks. The compile-only
kernel build and release Blueprint package build passed, including the CABI
import guard. Publishing was disabled. Package SHA-256:
`a0e42cb8bf6f435ade1507923a20f67dbb4b619096534cc52446660372a771d8`.
Build logs: [kernel](../../bld/quadtexture-native-quad-kernel-build.log) and
[QuadTexture](../../bld/quadtexture-native-quad-package-build.log).

Hardware validation is pending: compare the complete logo and its orientation
at default → **3** → **1**. The prepared render log now includes
`topology=QuadList` with `indices=4`, or `topology=TriangleList` with `indices=6`.
Retirement alone does not establish correct image output.

## Triangle gallery follow-up, 2026-09-06

QuadTexture now starts in key **3**'s retained PBR triangle gallery, containing
all 24 tilepack GLBs and their 604 authored triangles. Key **1** retains the
native quad logo probe described above. The two-triangle logo comparison is
superseded by the gallery; its build receipt above remains historical.
See the sibling QuadTexture README for the database preparation, texture
atlases, camera controls, and pending hardware checks. This follow-up does
not change the clip-position/UV shader or expand the native quad capability.
