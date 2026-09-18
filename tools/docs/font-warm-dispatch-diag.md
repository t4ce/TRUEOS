# Font warm-up / R8 coverage diagnostic

This is diagnostic-only, on top of the existing `log_os` GPU-first profile.
`FONT_WARM_DIAG_PROFILE_ENABLED` follows `TGL_GPU_DIAG_PROFILE_ENABLED`.
`log_font_warm_diag!` uses the existing rate limiter and the existing
`intel/gpgpu` -> Gpgpu/Info area policy. The TCP ring receives accepted bytes;
the unchanged LastAP MicroFont task reads the same bytes with its own cursor.
There is no separate screen filter, no new queue, no shader rebake, no bypass,
and no change to Spirit/cubes/Gridpaper/start-button autostart.

Each diagnostic site emits its first two occurrences and every 128th after
that, with the normal occurrence/suppressed counts. Critical fields are at the
front of short lines for the two-column screen's 320-character limit. Existing
warnings, errors and quarantine/ownership decisions are not removed or changed.
Render/Info remains filtered: this does not turn font-tessel polling back on.

## Follow these existing execution boundaries

- `phase=raw-face-enter`, `raw-face-return`: the raw-font registry/embedded-font
  availability call. `result=Ok(())` is CPU/raw font availability, NOT GPU warm-up.
- `coverage-request`, `coverage-input`: a prepared outline reaches R8 coverage;
  includes mask extent, pitch, GPU address and run count. Input/DMA early exits
  now identify their reason rather than only becoming `font-coverage-dispatch`.
- `coverage-admission reject=...`: distinguishes contract/offset/shape failure,
  font submit-lock contention, missing device, coverage-kernel upload failure,
  and unavailable font context. `font-context quarantined=1` is a consequence
  of an earlier uncertain submission; inspect that earlier event first.
- `coverage-ready`: kernel upload and font state exist. The exposed PCI identity
  is still the experimental ADL alias, not a claim about physical silicon.
- `coverage-prepare-failed`: `fw/control/ppgtt/kernel/ops/mask/batch` preserves the
  actual short-circuit ladder. The first zero is the failed stage; later zeros
  can simply mean the stage was not called. `coverage-prepared` means the
  complete batch was encoded, not that it ran.
- `font-rcs-enter`, `font-rcs-attempt`: font-lane runtime/pending state and the
  actual Submitted/Deferred/Rejected/Ambiguous result before Deferred and
  Rejected are both collapsed into the public rejection state.
- `coverage-submit`: public submission state, polling eligibility and timeout.
- `font-retire-complete` / `font-retire-incomplete`: the **existing raw completion
  observation and saved-LRC-head proof**, before the poll helper normalizes a
  failed retirement to zero. No extra GPU-register reads are added here.
- `coverage-marker`, `coverage-return`: result propagated back to the font
  builder. `retired_marker` is the normalized poll result, NOT necessarily the
  raw GPU-written word. The extra `pre` read is a CPU read of the existing result
  page only after batch preparation; no marker/command writes were added.

## Interpret the boundary, not a generic font failure

`font-coverage-dispatch` maps to `GpgpuDispatchRetirement::NotSubmitted`; it does
not by itself say which admission stage failed or prove a shader ran.
`SubmittedIncomplete` instead retains uncertain resources and quarantines the
font context, preserving the existing no-reuse contract.

For an encoded coverage batch, the pre marker follows the dispatch prologue
and precedes the outline walkers. The post marker is in the cache-draining
epilogue. A missing pre marker does not isolate the shader as the cause.

Retirement requires **both** the expected marker and saved head == published
tail. Thus `marker=1` with unequal `saved_head`/`tail` is a context-retirement
proof failure even when the GPU completion word was observed. It must not be
reported as "shader never ran" based on the public poll function returning 0.

The first failure is the useful one. Repeated R8-backing retirement refusals,
font-context quarantine rejections and start-button retry messages describe
what happens after it, not independent evidence of fresh GPU dispatches.

## Validation

`python3 tools/test_tgl_gpu_log_profile.py` validates the existing acceptance
matrix and real sink adapters. `python3 tools/test_font_warm_diag.py` compiles
the actual new macro and actual coverage-admission function with hardware stubs,
checks lazy sampled arguments, the Gpgpu route, half-column-sized records,
short-circuit preparation and unchanged dispatch ownership outcomes. Rustfmt
parsing covers all changed Rust files. These are not a full kernel build or a
hardware execution test. No build/deploy/reset is performed by these tests.
