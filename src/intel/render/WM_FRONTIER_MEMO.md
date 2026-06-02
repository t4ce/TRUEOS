# XeLP Render WM Frontier Memo

Date: 2026-06-01

This memo captures the current Intel XeLP render-engine triangle/fragment
frontier. It exists to keep the next work anchored to evidence, not to the
noise of the rotating boot logs.

## Current Verdict

The render path still stops before WM coverage:

```text
first_good=raster-input-ready
first_bad=wm-coverage
wm_coverage=0
psd_dispatch=0
fragment_observed=0
```

The most recent summary in `bld/baremetal-logs/latest.log` says:

```text
primary-frontier-summary first_good=raster-input-ready first_bad=wm-coverage ... wm_coverage=0 psd_dispatch=0 fragment_observed=0 next=instrument-wm-coverage
primary-problem last_proven=raster-input-ready first_missing=wm-coverage problem=fixed-function-raster-does-not-report-wm-coverage suspect=sf-object-setup-to-wm-scan-conversion-state-packet-mismatch
```

The first missing behavior is not VS, HS, DS, GS, SOL, VF fetch, or basic
CLIP input. It is the transition from a raster-input-ready object into WM scan
conversion coverage.

## Strongest Current Probe

The strongest isolation probe is:

```text
late-vf-screen-pointlist-slot0-xyzw-open-bounds-raster-wm-oa-probe
```

It uses:

- VF-synthesized VUE.
- Position in VUE slot 0 as full XYZW.
- No fake header slot.
- POINTLIST topology.
- Pretransformed screen-space coordinates.
- Clip enabled with ACCEPT_ALL pass-through.
- SF viewport transform disabled.
- Point width 8 pixels.
- WM scissor disabled through raster state.
- Full drawing rectangle: `draw_rect=[0,0..65535,65535]`.
- Mesa-simple ordering for the backend packets.
- PS state armed and KSP validated.
- OA begin/end reports valid.

This probe is important because it removes triangle/rect edge setup and tight
scissor/draw-rectangle boundaries from the first-bad set.

## Evidence From The Pointlist Open-Bounds Probe

Useful positive evidence:

```text
fragment-candidate-shape accepted=1
probe-vf-vue-contract experiment=pos-slot0-xyzw ... header_slot=none position_slot=slot0=xyzw
probe-clip-decoded topo=pointlist ... ClipMode=ACCEPT_ALL ... ClipEnable=1 PerspectiveDivideDisable=1
probe-sf-decoded ... PointWidth=0x40 ... DerefBlockSize=0(Block32)
probe-raster-decoded ... raster_scissor_legacy_bit=0 ... sample_mask=0x1
probe-wm-decoded ... force_thread_dispatch=2(ForceON) ... wm_hz_op=0 wm_hz_scissor=0
probe-sf-wm-contract topo=pointlist vertex_count=3 primitive_objects_expected=3 ... draw_rect=[0,0..65535,65535]
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps ... delta_cl=3 delta_cl_prim=3 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=3
clip-raster-proof accepted=1 cl_delta=3 cl_prim_delta=3
raster-packet-proof accepted=1 post_clip=0xC0DE7726 post_raster=0xC0DE7727
```

Negative evidence:

```text
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
scratch-rt-fragment-proof accepted=0 ... changed=0
render-target completed=1 any_change=0 triangle_change=0
```

The engine is not just failing to write the render target. The OA window sees
no raster samples and no PS thread dispatch.

## Ruled Out For This Frontier

The following are no longer good first suspects for the current track:

- Missing VS execution: VF-synthesized VUE path intentionally has `vs=0`; that
  is expected and logged.
- HS, DS, GS thread counters: also expected to be zero on this path.
- Fake VUE header slot: pointlist slot0 XYZW path has no fake header.
- Position slot ambiguity: position is now explicitly slot0 XYZW.
- Triangle edge equations: pointlist avoids triangle/rect edge setup.
- Tight scissor or tight draw rectangle: open-bounds pointlist still fails with
  WM scissor disabled and a full drawing rectangle.
- Basic CLIP pass-through: CL counters report three point primitives.
- PS kernel pointer or PS state absence: KSP and PS state markers are present.
- RT visibility alone: no WM coverage or PSD dispatch precedes any RT write.

## Current First-Bad Name

Use this phrase consistently:

```text
SF object setup to WM scan-conversion state-packet mismatch
```

