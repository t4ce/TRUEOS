# Font hardware tessellation — integration and patch plan

**Status:** Proposed engineering plan; no renderer implementation or device validation is claimed.

**Decision:** Option B: real hull shader / tessellator / domain shader (HS/TE/DS) font generation.

**Suggested repository location:** `tools/docs/FONT_HARDWARE_TESSELLATION_PLAN.md`

**Prepared:** 2026-09-10

**Implementation checkpoint (2026-09-17):** P0 now has a versioned candidate
contract, deterministic hand-authored wedge fixture, baseline record, and result
schema under `tools/font-tessellation/`. The first P1 patch adds a font-owned
quad-domain shader source/import lane, `FontCurvePatchV1` ABI/preflight, and a
source-only generated artifact that fails closed. Native compiler capture,
Font RCS stencil/cover submission, exact retirement, and bare-metal evidence
remain required before M1 can be marked complete or the backend can be enabled.

## 1. Engineering goal and boundary

Implement a font-owned hardware-tessellation backend that converts Skrifa glyph outlines into winding-correct, antialiased R8 coverage masks. Feed those masks into the existing retained font representation, OceanCache, and RGBA8 row stamping. Keep the producer-facing registration, row geometry, ownership, publication, and ACK contract unchanged.

**The first production integration point is a glyph coverage-cache miss, not presentation of an already-rasterized row.** Success means geometry is actually expanded by HS/TE/DS and the resulting mask is consumed by a real font producer. A prepared-segment compute backend, a wireframe outline, or a tessellated bitmap quad does not complete this goal.

Compute is permitted for GPU-only coverage reduction and format conversion. CPU work may decode, validate, normalize, and exactly split Bézier control data; it must not expand the glyph into a filled triangle mesh for this backend.

Out of scope: a public arbitrary-shader API, Picasso asset/schema changes, a new font shaper, SDF/MSDF, glyph-specific compiled shaders, 3D/extruded text, new concurrent Font RCS slots, and replacing every legacy font path in the first release.

## 2. Inspected baseline: why these patches belong here

Repository snapshots: **TRUEOS `6a89c94b442341f2ee4618fdc0df33dd44156c1b`**, **Cubes `6bd23b4b27def0a1bbba08cd4fcf144d3e75cd80`**, and **TRUEOS-Picasso `2d2a33b99426b2cbeb72d4b35c10d4762ea02fae`**. Rebase and recheck these anchors before implementation.

| Current anchor | Consequence for this work |
|---|---|
| `font_outline_coverage_r8.clcpp` reconstructs subdivided curves inside each pixel's outline walk, then applies a signed-distance coverage ramp.[^coverage] | Replace coverage construction, not the warm stamping loop. Improved geometric subdivision alone is not equivalent to improved pixel coverage. |
| `GpuFontGlyphRecipe` already contains glyph-local, scaled, Y-oriented, sheared outline operations. Its bounds and audit/support metadata describe the current coverage path.[^recipe] | Reuse its outline authority; do not transform twice. Recompute backend-appropriate bounds instead of retaining assumptions derived from fixed subdivision. |
| `font_plan_service.rs` admits work using `estimated_segment_evaluations()`; `gpu_font.rs` applies legacy analytical-work limits.[^planner][^font] | Introduce a tessellation-specific bounded cost estimate. Do not globally raise or remove the analytical guard. |
| OceanCache retains colorless masks under face/native-size/raster-policy identity; the producer service owns exact row tokens and ACKs.[^ocean][^producer] | Add internal coverage-policy identity and preserve row ownership. Cache-hit rows should not execute HS/TE/DS again. |
| `render/resources.rs` and `render/pipeline.rs` specialize tessellation around `patch_cube`; the pipeline binds a null stencil surface.[^resources][^pipeline] | Cubes is reusable driver experience, not an existing general font or stencil backend. Real stencil allocation/state is a prerequisite. |
| Font RCS has one job slot and its own GPU address-space contract.[^rcs] | Execute the new stages under the existing serialized font ownership, not by borrowing Picasso's live draw state. |

