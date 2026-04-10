# xe_lp_shader_bake

This is a host-side packaging stub for the first TRUEOS Xe-LP triangle
pipeline.

It does not compile Intel shaders by itself. Its job is narrower:

- read offline-baked VS and PS binary blobs
- read stage metadata from JSON
- emit the exact Rust source file shape required by
  [src/intel/shader/generated.rs](/home/t4ce/REPOS/TRUEOS/src/intel/shader/generated.rs)

The runtime contract for the output file is documented in
[src/intel/shader/bake_format.md](/home/t4ce/REPOS/TRUEOS/src/intel/shader/bake_format.md).

## Expected Inputs

- `triangle_vs.bin`: little-endian stage code blob, 4-byte aligned
- `triangle_ps.bin`: little-endian stage code blob, 4-byte aligned
- `triangle_vs.json`: vertex shader metadata
- `triangle_ps.json`: fragment shader metadata

Starter metadata templates are provided next to this README.

## Example

```bash
python3 tools/xe_lp_shader_bake/emit_generated_rs.py \
  --vs-bin path/to/triangle_vs.bin \
  --ps-bin path/to/triangle_ps.bin \
  --vs-meta tools/xe_lp_shader_bake/triangle_vs.template.json \
  --ps-meta tools/xe_lp_shader_bake/triangle_ps.template.json \
  --out src/intel/shader/generated.rs
```

After capturing a dump directory, summarize candidate blobs with:

```bash
python3 tools/xe_lp_shader_bake/summarize_dump.py /tmp/intel-shaders
```

To print Rust-ready words for selected blobs:

```bash
python3 tools/xe_lp_shader_bake/summarize_dump.py /tmp/intel-shaders \
  --emit-rust triangle_vs.bin triangle_ps.bin
```

## What It Verifies

- binary sizes are multiples of 4 bytes
- required metadata keys exist
- dispatch mode is one of `Simd8`, `Simd16`, `Simd32`
- `code_size_bytes` is emitted from the actual blob size, not trusted from JSON

## What It Does Not Do

- compile GLSL, NIR, or EU assembly
- discover metadata automatically from Mesa
- validate shader semantics against hardware

That means the intended workflow is:

1. compile the tiny shaders offline with Mesa or another Intel-capable path
2. extract the metadata values needed by TRUEOS
3. run this tool to package them into `generated.rs`
