# Clip position + UV shader bake

This compile-only lane targets ADL-S UHD 770, PCI `8086:4680`, through
Mesa ANV's no-op DRM shim. It does not submit rendering work. The tiny GLSL
vertex stage preserves clip-space xyz exactly and exports authored UV; the
linked fragment stage must compile byte-identically to Picasso's existing
SIMD16 sampled fragment executable, which is reused at runtime.

The baker requires `glslc`, `iga64`, a C compiler, and a Mesa build containing
ANV plus `libintel_noop_drm_shim.so`. Apply `mesa-vs-capture.patch` to that
Mesa source and rebuild ANV to expose compiler-selected VS state. The default
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
barycentrics beginning at g2, samples texture BTI2 through sampler 0, and
writes RT0. It has no push constants or scratch. Its full shader body is
independently decoded with IGA's `12p1` platform, as is the new 224-byte VS.

This proves compilation, extraction, ISA validity, payload layout, and the
existing PS's exact binary equivalence. The metadata deliberately records
that neither host image rendering nor bare-metal rendering was verified by
this bake.