**Documentation discrepancy:** the inspected Cubes README describes contract 7, while the inspected TRUEOS shader module asserts cube contract 8. Use the matched generated bundle and runtime checks as authority; reconcile the documentation during baseline capture. Do not encode either cube contract into the new font ABI.[^cubes][^shader]

## 3. Chosen rendering design

```text
registered raw font → Skrifa → glyph-local recipe
    → validated, bounded Bézier wedge records
    → VS → HS(adaptive factors) → TE → DS(evaluated wedge geometry)
    → non-zero-winding stencil → cover pass
    → GPU coverage reduction → owned linear R8 mask
    → retained scene / OceanCache → existing RGBA8 row stamp
    → exact final release → publication → exact display-release ACK
```

### 3.1 Wedge patches, not contour lines alone

Use **oriented Bézier wedges** as the initial fill representation: four cubic control points plus one shared anchor for each closed contour. Lines and quadratics are degree-elevated without intentionally changing their geometry. Preserve explicit closes, contour direction, holes, and the endpoints shared by adjoining patches. Empty outlines produce valid transparent results without inventing geometry. Hardware-tessellated five-point wedges are an established stencil-fill construction; Skia is an architectural reference, not a new runtime dependency.[^wedges]

Proposed internal ABI: `FontCurvePatchV1`, a **64-byte, 16-byte-aligned** record containing `P0..P3`, anchor, contour index, flags, and reserved fields. Assert all offsets and reserved values. Use a font-specific `PATCHLIST_1` layout: VS forwards the record; HS emits the five control points. This is not the cube's seed layout or 44-zero-index contract. Native compilation must verify the interface, patch constants, and URB requirements.

Start with a quadrilateral domain parameterization:

```text
B(u)   = cubic Bézier(P0, P1, P2, P3, u)
P(u,v) = (1-v) * anchor + v * B(u)
```

The collapsed anchor edge forms a fan. Capture and test the quad-domain factor mapping and triangle orientation; do not copy Cubes' triangle-domain TE packet. All wedges of one glyph contribute to its winding stencil before covering. Disable backface culling: opposite-facing contributions are needed for cancellation.

### 3.2 Adaptive factors and bounded geometry

HS selects subdivision from a device-space Wang-style error bound, not a constant eight segments. DS evaluates the curve at tessellator-generated coordinates.[^wang][^stages] Start experiments at **0.125 native-mask pixel geometric tolerance**; this is a proposed parameter, not a quality result. A supersampled target must scale both coordinates and the tolerance consistently.

Before submission, conservatively bound factors and generated work. Exactly split curves that exceed the validated hardware factor limit, preserving shared endpoints and contour anchors. Reject or route over-cap input before GPU ownership; never silently clamp away the error guarantee. Bound patch count, output extent, descriptor/code space, stencil work, and scratch bytes with checked arithmetic. Do not equate a small geometric error with a guarantee of tiny-feature topology.

### 3.3 Winding, antialiasing, and R8 production

Choose **stencil-and-cover with bounded spatial supersampling** for the first complete backend. This avoids making a new MSAA implementation a prerequisite, but still requires validated stencil resources and render-to-compute visibility.

Clear a private stencil and color target. Accumulate front/back winding with opposite wrapping operations, color writes off, and no depth rejection. Cover samples where stencil is nonzero, then reduce the binary coverage target into the existing linear R8 allocation entirely on the GPU. Do not add wedge colors together: overlapping wedges are winding contributions, not alpha layers.

For a finite stencil width, prove an admission bound preventing a nonzero winding from aliasing to zero. Saturation is not a substitute for signed cancellation. Unsafe glyphs must fail preflight or use an explicitly eligible legacy route. Test stencil overflow and reversed contours deliberately.

Start with **4×4 spatial samples per native pixel**. This is approximate coverage, not exact area integration. Quality tests may require a denser pattern or a later AA improvement. Render through a validated RGBA8 intermediate initially; direct R8 render-target support is an optimization, not an assumption. Format, tiling, pitch, clear, and GPU reduction behavior must be established for each enabled device profile.

