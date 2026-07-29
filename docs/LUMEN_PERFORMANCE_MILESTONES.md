# Lumen end-to-end performance campaign

This document is the durable control plane for improving the fixed
LFM2.5-350M Q8 inference path. The campaign has three implementation
milestones. Each milestone ends with the same three bare-metal validations, so
the nine runs remain comparable from the first diagnostic build through the
final optimized build.

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
