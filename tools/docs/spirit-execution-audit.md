# Spirit execution isolation audit

Date: 2026-09-23. Scope: source inspection of the current working tree.
No hardware reproduction, reset, deployment, or runtime change was performed.
This establishes architectural failure paths, not attribution of a particular
observed freeze. Proposed requirements below are not implemented guarantees.

## Required contract

Spirit owns the hardware cursor presentation path. Ordinary GPU workloads must
not consume its mutable execution resources or indefinitely prevent its service.
Its diagnostic view must distinguish healthy progress, overload, a quarantined
client, recovery, and unavailable evidence. A retained old frame is not health.

Separate cursor scanout and private address spaces do not alone establish a
service deadline or recovery boundary. Shared physical-engine failure remains
a separate fault domain which the software must handle explicitly.

## Findings grounded in source

1. **Spirit's execution context is shared with Lab256.**
   `src/intel/gpgpu/operations/spirit_vfx.rs::submit_spirit_vfx_frame` and
   `src/intel/gpgpu/operations/lab256.rs` use `EXECUTION_RCS_SUBMIT_LOCK`,
   `EXECUTION_RCS_DETACHED_TAG`, and `execution_rcs_state_once`.
   `src/intel/gpgpu/rcs/runtime.rs::execution_rcs_state_once` rejects access
   while detached or quarantined. Separate Helio/Spirit HWLRCAs and PPGTT roots,
   checked by the broker, do not establish exclusive Spirit ownership against
   Lab256. This is a concrete coupling regardless of whether Lab256 caused the
   reported incident.

2. **Spirit has no demonstrated maximum scheduling delay.**
   `src/gpu/vgpu.rs::KernelClient::physical_priority` assigns the execution
   lane normal priority, while system GPGPU, Font, UI4 compositor, and LFM25
   receive high priority. `src/gpu/executor.rs::kernel_client_inflight_limit`
   bounds outstanding requests (two for UI4, one otherwise). That is useful
   storage/admission control, but does not bound execution duration or the
   aggregate demand of continuously replenished clients. These facts identify
   a service-risk mechanism; source inspection does not prove observed starvation.

3. **The Spirit timeout deliberately permits indefinite pending state.**
   `src/intel/gpgpu/operations/spirit_vfx.rs::poll_spirit_vfx_submission`
   requires a marker plus saved-HEAD retirement proof. After the 1,000 ms
   timeout, it quarantines the execution lane, retains the job and backing,
   and continues returning `Pending`. The Spirit worker's pending branch in
   `src/spirit/mod.rs` yields and continues before downstream presentation
   and logger service. A late retirement proof is possible, but quarantine
   still prevents ordinary future admission. This is containment, not service
   recovery. Releasing the lease on timeout would introduce a late-write hazard.

4. **Presentation has another terminal failure path.**
   `src/spirit/mod.rs` checks cursor SURFLIVE with a 100 ms timeout, retries
   cursor programming once, and returns from the worker on terminal failure.
   This must be diagnosed separately from an unretired render request.

5. **Automatic reset recovery is explicitly incomplete.**
   `src/intel/guc.rs::build_ads` sets
   `GLOBAL_POLICY_DISABLE_ENGINE_RESET`; its comment identifies missing golden
   reset contexts and physical-boundary device-loss/reset propagation.
   `src/intel/gpgpu/rcs/commands.rs::quarantine_direct_rcs_lane` rejects future
   submissions until reboot. The independent fault pump and exact-context
   containment in `src/gpu/vgpu.rs` are useful foundations, but do not establish
   a complete recovery transaction.

6. **Existing timing and diagnostics do not answer whole-device health.**
   `src/intel/guc_submission.rs` requests a 1,000 us execution quantum and
   a 7,500,000 us preemption timeout. Neither is a proven Spirit deadline;
   reducing a timeout does not supply the missing recovery mechanism.
   `src/spirit/gpu_logger.rs::GpuLoggerSample` exposes producer frame timings,
   counts, and retries, with Helio as the current production source. This is
   useful workload telemetry, not an inventory of every running GPU client.

## Repair order and acceptance criteria

1. **Make progress and ownership observable.** Capture per-client identity,
   engine/context generation, admitted request, age, last retirement, outstanding
   bytes, quarantine reason, and recovery state. Distinguish queued, published,
   and proven retired work; do not claim that submitted work is currently
   executing. Track Spirit producer progress and display-live progress separately.
   Keep diagnostic collection independent of a blocked Spirit producer.
   Acceptance: every admitted request can be attributed through completion or
   loss; stale observations visibly age instead of appearing healthy.

2. **Give Spirit an exclusive execution client.** Separate it from Lab256
   across principal, HWLRCA, PPGTT, mutable encoder resources, locks, pending
   state, quarantine, and completion ownership. Audit mappings and all broker
   identity checks together. Acceptance: Lab256 busy/fault injection cannot
   reserve or quarantine Spirit's client; unrelated clients remain usable.

3. **Establish bounded service under admitted load.** Define measurable Spirit
   producer/display deadlines and per-class budgets. Bound work per dispatch
   and outstanding memory; defer or reject excess work before publication.
   Measure completion latency and preemption behavior under combined workloads
   before selecting numeric caps. Priority is one mechanism, not the contract.
   Acceptance: sustained admitted mixed load meets the chosen deadline;
   overload produces backpressure with an attributed reason and bounded memory.

4. **Implement recovery with ownership proof.** Use an explicit progression:
   suspect -> stop admission -> contain -> prove quiescence or complete an
   appropriate reset -> invalidate affected generations -> rebuild -> resume.
   Keep old allocations pinned until old work provably cannot write them.
   Attribute context faults narrowly; a shared-engine/GT reset must notify every
   affected client and reject stale completions. Rate-limit repeated recovery.
   Acceptance: injected timeout, ambiguous publication, context fault, and
   engine loss reach either bounded recovery or an explicit terminal state,
   without unsafe reuse, infinite restart, or silently frozen health display.

5. **Keep an honest degraded Spirit path.** Reserve separate CPU-writable
   presentation backing that pending GPU work cannot target, and service it
   without waiting for that work. Display degraded/recovering status rather
   than disguising GPU failure with normal animation. Preserve display flip
   lifetime proofs; display-engine failure can also defeat this path.
   Acceptance: a stuck producer does not stop diagnostic updates when the
   display path remains functional; display failure is independently reported.

## Validation boundary

Run deterministic ownership/state-transition tests before hardware fault tests.
On hardware, collect request ages, exact identities, retirement and SURFLIVE
evidence before any reset. Exercise sustained mixed load as well as isolated
faults; successful clean frames alone do not validate overload or recovery.
No numeric service guarantee or claim that a moving avatar proves all GPU
calculations correct is justified by this source audit.