Reuse Skrifa's existing hinting decision. **Do not silently treat the legacy distance-ramp optical bias as equivalent to area coverage.** The first forced-test policy can use zero optical bias under a distinct policy ID. Small-text production enablement requires explicit stem-weight/coverage approval or a defined optical treatment. Keep affected production registrations on legacy until that gate passes.

## 4. Integration contracts and ownership

### Internal seam

Introduce a backend-neutral coverage-build boundary beside the current retained-scene construction in `src/intel/gpu_font.rs`. Illustrative contract names below are new, not existing callable APIs:

```text
FontCoverageBuild { recipe, raster_policy, validated_work_budget }
FontCoverageResult { owned_mask, bounds, mask_ready_proof, actual_policy, metrics }
FontCoverageError = NotSubmitted(reason) | SubmittedIncomplete(reason)
```

Keep shader-facing patch definitions separate from service policy. An immutable raster-policy ID must distinguish backend, geometric tolerance, sample pattern, hinting/optical treatment, and version. Include any newly supported fractional raster phase in identity; do not expand the current phase contract implicitly.

Resolve policy before recipe/mask cache lookup. Update both recipe identity/fingerprinting and OceanCache sealing where their meaning changes. Never insert a legacy fallback mask under a tessellation-policy identity. Preserve existing cache bounds, sharing, and retirement rather than adding an unbounded glyph-geometry cache.

### Three mandatory integration fixes

**Bounds:** derive conservative Bézier bounds plus the selected coverage support. Update `mask_extent`, offsets, `audit_rect`, and `support_rect` consistently. A more accurate curve must not be clipped by an old flattened bound or incorrectly classified as non-overlapping.

**Admission:** make planner estimates backend-specific. Keep queue, character, extent, memory, and legacy work limits intact. Account for patch expansion, supersampled fill area/overdraw, reduction, and submission overhead; patch count alone is insufficient.

**Overlap:** retain `classify_gpu_font_prepared_placements` and the existing whole-scene/max-union behavior for overlapping placements in v1. Do not replace max-union with repeated source-over, or combine different glyphs into one shared winding calculation.[^recipe][^service]

### Font RCS lifecycle

Add the graphics batch under the existing Font RCS submit lock/runtime and `FontKernelGpuLease`. Reuse small, explicitly parameterized shader-upload and packet-emission helpers where appropriate; do not invoke the full Picasso/Cubes retained draw lifecycle or reuse its mutable buffers. Map every new resource into the font context explicitly. A font semaphore does not serialize unrelated renderer clients.[^service][^rcs]

Provision one bounded, font-owned transient GPU arena during backend initialization, reusable only after exact retirement. Keep existing bounded first-fill mask allocation separate from this arena. No new per-row GPU scratch allocation/mapping is allowed on the warmed path. M0 must freeze byte/count limits; never grow the arena underneath an active job.

The dependency chain is:

```text
upload/clear → stencil draws → cover → render-cache release
    → compute visibility/acquire → reduction → exact mask-ready proof
    → stamping → exact RGBA8 scanout release
```

Implement and test the required pipeline transitions, cache operations, and context-local addresses. An HS/DS completion or an ordinary render result is not the final mask/row release proof. Retain patches, shaders, descriptors, intermediates, and output through their actual last GPU use.

On ambiguous submission/retirement, propagate `SubmittedIncomplete` and quarantine the full reachable resource set and affected generation. Do not retry through legacy, publish the mask, evict referenced resources, or manufacture a producer credit. Only provably pre-submit rejection permits normal fallback. Ordinary subsequent work must restore its own HS/TE/DS, URB, stencil, blend, depth, and pipeline state.

## 5. Patch sequence and milestone gates

All milestones start **not started**. Each row is a bounded review unit; split mechanically large code changes without separating their safety tests. Paths marked **new** are proposed. No milestone requires implementing option A first.

