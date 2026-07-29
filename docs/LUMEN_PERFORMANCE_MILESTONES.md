# Lumen end-to-end performance campaign

This document is the durable control plane for improving the fixed
LFM2.5-350M Q8 inference path. The campaign has three implementation
milestones. Each milestone ends with the same three bare-metal validations, so
the nine runs remain comparable from the first diagnostic build through the
final optimized build.

The exact physical campaign target is Intel Alder Lake-S GT1 / UHD Graphics
770 at `00:02.0`, PCI vendor/device `8086:4680`, revision `0x0c`. The
checked-in C++/IGC artifacts pin the same device and revision; the boot and
runtime artifact-admission records must confirm both before a run is accepted.

The performance work must preserve the fixed model, tokenizer, greedy decode
schedule, packed-model hash, and exact `hi` response. A faster result is not
accepted if projection parity, token parity, completion-marker integrity, or
UI responsiveness regresses.

## Milestone 1 — trustworthy measurement and low-risk waste removal

Make every captured TCP log sufficient to explain one complete turn:

- resident-open and cold pack/seal latency;
- prompt/reply token counts and end-to-end turn latency;
- callbacks, projections, submissions, failures, and total submit time;
- RCS encode, admission, completion, and GPU timestamp totals;
- projection-batch shape/signature totals;
- CPU quantization, activation packing, allocation/copy, and non-projection
  phase totals where practical.

Remove only directly proven redundant work while adding this instrumentation.
Milestone 1 is complete when Runs 1–3 pass and the resulting log can be reduced
to one comparison report without reading Matrix-only output.

### Captured Milestone 1 baseline

Runs 1–3 passed on source commit
`57370d5f6ee23620f5a25a07f84037ee519ec7fa`. The report selects only fresh
`turn=1 context_before=0` sessions; later conversational turns remain available
as context-stress evidence but are excluded from the campaign count.

| Run | Prompt | Asset state | Turn ms | Prefill ms | Reply ms | Reply tok/s | GPU us | Completion - GPU us | CPU attention us |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | `hi` | cold open, 16,521 ms | 9,094 | 4,501 | 4,593 | 1.96 | 5,314,157 | 3,138,843 | 315,000 |
| 2 | `hi` | cold open, 16,614 ms | 9,113 | 4,543 | 4,570 | 1.97 | 5,365,177 | 3,094,823 | 309,000 |
| 3 | sky | boot-resident assets | 21,367 | 9,635 | 11,732 | 1.88 | 12,159,127 | 7,227,873 | 1,230,000 |

The two canonical `hi` turns differ by only 19 ms, or 0.21% end to end.
Across all three selected runs the completion interval outside the timestamped
matrix walkers costs 2.52–2.60 ms per submission and about one third of turn
wall time. Per-signature GPU throughput is likewise stable at only about
0.80–1.53 logical GB/s, despite the same packed artifact reaching roughly
46–49 GB/s through the Linux/NEO hardware oracle.

The immutable evidence copies are:

- `bld/baremetal-logs/lumen-m1-validation1.log`, SHA-256
  `4f3ee1117ac720c8efbd5e1b62b295ac1c1601777e2c81f7f1da30e72f8a03b4`;
- `bld/baremetal-logs/lumen-m1-validation2.log`, SHA-256
  `112c6596520530dfeb39bec774690beb4d6266ed42c14d6388df3e6b0cff5867`;
- `bld/baremetal-logs/lumen-m1-validation3.log`, SHA-256
  `6047cd68abcbf07c6b99951be79941de06d1fff47117d5d1e97689cc04e64936`.

The first and third archives are non-overlapping campaign inputs: the third is
a rolling capture that already contains Run 2. Recheck the baseline with:

```sh
python3 -B tools/lfm25_baremetal_report.py \
  --expect-runs 3 \
  bld/baremetal-logs/lumen-m1-validation1.log \
  bld/baremetal-logs/lumen-m1-validation3.log
```

## Milestone 2 — host and submission hot path

Use Runs 1–3 to remove the largest measured costs around the kernel:

- collapse the attention scratch pattern that currently creates about 23,940
  temporary allocations during the canonical `hi` turn;
- reuse projection specifications, output storage, and remaining attention
  scratch;
