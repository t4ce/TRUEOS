# Font tessellation probe lane

This directory owns the offline, font-specific P0/P1 contract. It does not
enable production font rendering. `LegacyOnly` remains the runtime default and
the checked-in generated artifact is deliberately unavailable until a matched
native capture is imported.

Generate and validate the deterministic hand-authored source fixture:

```sh
python3 tools/font-tessellation/bake_font_patch.py \
  --out /tmp/trueos-font-patch --source-only
python3 -m unittest tools/font-tessellation/test_bake_font_patch.py
```

An instrumented Mesa capture lane may then import VS/HS/DS/PS binaries and
complete packet state. The importer rejects scratch, push data, relocations,
the wrong quad-domain TE state, missing URB state, or a device mismatch:

```sh
python3 tools/font-tessellation/bake_font_patch.py \
  --out /tmp/trueos-font-patch \
  --capture-dir /path/to/native-capture \
  --generated-rs crates/trueos-shader/generated_font_patch.rs
```

Capture/import is only native compiler evidence. Do not set the milestone or
result schema to `pass` until the private Font RCS probe demonstrates adaptive
factors, opposite-winding stencil cancellation, cover, and exact retirement on
the named device.