| Patch / milestone | Scoped implementation | Required exit evidence |
|---|---|---|
| **P0 / M0 — Freeze contract and baseline** | Add this document and **new** `tools/font-tessellation/` fixtures/results schema. Record current cold fills, warm rows, policy/bounds behavior, device/compiler/bundle identity. Fix initial limits and acceptance thresholds before comparison. | Matched repository/compiler/device manifest; baseline corpus and timings; signed-off ABI, fill/AA policy, resource caps, and forced-test/default-off behavior. |
| **P1 / M1 — Prove the hardware dependencies** | Add **new** `tools/font-tessellation/bake_font_patch.py` and **new** `crates/trueos-shader/generated_font_patch.rs`; wire `src/intel/shader.rs`. Add minimal full graphics-state setup and font-owned batch/resource support in **new** `src/intel/gpgpu/rcs/font_tessellation.rs` plus `rcs/runtime.rs`; narrowly extract helpers from `render/resources.rs` / `render/pipeline.rs` only as needed. | On-device, offscreen hand-authored patches prove quad-domain HS/TE/DS, multiple adaptive factors, opposite winding cancellation, real stencil clear/cover, and safe return to ordinary drawing. Bake/capture success alone does not pass. |
| **P2 / M2 — Normalize real glyphs** | Add **new** `src/intel/gpu_font/tessellation.rs`; consume `GpuFontGlyphRecipe::outline_ops()`. Implement cubic normalization, contour anchors/closes, exact splitting, conservative bounds, ABI validation, and bounded estimates. Touch `crates/trueos-graphics/font.rs` only if a demonstrated outline-access gap remains. | Host property tests cover line/quad/cubic equivalence, close semantics, finite input, shared endpoints, bounds, factor limits, and winding-budget rejection. Real `B`, `8`, `@` reach the hardware probe without CPU fill triangles. |
| **P3 / M3 — Complete coverage output** | Implement GPU supersample reduction and exact R8 readiness. Proposed resolve source: **new** `crates/trueos-shader/gpgpu/kernels/font_tessellation_resolve_r8.clcpp`; wire through `gpgpu/operations/` and its actual generated contract/registry/payload plumbing. | Independent coverage comparison passes the frozen quality gates; holes, overlaps within glyphs, phase sweeps, and target edges are correct. No live-path CPU pixel copy/readback. Distinct mask-ready proof and complete intermediate retirement are demonstrated. |
| **P4 / M4 — Integrate producer cache fills** | Wire coverage selection in `gpu_font.rs` / `font_kernel_service.rs`; update `font_plan_service.rs` estimates and `oceancache.rs` policy identity. Preserve `font_producer_service.rs` state machine; add tests rather than redesigning it. | A real registered producer fills, caches, stamps, publishes, and ACKs a hardware-generated glyph. Repeated glyph/color/position changes reuse masks. Cache-full, overlap, cancellation, stale ACK, and incomplete-submission tests preserve existing ownership. |
| **P5 / M5 — Validate and selectively enable** | Add device-specific benchmark/soak evidence and conservative internal backend selection. Update the producer architecture document and this milestone record. Keep a legacy-only rollback setting. | Eligible workloads meet quality, cold-fill, warm-path, memory, and tail-latency gates; Cubes/Picasso/UI4 state-switch stress passes. Enable only measured device/policy/workload combinations. |

Keep the Cubes bundle/ABI unchanged. Its bake lane demonstrates how to capture and relocate native HS/DS state, but font binaries, quad-domain TE state, descriptor layouts, and hardware evidence must be produced independently.[^cubes] Picasso needs no source change for the first font backend; it is a coexistence/regression client.

## 6. Acceptance, fallback, and release evidence

Use the existing faces when available, including the optional JuliaMono path; report unavailable assets as skips, not passes. Cover native tiers 16/24/36/54/80, small text, large eligible glyphs, curves/counters, dense CJK, composites, whitespace, reflected/reversed synthetic contours, degenerate segments, self-overlap, extreme factors, and rejected bounds. Exercise fractional phases in the offscreen harness without silently changing producer placement semantics.

An independent high-precision outline/winding raster reference must consume the **same resolved/hinted outline**. Record coverage error, ink-area/stem-weight change, counter behavior, and phase stability. Freeze numerical thresholds with M0 fixtures; review representative small-text images. Agreement with the legacy distance ramp is not the only correctness oracle.