- avoid redundant hidden-state and sidecar copies;
- avoid repeated quantize/repack work where ownership permits;
- reduce completion polling and cache-maintenance overhead without weakening
  timeout or quarantine behavior;
- preserve exact ordering and one-conversation state isolation.

Milestone 2 is complete when Runs 4–6 pass, exact outputs remain unchanged, and
the report demonstrates an end-to-end gain over the corresponding Milestone 1
runs.

The Milestone 2 diagnostic build keeps
`LUMEN_PERF_DIAG_PROFILE_ENABLED=true` in `src/log_os.rs`. That single profile
switch admits the focused Service/GPGPU records and controls the RCS phase
sampler itself; setting it to `false` compile-folds submissions back to the
legacy unsampled command path. While enabled, the sampler instruments only the
first and power-of-two successful submission for each fixed matrix signature
and emits one schema-1 `turn-rcs-probe` record after every completed turn.

Require that record when checking Runs 4–6:

```sh
python3 -B tools/lfm25_baremetal_report.py \
  --expect-runs 3 \
  --require-rcs-probe \
  bld/baremetal-logs/LatestOfThree.logs
```

### Captured Milestone 2 results

Runs 4–6 passed with exact token and response parity. Compared with the
selected Milestone 1 baseline turns:

| Campaign runs | Prompt | Turn ms (M1 → M2) | Turn gain | Prefill ms (M1 → M2) | Reply ms (M1 → M2) | Reply tok/s (M1 → M2) | CPU attention us (M1 → M2) |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 → 4 | `hi` | 9,094 → 8,754 | 340 ms (3.739%) | 4,501 → 4,394 | 4,593 → 4,360 | 1.96 → 2.06 | 315,000 → 59,000 |
| 2 → 5 | `hi` | 9,113 → 8,772 | 341 ms (3.742%) | 4,543 → 4,396 | 4,570 → 4,376 | 1.97 → 2.06 | 309,000 → 54,000 |
| 3 → 6 | sky | 21,367 → 20,044 | 1,323 ms (6.192%) | 9,635 → 9,260 | 11,732 → 10,784 | 1.88 → 2.04 | 1,230,000 → 246,000 |

Across the three turns, elapsed time fell from 39,574 to 37,570 ms: a
2,004 ms, 5.064% gain. Prefill fell 629 ms (3.367%), reply fell 1,375 ms
(6.581%), and aggregate reply throughput rose from 1.914 to 2.049 tok/s
(7.044%). CPU attention time fell from 1,854,000 to 359,000 us, an 80.636%
reduction. In contrast, GPU timestamp time fell only 132,847 us (0.582%) and
completion time outside the timestamped walkers fell 358,153 us (2.661%).
The end-to-end gain is therefore primarily host-side, while the GPU data path
remains the Milestone 3 target.

The required schema-1 RCS probes contain 58/58 valid samples and zero invalid
samples. Their aggregate 590,820 us queue-to-observe interval comprises
481,178 us of walkers (81.442%), 108,016 us queue-to-batch (18.282%), and
1,655 us of preamble, epilogue, and release-to-observe phases (0.280%;
independently rounded phase totals). Vocabulary accounts for 281,124 us, or
58.424% of sampled walker time. These figures identify phase and signature
dominance; they must not be extrapolated to every submission because the
sampler records only the first and power-of-two successes per signature.

The immutable evidence copies are:

- `bld/baremetal-logs/lumen-m2-validation1.log`, SHA-256
  `6486474777dcd9cd5207466140993a6c0f7922f10089163d0a0db2cfdcda80db`;
- `bld/baremetal-logs/lumen-m2-validation2.log`, SHA-256
  `6efb19b691037f3c7a63fd0b36d9798fa2438836b586e0771ba12e78087f7b08`;
- `bld/baremetal-logs/lumen-m2-validation3.log`, SHA-256
  `036f3b09bdd41c5453921c706e7799361976d5b572e4f2fe3232b4b8be1baba7`.
- `bld/baremetal-logs/lumen-m2-runs4-6.log`, the non-rotating three-run
  aggregate used by the strict replay below, SHA-256
  `00ed16c766e89ddff525d76e77b970d8d15688eea7f2e9f9f110015d7ecf6439`.

Strictly replay the three-run aggregate with:

