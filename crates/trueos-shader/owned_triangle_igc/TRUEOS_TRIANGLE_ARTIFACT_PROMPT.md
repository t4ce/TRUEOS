# TRUEOS-Owned 3-Vertex Triangle Artifact Prompt

Build a source-owned minimal triangle artifact for TRUEOS Intel render bringup.

Hard constraints:

- Do not use Mesa shader-cache extraction as the binary source.
- Do not infer success from KSP byte upload alone.
- Keep the draw contract to exactly three vertices.
- Vertex payload is one `float3` position per vertex, 12-byte stride.
- Vertex positions are NDC:
  - `(-0.55, -0.45, 0.0)`
  - `( 0.55, -0.45, 0.0)`
  - `( 0.00,  0.55, 0.0)`
- Fragment color is constant orange-red `(1.0, 0.2, 0.1, 1.0)`.
- Runtime integration stays gated until the artifact is a graphics-stage VS/PS
  contract, not only a compute device binary.

Compiler route:

- Prefer Intel IGC through `ocloc` when available.
- Record exact compiler binary, version, device target, source hash, and output
  hash.
- If using `ocloc`, mark the artifact as `igc-compute-proof`, because `ocloc`
  emits OpenCL/compute device binaries and does not by itself produce a TRUEOS
  3DSTATE_VS/3DSTATE_PS graphics-stage ABI.

Acceptance:

- Artifact must be reproducible from checked-in source.
- Metadata must say whether it is safe for TRUEOS graphics upload.
- Kernel-side upload must remain disabled unless `graphics_stage_upload_safe=1`.