Measure CPU preparation, lane wait, patch upload, HS/DS/fill, reduction, total cold-fill p50/p95, cache-hit row latency, scratch peak, and fallback/rejection rate. Separate timing runs from intrusive diagnostics. Report the full requested workload, not just the subset fast enough to use the new backend.

**Proposed performance gate:** at least 20% lower median cold-fill time on a predeclared eligible corpus, no worse p95 within measured uncertainty, and no material warm-row regression (initial budget: 5%). These are engineering targets, not observed results. Missing performance targets keeps the backend experimental even if correctness passes. Do not relax existing retirement deadlines to hide stalls.

Initial modes are `LegacyOnly` (default) and `ForceHardwareProbe` (no silent fallback). Introduce automatic selection only after quality/policy compatibility is demonstrated. Unsupported devices, limits, or optical requirements select legacy before submission or produce an explicit safe rejection in forced mode. Fallback must use its actual policy identity or remain uncached. Mixed-policy rows are not approved implicitly.

Each milestone records: source commits, compiler and artifact hashes, device/stepping, commands executed, fixture IDs, host/device results, timing method, resource peaks, and unresolved failures. Debug-only fenced readback is allowed for verification; it must remain absent from production traffic. Host tests, native capture, and bare-metal rendering are separate evidence categories.

**Release definition:** a real producer consumes HS/TE/DS-generated, winding-correct R8 coverage; warm reuse remains unchanged; memory/ACK safety survives failures; measured benefits justify the selected default policy.

## 7. First implementation handoff

Start P0, then land the smallest P1 vertical slice: a private offscreen font-context draw of hand-authored curved wedges, with real stencil cancellation and explicit retirement. Keep ordinary font rendering legacy-only. Do not begin by enlarging the public producer API, adapting cube immediates per glyph, or replacing the current renderer wholesale.

## Source anchors

The repository links below are pinned to the inspected snapshots. External references explain algorithms/stage roles, not TRUEOS hardware readiness.

[^coverage]: [TRUEOS coverage kernel](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/crates/trueos-shader/gpgpu/kernels/font_outline_coverage_r8.clcpp).
[^recipe]: [TRUEOS recipe, retained stamping, placement classification](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/src/intel/gpu_font.rs#L600-L1200).
[^planner]: [TRUEOS font plan service](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/src/r/services/font_plan_service.rs).
[^font]: [TRUEOS font construction and legacy limits](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/src/intel/gpu_font.rs).
[^ocean]: [TRUEOS OceanCache](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/src/r/services/oceancache.rs).
[^producer]: [TRUEOS producer architecture and lifecycle](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/tools/docs/FONT_SEMIPERSISTENT_PRODUCERS.md).
[^service]: [TRUEOS FontKernel service and lane](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/src/r/services/font_kernel_service.rs).
[^rcs]: [TRUEOS Font RCS runtime](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/src/intel/gpgpu/rcs/runtime.rs).
[^resources]: [TRUEOS render resource upload](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/src/intel/render/resources.rs).
[^pipeline]: [TRUEOS render pipeline state](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/src/intel/render/pipeline.rs).
[^shader]: [TRUEOS shader contracts](https://github.com/t4ce/TRUEOS/blob/6a89c94b442341f2ee4618fdc0df33dd44156c1b/src/intel/shader.rs).
[^cubes]: [Cubes hardware tessellation and native bake notes](https://github.com/t4ce/Cubes/blob/6bd23b4b27def0a1bbba08cd4fcf144d3e75cd80/tools/README.md).
[^wedges]: [Skia historical hardware wedge tessellator contract](https://skia.googlesource.com/skia/+/2bec8ab55b5b4e51a0a8fcaabf7ef95c4167b216/src/gpu/tessellate/GrPathTessellator.h).
[^wang]: [Skia Wang's formula implementation and derivation](https://skia.googlesource.com/skia/+/refs/heads/main/src/gpu/tessellate/WangsFormula.h).
[^stages]: [Microsoft tessellation-stage roles](https://learn.microsoft.com/en-us/windows/win32/direct3d11/direct3d-11-advanced-stages-tessellation).
