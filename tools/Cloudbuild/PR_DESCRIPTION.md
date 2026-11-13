# Add configurable cloud builds, per-app buildins, and size estimates

People should be able to choose kernel capabilities and bundled tools,
including zero tools, and request a personal build by proving possession of
their public key. This handoff preserves the implementation and conversation
record for local integration.

## Included

- Subsystem presets and twelve independent app choices, with all/none.
- Exact app selection in schema 2 manifests, hashes, challenges and receipts.
- Real selected-package preparation and regular zero-app kernel packaging.
- Python/OpenSSH build control plane, private job queue, worker, grants and
  signed artifact receipts; separate manual Actions build workflow.
- Measured release-reference sizes, live selected/omitted app totals, and
  explicit derived/unmeasured labels.
- Conversation record, build/configuration guide and Server architecture.

## Scope and integration

USB/iGPU/Virgl currently gate initialization, not full driver compilation.
The GPU-free Server preset is design only. Kernel baseline and app weights
are historical 0.0.235 measurements, not an exact prediction of a new image.
The key flow authorizes builds and signs receipts; it adds no boot activation.

The work is based on v0.0.235. Current `true` was observed at `94af018`.
There are three textual merge conflicts and additional AppDB metadata/seed
interface changes. See `INTEGRATION.md` and the conversation record before
reconciling. Full merged-source build and boot validation remains outstanding.

## Recorded validation

Implementation-session results: 121 cloudbuild tests passed with three
compiler-dependent skips; 20 rig/preparation checks passed, with Cargo
discovery unavailable; ten grouped size UI/real API checks passed. No full
kernel compilation, visual browser assertion or production deployment is
claimed. Export integrity is checked separately in `validation/`.