```sh
python3 -B tools/lfm25_baremetal_report.py \
  --expect-runs 3 \
  --require-rcs-probe \
  bld/baremetal-logs/lumen-m2-runs4-6.log
```

## Milestone 3 — GPU data path

Use the per-signature evidence from Runs 4–6 to optimize the actual dominant
GPU transport and graph costs. The identical packed kernel already sustains
roughly 46–49 GB/s through the Linux/NEO hardware oracle while TRUEOS observes
roughly 1.2–1.5 GB/s. Direct-RCS state, power, cache policy, dispatch topology,
and synchronization must therefore be isolated before changing arithmetic.
Candidate work is deliberately evidence-gated:

- add per-walker timestamps and GT frequency/state observations;
- compare 1/2/4/8 repeated walkers per submission to distinguish per-submit
  ramp/state overhead from steady kernel throughput;
- verify model/activation/output cache policy against the NEO oracle;
- move quantized activations and intervening layer operations GPU-resident,
  first targeting about 17 submissions per token and then a prebuilt
  one-submission token graph;
- consider tiled or split-K accumulation only after the transport gap is
  resolved, because changing FP accumulation order needs a separately versioned
  parity contract;
- keep the sealed packed ABI unless a versioned replacement proves a larger
  end-to-end win.

Milestone 3 is complete when Runs 7–9 pass, all local parity/ISA gates pass,
and the final audit demonstrates repeatable end-to-end gains without Lumen or
shared-RCS regressions.

### Implemented Milestone 3 intervention

The first evidence-gated GPU intervention restores the cache-policy
initialization that TRUEOS was missing. Before this build, TRUEOS programmed
the Gen12 PAT table but never initialized `GLOBAL_MOCS` or `LNCFCMOCS`, even
though every LFM surface and state-base-address entry selects MOCS index 4.
Reset values therefore left that index undefined by TRUEOS. The M3 build now:

- writes and reads back the complete 64-entry Gen12 global MOCS table;
- writes and reads back all 32 packed L3CC registers, in global-before-L3CC
  order;
- completes both tables while render/GT forcewake is retained and before GuC
  firmware, transport, and golden contexts are initialized;
- tracks PAT readback readiness separately from the complete PPGTT
  cache-policy contract;
- checks the complete PAT and MOCS tables after initialization, after GuC
  bring-up, at turn admission, after the first retired LFM submission, and at
  turn completion;
- revokes the combined contract on any mismatch before further PPGTT mapping
  or GuC submission, while preserving PAT-only display GGTT service if only
  MOCS is lost; and
- samples actual GT ratios immediately before submission and after completion
  observation for the same first-and-power-of-two signature samples already
  used by the RCS phase probe.

For this exact ADL-S device, upstream i915 defines MOCS index 4 as global
control `0x00000005` with L3CC `0x0030`; the packed L3CC register containing
entries 4 and 5 is therefore `0x00100030`. The implementation and ordering are
pinned to Linux commit `fc02acf6ac0ccde0c805c2daa9148683cdd01ba8`:

