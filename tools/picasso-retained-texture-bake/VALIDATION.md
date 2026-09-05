# Picasso retained materials — validated baseline, 2026-09-05

The user confirmed that the DamagedHelmet now renders with continuous textured
surfaces after the render-batch LRI address correction. Screenshot:
`/home/t4ce/Pictures/Screenshots/Screenshot From 2026-09-05 21-55-28.png`.
This records the recovered geometry/authored-UV milestone, not full glTF or
physically based lighting conformance.

## The correction to preserve

The old batch register helper used the context-image LRI opcode with an absolute
MMIO address. `AddCSMMIOStartOffset` added the RCS base again: the intended
L3ALLOCREG write to `0xB134` targeted `0xD134`. The fixed batch helper emits
`[0x11001001, 0xB134, 0xB0000040]`, using an absolute address with the offset bit
clear. Saved-context register lists retain their separate encoding.

The rig's four enabled L3 banks provide 256 KiB at the reset 16-way allocation,
or 512 KiB at the requested 32-way allocation. PBR needs 32 KiB of reservation
plus 3576 × 128-byte VS entries: 490,496 bytes. The earlier 64-byte shader fits
the reset allocation; PBR does not. The UV diagnostic also deliberately retained
128-byte entries, explaining why that shader swap did not remove the fault.

Intel TGL PRM Vol2a pp1003–1004 specifies the different batch/context address
rules; Vol2c pp84–85 and pp1265–1266 defines the bank fuse and allocation fields.
Local primary references live under `/home/t4ce/Repos/bak/reference/G12TL_intel_prm`.

Before the correction, a complete VUE capture contained 96,266 records with
clip position, world position and UV all zero; 89,158 records matched all three
references. The user then confirmed visual recovery after deploying the fix.
No VUE capture or runtime L3 readback is present in the successful run, so a
post-fix zero-mismatch count is not claimed.

## Evidence identity

- Baseline before the UV work: `059a2fb692d5c44a0064ea9d7cb46c62c0d3b83c`.
- Successful source HEAD: `6104b2d0b` (the register fix is already committed).
- Deployed kernel SHA-256: `cc854128a3612450e32c668218c72421883bb82e6aeed465685bc96b7e62cf91`.
- ISO SHA-256: `b01f4fe04b787ebfcb7e3c22ef6ea36c74a55bee1d2741a315a8453c8ffcd73f`.
- Screenshot SHA-256: `f4be5ffefd3b42747e412086e577b9a28927c29cfb2fdcfd6a0608e0e805397c`.
- Preserved local evidence and database backup:
  [success-2026-09-05](../../bld/picasso-pbr-validation/success-2026-09-05/).

The successful run reports `pipeline=pbr`, `material_mask=0x1F`, the 48-byte
vertex layout, four varyings and retired native matrix draws. The screenshot
and user confirmation supply the visual evidence missing from those log claims.

## Texture coverage and remaining scope

All five source images are processed; there is no unused helmet map to connect.

| Map | Shader input | Current evidence |
| --- | --- | --- |
| Base color | sRGB RGBA × factor | Recognizable authored UV texture on recovered surfaces |
| Metallic/roughness | Linear B/G × factors | Decoded, uploaded, bound and used by the combined PBR shader |
| Normal | Linear RGB, tangent basis and normal scale | Same combined-path evidence; independent effect validation pending |
| Occlusion | Linear R × strength | Same; applied to indirect lighting |
| Emissive | sRGB RGB × factor | Same combined-path evidence |

The source material uses default metallic/roughness factors of one. The shader
already contains a GGX/Smith/Schlick direct-light model. Indirect reflection uses
an analytic sky/softbox; there is no environment-map IBL or prefiltered BRDF
lookup. Metallic appearance is therefore a lighting/conformance follow-up,
not evidence that the metallic texture is absent. Independent metallic and
roughness diagnostic views would be a small next step; environment-map IBL
requires a larger resource/shader contract.

Opaque materials, TEXCOORD_0, repeat sampling and the base mip are the current
admitted path. Alpha blend/mask, mip chains, skinning, animation and broader
glTF scene conformance are not established by this result.

## Multi-instance follow-up

The next user experiment exposed a separate lifecycle failure: starting a
second PBR Example closed the first. The preserved
[dual-run log](../../bld/picasso-pbr-validation/dual-run-2026-09-05/dual-run.log)
records `vm0: PicassoExample: retained animation failed: Ui4("frame-begin", Busy)`
at line 1400, while vm1 decodes its first material image. VM0 then explicitly
requests shutdown after a fatal scene error; both carrier claims succeeded.
This identifies the termination path. It does not establish why display buffer
ownership remained busy at that instant, or prove a GPU allocation shortage.

UI4's streaming `begin_gpu_frame` is nonblocking. `Busy` is returned before a
write lease is acquired or any frame state is changed, so the Example can defer
that frame and retain the existing front buffer. The Example now distinguishes
deferred frames from rendered frames, retries at its existing 16 ms cadence,
and waits for a rendered startup frame before claiming submission success.
Errors after acquisition remain fatal. Per-instance deferral/resume messages
make recovery visible in the next run.

Validation: all eight Example host tests passed, including three admission
regressions. The normal Blueprint builder produced the release package with
`TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1` and passed its CABI import guard; no
publication or rig operation was performed. Package SHA-256:
`3b2527bf587c251a7fdcbbcf35d321a4c80da9d6f5958c83b9fe8bf05541beff`.
Build log: [picasso-dual-run-package-build.log](../../bld/picasso-dual-run-package-build.log).

The user subsequently confirmed three simultaneous PBR Example instances after
the frame-admission fix. This establishes the requested coexistence milestone
for that run. The user reports remaining trouble under some heavier video and
window combinations and explicitly deferred that separate investigation.
Seed tracking has not been broadened to claim arbitrary mixed workloads.

## Consolidation on the current branch

Batch absolute addressing is named separately from saved-context addressing;
the already-correct relative indirect draw packets keep identical bytes.
Regression checks cover all three paths and tie the actual PBR artifact's VUE
size to the intended URB partition. A compile-time budget assertion protects
the current 128-byte allocation. Python cache outputs from this UV work are
removed from version control and ignored going forward.

The `picasso-seed.redb` mirror updates eight existing tracking rows only. UV
accessor/view and base-color view are fully respected/tested; the containing
buffer and four remaining image views are included/tested, with
`fully_respected=false`. Other asset statuses and immutable tables are retained.
There are no separate normalized material/texture records in this seed schema;
the update does not invent them. The [tracking plan](../../bld/picasso-pbr-validation/success-2026-09-05/seed-tracking-update.json)
and [verified receipt](../../bld/picasso-pbr-validation/success-2026-09-05/seed-update-receipt.json)
record the exact keys and reasons.


Validation of this consolidation: compile-only kernel build passed (7.17s,
171 existing warnings); host checks passed for register/indirect/URB contracts
(6), context restore (7), VUE capture (6), and prepared asset/map round trips
(5): 24 tests total. Build log:
[`picasso-proven-cleanup-build.log`](../../bld/picasso-proven-cleanup-build.log).
No rig deployment or reboot was performed by the agent.
