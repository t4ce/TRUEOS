# Helio G-buffer native capture

This tool compiles Helio's unmodified `helio-pass-gbuffer/shaders/gbuffer.wgsl`
through Helio's vendored Naga and a dedicated Vulkan graphics-pipeline
descriptor. Unlike the SimpleCube and Churn compiler probes, the descriptor
mirrors the real `GBufferPass` ABI:

- one 40-byte vertex stream with six attributes;
- bind groups 0 and 1, including nine frame/scene bindings;
- 256 sampled-image and sampler descriptors with non-uniform indexing and
  update-after-bind semantics;
- eight color attachments in Helio's exact format order;
- `D32_SFLOAT`, depth writes, and `LESS_OR_EQUAL` comparison.

The executable only creates the pipeline. It never binds placeholder scene
resources or submits a draw, so native ISA capture remains independent of one
particular SceneDB snapshot.

Build and validate the checked-in result from anywhere:

```sh
python3 tools/helio-gbuffer-shader-bake/bake.py
python3 tools/helio-gbuffer-shader-bake/bake.py --validate-only
```

The default output is `picasso/helio-gbuffer`. It contains the exact
WGSL, per-entry SPIR-V, extracted Intel SIMD8 VS/FS programs, compiler log,
Mesa assembly, and hash-bound ABI metadata. Use `--device-id 0x....` when a
machine exposes more than one Intel Vulkan compiler device.