- [Gen12 MOCS entry definitions](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_mocs.c#L165-L178),
  [ADL-S table selection](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_mocs.c#L342-L368),
  and [table size/readback rules](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_mocs.c#L454-L492);
- [global-before-L3CC programming order](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_mocs.c#L666-L684);
- [global MOCS](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_gt_regs.h#L298-L299)
  and [LNCFCMOCS register definitions](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_gt_regs.h#L958-L963);
- [requested-frequency register](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_gt_regs.h#L772-L816),
  [Gen12 actual-frequency register](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_gt_regs.h#L1553-L1557),
  and [throttle-reason register and mask](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/i915_reg.h#L602-L611);
- [actual-frequency/RC6 and ratio decoding](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_rps.c#L2081-L2199),
  [capability decoding](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_rps.c#L1110-L1204),
  and [ratio-to-MHz conversion](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/drivers/gpu/drm/i915/gt/intel_rps.c#L1657-L1688);
- [MCHBAR RP0/RPe/RPn capability registers](https://github.com/torvalds/linux/blob/fc02acf6ac0ccde0c805c2daa9148683cdd01ba8/include/drm/intel/mchbar_regs.h#L224-L236).

PAT readiness and complete PPGTT cache-policy readiness are intentionally
separate. The display GGTT path retains its PAT-readback gate and its
address-plus-present system-memory PTE format. A new sparse PPGTT, however,
fails closed unless both the PAT table and the complete global-MOCS/L3CC
readback are accepted. This prevents a PPGTT leaf from selecting a known PAT
entry while its surface-state MOCS index still inherits unknown firmware or
reset policy. The gate sits under persistent direct-RCS/UI4 mappings, sparse
GPUVM mappings, BLT PPGTT mappings, and the common GuC scheduler admission
path, so an already-created context cannot bypass a later revocation.

This experiment intentionally does not force RP0, add an RPS owner, or change
SLPC. The `gpu_hz=19200000` field in the command-stream phase record is the
19.2 MHz render timestamp clock used to convert 36-bit RCS timestamps; it is
not GT core frequency. The separate schema-1 `turn-gt-state` record has an
`available` bit and samples `GEN12_RPSTAT1` with host CPU MMIO immediately
before submission and in the exact matched-marker branch, before diagnostic
logging or executor completion bookkeeping. It reports
`start_active`, `end_active`, `start_zero`, `end_zero`, ratio sums, active
average MHz, and per-signature buckets. Zero-ratio samples identify a boundary
at which `RPSTAT1` reported no active ratio, which is consistent with RC6 but
is not a direct RC-state measurement. These are submission-boundary samples,
not command-stream timestamps and not measurements taken inside a walker.

The record's final snapshot separately reports actual and requested
ratio/MHz, RP0/RPe/RPn capability MHz, throttle reasons, and raw `RPSTAT1` and
`RPNSWREQ` values. This lets Runs 7–9 establish whether cache policy alone
closes the transport gap and whether a later frequency-control experiment is
justified without conflating timestamp-clock and GT-core units.

On the fresh Run 7 boot, require these cache-policy checkpoints:

- `checkpoint=boot-init accepted=1`, with
  `after_global=0x00000005` and `after_l3cc_pair=0x00100030`;
- `checkpoint=post-guc accepted=1`; and
- `checkpoint=first-lfm-retire accepted=1`.

The `before_global` and `before_l3cc_pair` fields are part of the result: reset
or firmware values different from the expected values prove that M3 changed
the effective policy; already-correct values mean this particular mutation
cannot explain a speed change.

The focused profile routes the early claim and cache checkpoints through
GPGPU/Info while leaving the noisy general graphics area at Warn. It also
persists boot-init and post-GuC outcomes and emits one schema-1
`turn-admission` record after each turn. That record carries the exact BDF,
vendor/device/revision, boot `before_*` and `after_*` values, first-retire
status, turn-start and turn-end PAT/MOCS state, and GuC firmware/submission
readiness. Therefore a TCP capture can validate the boot policy even if its
connection began after the early boot lines or the finite log ring rotated
them out.

### Run 7 first physical check

Use one freshly deployed M3 build and a physical reset, then verify this
sequence before continuing to warm Runs 8 and 9:

1. Boot admission names `00:02.0`, vendor/device `8086:4680`, and revision
   `0x0c`.
2. `intel/cache-policy` has `accepted=1`, `pat=1`, and `mocs=1`.
3. The `boot-init` and `post-guc` MOCS checkpoints are accepted; boot-init
   has the exact index-4 values above, and its `before_*` fields are preserved
   as evidence.
4. Run exactly `lum "hi"` as the first fresh turn after reboot.
5. Require `checkpoint=first-lfm-retire accepted=1` with global
   `0x00000005` and L3CC pair `0x00100030`.
6. Require the canonical 10 prompt tokens, 9 reply tokens, EOT response and
   response hash, zero projection failures, a valid schema-1
   `turn-rcs-probe`, and a schema-1 `turn-gt-state` with `available=1` and at
   least one boundary sample. Its active-plus-zero counts must equal
   `samples` independently at both start and end. Also require schema-1
   `turn-admission` with the exact target identity, all boot/post-GuC/
   first-retire/turn-boundary cache gates accepted, index-4 values
   `0x00000005` and `0x00100030`, and GuC firmware/submission readiness.
7. Preserve the complete boot-through-turn log. If any admission, cache
   checkpoint, parity, or telemetry gate fails, stop the set rather than
   treating Runs 8 and 9 as comparable warm runs.

Run 7 is diagnostic evidence, not by itself a performance conclusion. After
all three physical runs, validate Runs 7–9 strictly with:

```sh
python3 -B tools/lfm25_baremetal_report.py \
  --expect-runs 3 \
  --require-rcs-probe \
  --require-gt-state \
  --require-m3-admission \
  bld/baremetal-logs/LatestOfThree.logs
```

## The nine bare-metal runs

Deploy exactly one build at the start of each milestone validation set. Do not
reboot between the warm runs. After each response, close the Matrix slot that
owns the resident `lum` session (the slot named by `lum: resident ready`), not
only the separate Spirit response window. Start the next command from a fresh
Matrix slot. This resets conversation/KV state while the immutable model assets
remain boot-resident.

| Run | Milestone | State | Command |
| ---: | --- | --- | --- |
| 1 | Measurement | first turn after reboot | `lum "hi"` |
| 2 | Measurement | fresh session, same boot | `lum "hi"` |
| 3 | Measurement | fresh session, same boot | `lum "Explain why the sky is blue in one short sentence."` |
| 4 | Host/submit | first turn after reboot | `lum "hi"` |
| 5 | Host/submit | fresh session, same boot | `lum "hi"` |
| 6 | Host/submit | fresh session, same boot | `lum "Explain why the sky is blue in one short sentence."` |
| 7 | GPU data path | first turn after reboot | `lum "hi"` |
| 8 | GPU data path | fresh session, same boot | `lum "hi"` |
| 9 | GPU data path | fresh session, same boot | `lum "Explain why the sky is blue in one short sentence."` |

For the first validation set, build/deploy once, execute Runs 1–3 in the order
above, and then validate the captured log:

```sh
make iso
python3 tools/lfm25_baremetal_report.py \
  --expect-runs 3 \
  bld/baremetal-logs/LatestOfThree.logs
cp --dereference \
  bld/baremetal-logs/LatestOfThree.logs \
  bld/baremetal-logs/lumen-m1-runs1-3.log
```

`make iso` performs the configured test-rig deployment, physical reset, and
log-drain setup. The three `lum` commands themselves are entered manually on
the booted TRUEOS system. Preserve the resulting log before the next deploy so
the rotating three-slot log drain cannot replace campaign evidence.

The pinned oracle expectations are:

- `hi`: 10 prompt tokens, 9 reply tokens, EOT, response
  `Hello! How can I help you today?`, trimmed-response SHA-256
  `fda564ba3f7a0f028106d468420f674898ed99ac5bf2765ac9586206e39d73c5`;
- sky prompt: 21 prompt tokens, 22 reply tokens, EOT, response
  `The sky appears blue due to Rayleigh scattering, where shorter wavelengths of light are scattered more than longer ones.`,
  trimmed-response SHA-256
  `79953eee1910284066aebc0a0147a1359c9b6ca6778ac98fc43f1eec05e5b3ce`.

If the Spirit emotion adapter is enabled, validate the raw Lumen token summary
and parity records rather than requiring the displayed presentation text to be
byte-identical.

## Per-run acceptance gates

Every run must satisfy all of these:

1. one `lfm25: turn stage=start`, one `stage=prefill`, and one `stage=done`
   record for the same turn;
2. zero projection failures and no LFM completion timeout or lane quarantine;
3. callback, projection, and submission counts match the fixed decode schedule;
4. valid GPU timestamps for every successful submission;
5. exact prompt/reply token counts and expected EOT behavior;
6. exact canonical output, subject only to the documented Spirit presentation
   adapter;
7. no new GPGPU, display, or Spirit timeout attributable to the inference
   change.

## Comparison metrics

Record these from each run:

| Metric | Why it matters |
| --- | --- |
| resident open and pack/seal ms | cold-start cost |
| turn elapsed ms | user-visible end-to-end result |
| first-token/prefill elapsed ms | interaction latency |
| reply tokens per second | steady decode result |
| GPU us / submission | kernel data-path cost |
| completion us minus GPU us | scheduling/polling cost |
| submit ms minus accounted phases | host preparation/copy blind spot |
| projections / submission | batching effectiveness |
| per-signature GPU and completion us | identifies the next dominant shape |
| interleaved shared-RCS submissions | UI contention |

The campaign optimizes the end-to-end result first. A microbenchmark gain that
does not improve the corresponding bare-metal turn is diagnostic evidence, not
a milestone victory.