This means the software-visible packets are sufficient to get VF and CL input
and to retire the raster packet markers, but the hidden SF-to-WM object payload
does not appear to produce WM coverage.

## Current Code Anchors

The current focused probe is added in:

```text
src/intel/render/primary.rs
```

Backend mode and submit-name classification live in:

```text
src/intel/render/state.rs
```

Packet construction and decoded proof logging live in:

```text
src/intel/render/pipeline.rs
src/intel/render/submit.rs
```

The pointlist open-bounds mode is:

```text
BackendProbeMode::RasterWmInputOaScreenSpacePointListOpenBounds
```

The submit label is:

```text
late-vf-screen-pointlist-slot0-xyzw-open-bounds-raster-wm-oa-probe
```

## Host Oracle Track

There are two different host-side tracks. Keep them separate.

The sane track is a host Mesa render oracle: run a known-good Intel Vulkan
triangle under the i915 ioctl tracer, preserve its execbuffer BO dumps, and
compare Mesa's render packet/state setup against TRUEOS. This is the right next
tool for the WM frontier because the current suspect is an SF-to-WM state
contract, not arbitrary ring replay.

The command is:

```sh
bash tools/intel_userland_oracle/run_render_triangle_oracle.sh
```

The default output directory is:

```text
.codex_tmp/intel_userland_oracle/render-simple-triangle
```

The useful files are:

```text
simple_triangle_dump.log
pipeline_exec/host_state_reference.txt
pipeline_exec/*GEN_Assembly.txt
log.txt
replay_manifest.json
summary.txt
```

The host triangle must say `simple_triangle_dump: verified=1` before treating
the captured packets as a good oracle. If it does not verify, discard that
capture for WM comparison.

The insane track is the replay path:

```sh
python3 tools/intel_userland_oracle/extract_replay_manifest.py TRACE/log.txt --hash > TRACE/replay_manifest.json
python3 tools/intel_userland_oracle/emit_replay_artifact.py TRACE/replay_manifest.json --out src/intel/replay_*.rs --name NAME
```

That preserves concrete BO GPU VAs and batches for baremetal replay. It is
useful later, but it should not be the first WM experiment because replaying a
captured ring submission drags in arbitrary host driver state and does not
explain which visible packet contract TRUEOS is missing.

Existing historical evidence:

```text
crates/trueos-shader/host_shader_validation/simple_triangle_dump.log
.codex_tmp/intel_userland_oracle/adapterlibgfx-rotating-intel/log.txt
```

The first file is the older verified Intel simple-triangle host proof. The
second is a noisier render-ish execbuffer capture suitable for replay/autopsy,
not a clean first oracle for the current WM boundary.

## Next Useful Experiments

Do not start by changing VF geometry again. The better next probes should
change one SF-to-WM contract variable at a time while keeping the pointlist
slot0 XYZW baseline intact.

Good next candidates:

1. Compare pointlist with SF viewport transform enabled and clip-space/NDC
   coordinates, while keeping POINTLIST and point width 8.
2. Keep pointlist slot0 XYZW but switch CLIP mode to PASSTHRU if the packet
   encoding supports that distinct mode cleanly.
3. Emit a line-list or line-strip screen-space primitive with nonzero line
   width, because line setup shares SF/WM scan-conversion but has different
   object setup than points and triangles.
4. Instrument more SF/WM-related live registers around the pointlist submit,
   especially anything that can distinguish "SF produced object setup" from
   "WM refused scan conversion".
5. Capture or refresh the host render oracle above, then compare any packet
   that affects SF output contracts, not just visible raster/scissor state.

Bad next candidates:

- More changes to VUE slot/header layout without a new reason.
- More scissor/draw-rectangle probes; open-bounds did not move coverage.
- Treating `completed=1` or post-draw markers as a fence for WM coverage.
- PS payload spectrum before `wm_coverage=1` or `psd_dispatch=1`.

## Run Loop

The standard runtime loop for this frontier remains:

```sh
make iso
sleep 25
rg -n "late-vf-screen-pointlist-slot0-xyzw-open-bounds|raster-wm-input-proof|raster-wm-oa-raw|wm-boundary-regs|primary-frontier-summary|primary-problem" bld/baremetal-logs/latest.log
```

If `latest.log` is stale or empty, inspect all three maintained rotating logs:

```sh
rg -n "late-vf-screen-pointlist-slot0-xyzw-open-bounds|primary-frontier-summary|primary-problem" bld/baremetal-logs/trueos-baremetal.*.log
```
