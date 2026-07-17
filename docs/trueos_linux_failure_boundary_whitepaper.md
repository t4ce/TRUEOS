# The Scanout Convention

## TRUEOS and the Linux desktop under equal cross-examination

- **Status:** technical position paper and experiment proposal
- **Audit date:** 17 July 2026
- **TRUEOS base revision:** `e9e46afbef97bc9ba0046fd990de21c52af2e3ab` (`true`)
- **Audit condition:** the checkout contained uncommitted author work; this paper does not treat that work, a local build, or project-authored logs as independent verification.

## Abstract

A graphical workstation should not turn one fault in its core user interface into the disappearance of every application, every visible work surface, and the user's authenticated session. That requirement is legitimate regardless of whether the system is Linux, TRUEOS, Windows, or a research kernel. It is a statement about failure containment, not blame.

This paper compares TRUEOS with the actual Linux desktop stack rather than with a straw man called “Linux.” On one side is a small, vertically integrated Rust `no_std` kernel that directly drives selected Intel Gen12 graphics hardware, owns a kernel-side UI4 frame/window contract, and is building VM-mediated application and vGPU boundaries. On the other side is Linux DRM/KMS plus a GPU driver, Mesa or another userspace driver, a Wayland compositor such as Mutter, desktop-shell policy, and applications. The stacks optimize for different constraints.

The comparison produces no ceremonial winner. TRUEOS has unusually direct ownership and can change its entire contract without negotiating decades of compatibility. Linux has hardware breadth, mature per-process isolation, stable interfaces, deployment experience, and recovery machinery that TRUEOS does not yet match. Linux's usual Wayland session architecture nevertheless concentrates display-server, window-manager, compositor, and often shell policy in a failure domain whose death disconnects the session's clients. TRUEOS currently avoids that exact process graph, but its compositor-like UI4 consumer and many services execute inside the kernel. A panic on a non-disposable core can halt the system, and the current UI service path is not an independently restartable, proven recovery domain. Smaller is not automatically safer.

The useful result is a convention: put both stacks on the same hardware, inject the same faults, measure what remains alive, and require evidence at the scanout, application-state, security-boundary, and human-session levels. The core question is not “whose code is cleaner?” It is:

> What is the smallest fault that can erase the user's visible and authenticated working identity, and how much of that identity can the system restore without asking the user to reconstruct it?

## 1. Claims this paper does and does not make

### 1.1 The claims

1. **A display server is a critical service, not the user's identity.** Its failure should have a bounded and tested recovery path.
2. **Trigger and blast radius are separate facts.** A GPU driver, compositor, extension, client, or hardware defect may trigger a failure. The architecture determines what else dies with it.
3. **The comparison unit is the complete usable stack.** TRUEOS kernel plus its UI and Blueprint/VM environment must be compared with Linux kernel plus DRM driver plus userspace graphics plus compositor plus session services.
4. **Direct ownership is a valuable design property, not a proof of correctness.** Fewer translations make state easier to reason about. They can also place more code inside one privileged failure domain.
5. **Compatibility is a systems property.** Linux's old interfaces are not merely clutter; they preserve working hardware and applications. They also constrain redesign.
6. **Recovery must be demonstrated, not inferred from type names or architecture diagrams.** A `DeviceLost` enum, a generation counter, or a reset function is useful machinery. It is not evidence that an interactive session survives an actual GPU hang.

### 1.2 The non-claims

- This paper does not attribute the motivating GNOME Shell crash to the Linux kernel, Intel, Ubuntu, Mutter, or any client without a proven causal trace.
- It does not claim that X11 generally survives an X server crash. A separately crashing X11 window manager may be restarted while clients remain connected, but an X server failure still terminates the display connection.
- It does not claim TRUEOS is presently a general Linux replacement, a secure multi-user system, or a conformant OpenCL implementation.
- It does not claim that Linux's size makes it bad or that TRUEOS's size makes it correct.
- It does not claim novelty from absence of search results. The project may make a carefully qualified public-research claim, but priority needs independent literature and artifact review.

## 2. Evidence rules

The convention uses four evidence grades:

| Grade | Meaning | Examples |
|---|---|---|
| A | Independently reproducible current evidence | tagged source, clean build recipe, signed artifact, raw trace, independent rerun |
| B | Author-produced measurement with inspectable method | serial log, hardware counter dump, framebuffer hash, packet comparison |
| C | Implemented mechanism not yet tested end to end | reset helper, device-loss state, opaque handle contract |
| D | Design, roadmap, or aspiration | planned SFC hot path, future security hook, proposed recovery supervisor |

Every result must also name its layer:

- **hardware:** scanout, engine, firmware, reset domain;
- **kernel:** memory, scheduler, driver, interrupt, process/VM boundary;
- **display service:** composition, window graph, input routing, KMS ownership;
- **application:** buffers, model state, unsaved work;
- **session identity:** credentials, authorization agents, user services, workspace placement.

“The screen came back” is not a complete recovery result. Neither is “the process stayed alive.” A valid report records all five layers.

## 3. Audit snapshot

The following observations are from the checkout named above and are evidence about its state, not general judgments of quality:

- The root binary is Rust `#![no_std]` and `#![no_main]`; both development and release profiles use `panic = "abort"`.
- Git history at the audit point contained 1,994 commits: 1,991 authored as `t4ce` and three as `Jonas`, using the same email address. This shows concentrated control and velocity, but also a one-person review and continuity risk.
- The tree contained 3,024 tracked paths, of which roughly 1,170 were first-party and 1,854 were under vendored trees in this audit's path classification. These counts are inventory, not a score.
- A present first-party Rust line count, excluding nested vendor trees, was roughly 297,000 lines. The Intel tree alone was roughly 87,000. This is no longer a toy-sized codebase even though it is far smaller in scope than Linux and its desktop ecosystem.
- The checked release workflow builds and signs release assets, but no test, `cargo check`, or Clippy stage was visible in that workflow.
- The repository had a roughly 22 MB current debug ISO and a roughly 15 MB archived release ISO. The README's “around 5 MB” sentence is stale for these artifacts and should not be used as a current metric.
- The release provenance record correctly failed verification against the changed checkout and current `bld/trueos.iso`. Its recorded ISO hash did match the archived `bld/trueos-release/trueos.iso`. That is useful fail-closed behavior, but the audited workspace is not a clean reproduction of that record.
- The repository is **source-available, not open source** under the usual OSI meaning. Its license permits public review but withholds general modification, redistribution, and fork rights. That can protect authorial control, but it materially limits independent replication, downstream maintenance, and community contribution.

The audit deliberately does not convert commit count, line count, language choice, or artifact size into correctness claims.

## 4. A direct one-to-one model

| Axis | TRUEOS, current checkout | Conventional Linux Wayland workstation |
|---|---|---|
| Design center | One vertically controlled workstation OS and research vehicle | General kernel and ecosystem supporting many vendors, devices, desktops, and workloads |
| Hardware scope observed in code | Explicit Intel device IDs for Alder Lake-S GT1 (`0x4680`), Alder Lake-N (`0x46D1`), and Raptor Lake-S UHD 770 (`0xA780`) | Broad driver ecosystem; support and quality remain device/driver specific |
| Display ownership | Kernel Intel display code and UI4 paths directly manipulate the target hardware | Kernel DRM/KMS exports display objects; a privileged userspace compositor normally owns session policy and commits |
| Rendering | Direct Intel MMIO/GGTT/PPGTT, GuC path, native render/GPGPU/media experiments | Kernel driver plus Mesa or vendor userspace, standardized and vendor-specific APIs |
| Window/frame contract | UI4 generation-tagged handles, bounded registries, frame leases, explicit ownership and plane placement | Wayland objects and compositor policy; application-owned buffers passed to compositor; KMS state below |
| Application boundary | Blueprints and a VM/Hull/vmcall path under active construction; some kernel apps and services remain in-kernel | Mature process address spaces, credentials, cgroups/namespaces/LSMs, IPC, optional VMs/containers |
| GPU tenant boundary | Principal-scoped vGPU broker with opaque, generation-tagged handles and quotas; physical loss propagation not wired end to end | Render nodes, per-file driver contexts, GPU VMs where supported, fences/schedulers, driver-specific reset and isolation |
| UI failure domain today | UI4 consumer/input/screenshot tasks run as kernel tasks; kernel panic usually halts, except a special disposable worker-AP restart path | Compositor/shell is a userspace process, so the kernel and unrelated system services normally survive; its Wayland clients usually lose their server connection |
| Compatibility burden | Small, author-controlled, intentionally narrow | Very large kernel UAPI, userspace, hardware, application, and behavioral compatibility burden |
| Security maturity | Explicitly incomplete VM security hooks and broad mappings remain | Extensively deployed process and kernel security model, still with a large attack surface and recurring defects |
| Change velocity | A single owner can change all layers together | Changes cross subsystem, distribution, desktop, vendor, and application boundaries |
| Independent validation | Publicly inspectable source and author-produced hardware evidence; restrictive reuse license and one-author history constrain reruns | Large public review and test ecosystem, but integration combinations still escape coverage |

This table explains the disagreement better than “kernel versus kernel.” TRUEOS is currently closer to a single product team owning firmware-facing driver, compositor contract, application runtime, and system services. Linux is a federation of contracts.

## 5. TRUEOS's actual ownership path

At a high level, the current source traces this path:

```text
bootloader
  -> _start / known boot stack
  -> kmain
     -> memory, exceptions, SMP, heap, PCI, DMA
     -> intel::init_once
        -> claim selected Intel display device
        -> display/GGTT/PPGTT setup
        -> optional GuC firmware, ADS, CTB and scheduling
        -> primary scanout surface
     -> Embassy executors and central service registry
        -> UI4 input service
        -> UI4 screenshot service
        -> temporary in-kernel UI4 composition consumer
        -> GPGPU preview and media producers

Blueprint/application
  -> VM/Hull and vmcall ABI
  -> trueos-v vGPU facade
  -> principal-scoped vGPU broker
  -> physical Intel device / GuC scheduler
  -> opaque UI4 frame/window publication
  -> composition and scanout
```

The attractive part is visible: there is no mandatory DRM-to-Mesa-to-compositor translation just to understand which TRUEOS code owns a frame. UI4 defines cadence, format, lease, publication, window owner, session, placement, and handle generation in a compact graph. Producers do not receive a display-plane address. The public vGPU facade does not expose raw MMIO, physical addresses, PPGTT entries, GuC identifiers, or arbitrary command streams. Default vGPU capabilities exclude presentation.

That is good interface design.

The current failure boundary is less attractive. `kmain` starts many privileged asynchronous services in one kernel image. The panic handler prints context and then halts; only a specially classified disposable worker AP has a panic restart route. The temporary “dummy” UI4 consumer is the current primary-compositor-like service. It can return on initialization or runtime errors. Unlike tasks that hold `TaskRunGuard`, it does not clear its central `started` flag when returning, so the service loop does not respawn it in that case. Even if it did respawn, frame graph restoration and scanout continuity have not been demonstrated.

The result is not “TRUEOS has the recovery problem solved.” It is “TRUEOS has enough ownership information to design a much cleaner recovery contract, and now must prove it.”

## 6. The motivating host incident: what it establishes

The local Ubuntu host incident that motivated this comparison had a precise observable boundary:

- Ubuntu 26.04 was running GNOME Shell 50.1 and Mutter 50.1 on Wayland.
- At 20:57:57, GNOME Shell PID 8058 logged a signal 11 crash.
- `org.gnome.Shell@ubuntu.service` exited with `status=11/SEGV` and a core dump.
- GDM closed the old session; a new login session appeared at 20:58:08.
- The applications attached to the failed Wayland display disappeared with the session.
- No OOM event, kernel GPU hang/reset, or NVIDIA Xid was observed at that exact time.
- An earlier GNOME Shell core on the same host reached `g_signal_emit_by_name`, an address resolved to `clutter_stage_view_schedule_update_now`, and `clutter_frame_clock_dispatch`. That narrows the location of that earlier failure; it does not prove a root cause for the later one.

Therefore:

**Proven:** a userspace session-compositor/shell failure had whole-session graphical blast radius and forced a new login.

**Not proven:** the kernel, Ubuntu, Intel, NVIDIA, a particular extension, or a particular application caused the invalid access.

This distinction matters. The criticism survives without overstating causality: even if a client supplied the trigger, the desktop architecture allowed the display-service failure to erase the visible session.

## 7. Why the Wayland blast radius is real—and why Wayland chose it

The Wayland architecture says directly that “the compositor is the display server.” The compositor receives input, owns the scene graph, accepts application buffers, and submits page flips through KMS. The Wayland FAQ likewise says the architecture integrates display server, window manager, and compositor into one process. See the [Wayland architecture](https://wayland.freedesktop.org/architecture.html) and [Wayland FAQ](https://wayland.freedesktop.org/faq.html).

That integration removed redundant routing and allowed the component with the correct scene graph to own input transformation and final composition. It was a rational answer to X11's split state. It also creates a conspicuous fuse: if the server connection and its object graph die, clients cannot simply continue speaking the same connection to a replacement process.

This is not forced by the Wayland protocol's existence. Wayland's own documentation describes a **system compositor** that can run from early boot to shutdown and host nested **session compositors**. A failed session compositor could therefore be contained beneath a longer-lived display owner, at least at the scanout and session-selection layers. See [Types of Compositors](https://wayland.freedesktop.org/docs/book/Compositors.html). The hard problem is preserving or reconstructing each client's protocol objects, buffer ownership, security state, and application-level model after its server endpoint disappears.

Linux also provides strong primitives below the compositor:

- DRM/KMS models framebuffers, planes, CRTCs, connectors, atomic validation, and commit. The kernel requires atomic drivers to validate hardware constraints before commit and to keep an enabled display pipe running through many error cases. See [Linux KMS documentation](https://docs.kernel.org/gpu/drm-kms.html).
- DRM render nodes deliberately remove modesetting and privileged ioctls from unprivileged render clients, separating rendering access from the display master. See [DRM userspace interfaces](https://docs.kernel.org/gpu/drm-uapi.html#render-nodes).
- The DRM scheduler defines timeout callbacks and driver-specific reset procedures. For hardware schedulers, the documented recovery shape stops affected schedulers, kills the guilty entity, resets faulty rings, resubmits innocent work, and restarts scheduling. See [DRM memory management and scheduler documentation](https://docs.kernel.org/gpu/drm-mm.html).

Those kernel mechanisms mean “Linux has no recovery architecture” would be false. The sharper statement is that GPU job/display recovery below the compositor and session-object recovery above it are different problems, and the latter remains visibly weak in a conventional monolithic Wayland desktop session.

## 8. What TRUEOS has earned the right to claim

### 8.1 Direct Intel ownership

The source contains direct PCI discovery, MMIO, GGTT, PPGTT, display-plane, pipeline, GuC firmware/ADS/CTB/submission, render, GPGPU, and media work for selected Intel Xe-LP/Gen12-class devices. The README documents an author-observed July 2026 3D milestone with unchanged tessellated mesh data, pipeline counters, pixel checks, and scanout without Linux DRM/i915/Mesa below it.

That is meaningful OS development. It is more than a framebuffer demo and should be evaluated as such. The evidence is still project-produced until another party reproduces it from a pinned artifact and test plan.

### 8.2 A coherent frame ownership model

UI4's strongest current work is not visual styling. It is the contract:

- immutable, dirty, and streaming cadence map to bounded buffering policies;
- generation-tagged frame/window handles reject stale identities;
- producers acquire checked lease tokens and must publish or cancel them;
- owners and sessions are explicit;
- the window broker enforces global and per-session bounds;
- presentation placement is a broker decision rather than an address handed to producers;
- frame format and alpha semantics are explicit.

These properties reduce ambient authority and make cleanup enumerable. They are the right raw material for crash recovery.

### 8.3 A mediated vGPU direction

The vGPU broker associates devices, queues, resources, quotas, timelines, and generations with principals. It rejects cross-principal handle use and keeps the guest-facing ABI away from raw command submission and privileged display control. The documented shell tests check broker isolation, ABI surface, GuC scheduling, quotas, stale handles, cleanup, and simulated device loss.

The limitation must travel with the claim: the test loss is simulated in broker state. The physical `notify_physical_device_lost()` hook has no caller in the audited tree. GuC autonomous engine reset is explicitly disabled because golden reset contexts and device-loss propagation are not complete. Some render experiments contain direct render-engine reset helpers, but that does not yet establish safe multi-tenant recovery.

### 8.4 Reproducibility machinery that fails closed

The release process records source identity, Git state, submodules, tool versions, and hashes for ELF and ISO artifacts, then signs release material. The local verifier rejected a changed checkout and mismatched current artifact. That is the correct direction.

The next bar is a clean, public, independently rerunnable release in which the signed record, source permission, exact hardware configuration, firmware hashes, and raw experiment output can all be checked by a third party.

## 9. What TRUEOS may not honestly claim yet

### 9.1 Production fault containment

Kernel-owned UI state is not automatically fault-contained UI state. An in-kernel out-of-bounds access, deadlock, panic, or corrupted shared structure can have a larger blast radius than a userspace compositor crash. TRUEOS needs a surviving supervisor and a recoverable display state machine, not just direct scanout ownership.

### 9.2 Hardened untrusted application isolation

The hypervisor path itself documents unfinished security:

- security hooks are labeled cheap stubs;
- the VM-exit mode is `StubOnly`;
- IBPB and related boundaries are future work;
- selected broad host spans and legacy RWX mappings remain;
- one guest-run path documents guest-physical-to-host-physical identity mapping across four gigabytes and shared host heap/time/statics;
- host and guest can contend on shared network queues;
- crash reports do not yet capture the promised DWARF stack.

The code is evolving, and newer sparse-EPT work may narrow parts of this. Until an explicit default-deny map, W^X permissions, DMA/IOMMU story, speculation boundary, and adversarial tests are demonstrated, “VM” must not be used as a synonym for “secure.”

### 9.3 General graphics compatibility

The OpenCL module explicitly says it is not a complete Khronos implementation and contains execution stubs. Media roadmaps distinguish present CPU fallback/oracle paths from future SFC hot paths. The supported Intel PCI-ID list is narrow. None of that diminishes the direct bring-up achievement; it defines the current product boundary.

### 9.4 Community-verifiable openness

Public readability is valuable, but a license that forbids ordinary modifications, derivative works, and redistribution prevents the normal fork-build-patch-review loop. A convention can still test official artifacts, but the strongest reproducibility claims require either broader research permissions or a formally managed independent test program.

## 10. What Linux has earned the right to claim

### 10.1 Breadth and compatibility as engineering results

Linux supports a hardware and workload range TRUEOS does not attempt. Its userspace/kernel interface policy and “no regressions” discipline make compatibility an explicit responsibility, not an accidental property. The policy is imperfect and best-effort in places, but it places the user's existing workload above architectural tidiness. See [Linux regression handling](https://docs.kernel.org/process/handling-regressions.html).

The cost is real: old exposed objects cannot simply be removed, drivers differ, distributions compose components differently, and a redesign must move multiple communities without breaking users. Linux's complexity is partly accumulated debt and partly stored compatibility value.

### 10.2 Mature privilege separation

An ordinary application, compositor, display manager, GPU render client, system service, and kernel are distinct protection domains. Render nodes exclude global modesetting ioctls. Filesystem permissions, credentials, process address spaces, seccomp, cgroups, namespaces, LSMs, containers, and VMs can further narrow authority. These mechanisms have bugs, but their operational maturity exceeds TRUEOS's current VM/Hull isolation.

### 10.3 GPU and display recovery primitives

KMS atomic state, fences, per-client contexts, hang accounting, scheduler timeout callbacks, reset procedures, and documented hot-unplug constraints give drivers and userspace a common framework. Hot-unplug and recovery support remain incomplete across some driver/userspace paths. Implementation quality varies by driver and hardware, and a recovered kernel GPU does not guarantee a recovered compositor. Still, these are substantial mechanisms rather than “blabla.”

### 10.4 Multiple deployment shapes

The conventional GNOME session is not the only possible Linux architecture. A long-lived system compositor, nested session compositor, single-application kiosk, remote compositor, or gaming micro-compositor can move boundaries. Valve's [gamescope](https://github.com/ValveSoftware/gamescope) demonstrates a focused nested or embedded compositor and can isolate a game inside its own Xwayland instance. It does not solve arbitrary desktop-session restoration, but it proves that narrower policy domains can be deployed on the Linux stack.

## 11. The Linux critique, stated fairly

1. **The desktop session has a single conspicuous graphical failure domain.** Mutter/GNOME Shell combines compositor and shell responsibilities; losing it commonly loses all native Wayland connections and the visible workspace.
2. **Recovery is fragmented across layers.** The kernel may reset a GPU while Mesa, the compositor, toolkit, application, and session manager each retain incompatible assumptions about lost buffers or contexts.
3. **The user experiences the composition, not the organizational chart.** Saying “the kernel stayed up” is technically important but humanly incomplete if every visible application vanished and authentication restarted.
4. **Compatibility slows boundary repair.** Existing client, toolkit, protocol, driver, and desktop behavior makes connection handoff or transparent replay a multi-project change.
5. **No single maintainer owns the whole outcome.** Linus Torvalds can enforce kernel merge and regression policy. He does not alone control Mutter, Wayland protocols, Mesa, systemd user sessions, GDM, distributions, proprietary firmware, or applications. Accountability must follow the ownership graph.

The last point rejects both excuses. “It is all Linus” assigns power he does not possess. “It is userspace, not our problem” ignores the user's end-to-end system. The convention seats every owner whose boundary contributes to the result.

### 11.1 Why people can repeat the correct criticism and still not move the stack

The missing ingredient is rarely one more person noticing the failure. A transparent recovery design crosses at least the compositor's object graph, the Wayland transport, toolkit behavior, application semantic state, GPU buffer/context loss, session credentials, display-manager policy, and distribution supervision. No participant can land that redesign alone, and the intermediate state may be worse than the current one. A replacement compositor that comes back to an empty protocol graph restores pixels but not work. A reconnect protocol without toolkit and application adoption restores a socket but not meaning. A system compositor without a strict privilege model may merely create a second critical server.

Linux is particularly difficult to move because a valid migration must be incremental, must coexist with old clients, and must not turn rare compositor crashes into everyday regressions. The same compatibility discipline that makes Linux dependable makes cross-layer correction slow. TRUEOS can change every side of an interface in one commit, which is a real experimental advantage. The counterweight is that one full-stack developer can share one mistaken assumption across every layer. Sitting in front of, using, and developing the whole system supplies unusually valuable end-to-end knowledge; it does not replace adversarial review.

Responsibility therefore divides by evidence:

- a kernel hang, failed KMS invariant, or driver reset defect belongs first to the relevant Linux/DRM/vendor maintainers;
- a Mutter crash or unrecoverable compositor state belongs first to Mutter/GNOME;
- a missing generic reconnect concept is a Wayland ecosystem design question;
- a display manager that converts a recoverable service failure into session destruction owns that policy choice;
- an application that cannot restore semantic state owns part of the last mile;
- the distribution owns the integration, supervision, version combination, and user-facing recovery promise;
- the complete desktop community owns the end-to-end requirement that no single subgroup can satisfy alone.

That map is less emotionally satisfying than one name. It is more actionable.

## 12. The TRUEOS critique, stated fairly

1. **The clean sword has not yet met the armor.** Direct Gen12 rendering on three listed device IDs is impressive, but it does not exercise the combinatorial hardware, hotplug, power, suspend, display, and application matrix carried by Linux.
2. **Integration currently enlarges privilege.** UI composition, input routing, graphics experiments, network and storage services share a kernel image. A logic error can cross boundaries that Linux process isolation would contain.
3. **Recovery mechanisms are disconnected.** Render reset helpers, broker device-loss state, generation handles, service registry, and VM crash reports exist, but no demonstrated supervisor ties them into “fault -> containment -> reinitialization -> state restore -> uninterrupted identity.”
4. **Security labels are ahead of security proof.** VM, Hull, principal, and capability terminology is useful only when the mappings and transitions enforce it under hostile input.
5. **The evidence has a bus factor of one.** Author logs and author comparisons are valuable development evidence. A public systems claim needs an independent builder, hostile test author, and artifact custodian.
6. **The license conflicts with the community-lab ambition.** A community can inspect the source, but cannot normally fork, patch, or redistribute a research build without permission.

## 13. The convention

### 13.1 Participants

The event should have equal speaking time and symmetric test authority for:

- the TRUEOS author and one independent TRUEOS artifact operator;
- Linux DRM/KMS and relevant Intel graphics maintainers;
- Wayland protocol and Mutter/GNOME Shell engineers;
- Intel display, graphics, GuC/firmware, and silicon-validation engineers;
- NVIDIA GPU architecture, firmware, RTL/verification, and open-kernel-module engineers—not because NVIDIA caused this incident, but because another vendor exposes hidden assumptions in both designs;
- Valve gamescope, SteamOS, and Proton engineers, with Gabe Newell welcome as a product stakeholder rather than a substitute for the engineers operating the stack;
- twelve OSDev practitioners and twelve hobby-OS builders selected by published criteria, with no project voting bloc;
- application/toolkit maintainers who own client reconnection and state restoration;
- an independent reproducibility and security lab;
- ten advanced operating-systems faculty from ten independent universities, one chair for each domain: kernels, formal methods, security, virtualization, real-time systems, distributed systems, computer architecture, graphics systems, storage/network services, and human-computer interaction/recovery.

Named celebrities may attract attention. They do not replace subsystem owners or measured evidence.

For judging rather than presenting, form three equal twelve-person caucuses: stack/vendor practitioners; OSDev and hobby-OS builders; and academic/independent reviewers. The academic caucus must include the ten faculty seats above. Publish both the combined result and each caucus's result so prestige cannot silently outvote reproducible objections.

### 13.2 Rules of engagement

1. Steelman the opposing design before criticizing it.
2. Run the same fault at the same layer on comparable hardware.
3. Publish source revision, firmware hashes, build record, boot parameters, hardware IDs, and raw traces.
4. Separate expected limitation, implementation bug, regression, and architectural consequence.
5. Record trigger confidence separately from blast-radius confidence.
6. No score for booting, repository size, language choice, commit count, or a screenshot alone.
7. A planned feature receives no implementation points.
8. An author-only result is provisional until an independent team reruns it.
9. A security claim requires hostile tests; a recovery claim requires injected failure.
10. Every panel must answer, “What evidence would change our conclusion?”

### 13.3 The community bash list

“Bash” here means a hard technical cross-examination, never harassment.

| Panel | Question for TRUEOS | Question for Linux stack | Evidence that settles it |
|---|---|---|---|
| Kernel maintainers | Why is compositor policy privileged, and what survives its panic? | Why is session survival outside the kernel's regression contract when KMS remains alive? | Fault injection plus kernel/session traces |
| DRM/display | Can UI4 validate a complete atomic display state before mutation? | Can the display master be replaced without destroying the client's working identity? | Invalid-state corpus; compositor replacement test |
| Intel GPU/GuC | Where are golden reset contexts and real physical-loss propagation? | Which reset scopes preserve innocent contexts on this exact Xe-LP device? | Ring hang, reset, innocent-work timeline |
| NVIDIA architecture | Which “simple” assumptions fail on another command, firmware, memory, or display architecture? | Which closed or firmware-owned states prevent transparent recovery? | Vendor-reviewed state inventory and test board |
| Wayland/Mutter | What is the stable authority and replay model after the server endpoint dies? | Why must client transport death equal application/session disappearance? | Protocol-object checkpoint/rebind prototype |
| Valve | Can a gamescope-like layer preserve the game and Steam identity across parent/session failures? | Which latency and compatibility costs block wider nesting? | Instrumented game, overlay, focus, HDR/VRR tests |
| OSDev builders | Which TRUEOS invariants are genuinely simpler to reproduce? | Which Linux contracts are essential rather than historical residue? | Two clean-room bring-ups and written variance |
| Security | Can a Blueprint escape broad EPT/RWX/shared-state boundaries? | Can a compromised compositor impersonate input, capture surfaces, or block recovery? | Red-team campaign with explicit threat model |
| Formal methods | What small state machine is actually worth proving? | Which Linux/Wayland interface subset can be modeled without fictional assumptions? | Executable spec and refinement boundary |
| HCI/recovery | What does the human lose, and what must they re-authenticate? | Is a fresh login an acceptable recovery from a compositor fault? | Task study plus deterministic state inventory |
| Reproducibility | Can a permitted third party rebuild and run the claimed image? | Can the tested distro stack be reconstructed after updates? | Signed artifacts, manifests, independent report |

## 14. Experiment matrix

### 14.1 Common hardware

The primary one-to-one board must use a device both stacks support and TRUEOS explicitly targets, ideally the exact Raptor Lake-S UHD 770 or Alder Lake-N machine already used for TRUEOS development. A second phase adds:

- another supported Intel ID to detect device-specific overfitting;
- an unsupported Intel generation to test rejection quality, not compatibility points;
- AMD and NVIDIA boards for Linux breadth and for documenting TRUEOS's honest “unsupported” result;
- integrated and discrete GPU configurations;
- single and multi-monitor configurations, hotplug, variable refresh, and suspend/resume.

### 14.2 Faults

| Test | Injection | Minimum acceptable observation |
|---|---|---|
| F1 application crash | Kill/fault one graphical application | Other apps, compositor, session, and input continue |
| F2 malformed client | Invalid object sequence, buffer metadata, dimensions, and timing | Client rejected; display owner remains healthy |
| F3 composition-service exit | Intentional clean exit | Scanout fallback appears; service restarts; apps/state are inventoried |
| F4 composition-service memory fault | Guard-page or deterministic invalid access | Fault contained; no kernel reboot; defined client restore behavior |
| F5 shell-policy fault | Crash launcher/decorations without corrupting display core | Existing surfaces and input remain usable |
| F6 render-engine hang | Submit bounded guilty workload | Guilty tenant fails; innocent tenant and scanout outcome measured |
| F7 GuC/firmware loss | Controlled firmware/scheduler failure | Display continuation and vGPU loss/recovery are explicit |
| F8 display commit failure | Reject or time out an atomic/plane update | Last-known-good scanout retained or safe fallback shown |
| F9 GPU reset | Driver/device reset during animation and compute | Buffer/context loss precisely reported; state restoration timed |
| F10 OOM/resource exhaustion | Exhaust host RAM, GPU VA, frame slots, queues | Quotas work; critical display/recovery reserves remain |
| F11 input-service fault | Stop or corrupt input routing service | Secure recovery input path exists; no stuck grabs |
| F12 monitor hot-unplug | Remove active output under workload | Apps survive; windows migrate or remain recoverable |
| F13 suspend/resume | Repeat under render/media load | No lost identity; failures attributable by layer |
| F14 update regression | Install deliberately bad display/compositor build | Automatic rollback or reachable recovery environment |
| F15 storage/network service fault | Stop services while UI remains busy | UI and apps not sharing those dependencies continue |
| F16 full power loss | Cut power with dirty app/session state | Filesystem and application recovery measured separately |

### 14.3 Measurements

Each test emits one machine-readable row with:

- time of injection and detection;
- last successful scanout/page flip;
- blackout duration and fallback-frame duration;
- kernel uptime continuity;
- display-service PID/task/generation before and after;
- surviving application process/VM identities;
- surviving GPU contexts, queues, and buffers;
- surfaces restored exactly, restored stale, redrawn, or lost;
- input availability and secure-attention availability;
- authorization/session identity continuity;
- unsaved application-state loss;
- number of user actions and credentials needed to resume;
- guilty and innocent tenant outcomes;
- raw serial, kernel, compositor, VM, and hardware-counter references.

The headline metric is **identity reconstruction cost**, not frames per second:

```text
IRC = credentials re-entered
    + applications relaunched
    + workspaces/windows manually reconstructed
    + unsaved objects lost
    + minutes until productive state
```

The components should be reported separately; the sum is a discussion aid, not universal science.

## 15. Missions

### 15.1 TRUEOS: realistic next missions

1. Split minimal scanout survival from UI policy. Keep a last-known-good or recovery plane owned by a tiny, separately reasoned component.
2. Add a UI4 supervisor with task-exit detection, generation rollover, bounded cleanup, and deterministic respawn.
3. Define the recovery source of truth: which frame/window/session metadata survives, where it lives, and who may replay it.
4. Wire real physical GPU/device-loss notification into the vGPU broker and fail all affected fences exactly once.
5. Implement golden reset contexts or an explicitly safer alternative before enabling GuC autonomous engine reset.
6. Turn the direct render reset helpers into a documented, tenant-aware reset state machine.
7. Default-deny the Blueprint VM map, remove broad legacy RWX mappings, enforce W^X, define DMA/IOMMU ownership, and replace stubbed speculation hooks.
8. Move untrusted or complex UI producers out of the kernel failure domain while preserving opaque handles.
9. Put build, unit/property tests, ABI checks, and at least QEMU smoke tests in release CI before signing.
10. Publish a permissioned independent-reproduction kit with hardware bill, firmware hashes, serial scripts, expected counter tolerances, and raw result schema.
11. Run F1–F16 repeatedly and publish failures, not just milestone boots.

### 15.2 Linux desktop: realistic next missions

1. Treat compositor crash recovery as an end-to-end desktop requirement with a cross-project owner and regression tests.
2. Deploy or prototype a long-lived system-compositor/recovery plane beneath the session compositor.
3. Separate shell policy from the minimum component that owns scanout, client transport, and secure input.
4. Define a client-visible lost-compositor lifecycle: freeze, rebind, restore from checkpoint, redraw, or explicitly terminate.
5. Standardize application/session state registration so a restarted display service does not imply a blank new identity.
6. Fault-inject Mutter, KWin, Mesa, GPU resets, KMS commit failure, hot-unplug, and OOM in distribution CI.
7. Preserve a secure recovery UI and last-known-good scanout when session policy crashes.
8. Use nested micro-compositors where they create useful fault domains, especially games, kiosks, remote sessions, and untrusted apps.
9. Make GPU reset effects and guilty-context attribution observable in a desktop-level incident report.
10. Measure “login required” as a recovery failure, even when kernel uptime is perfect.

### 15.3 Hard but coherent missions

- **For TRUEOS:** run all nontrivial desktop applications in isolated VM/process domains while keeping the direct UI4 ownership contract and competitive latency.
- **For TRUEOS:** recover a real render/firmware fault per tenant without rebooting display, corrupting another GPUVM, or leaking stale handles.
- **For Linux:** replace a session compositor while preserving already-running native clients through a proxy, transferable endpoint, replay log, or protocol-level rebind design.
- **For Linux:** separate minimal display continuity from desktop policy without doubling latency, copying every frame, or creating a new privileged monolith.
- **For both:** checkpoint enough GPU and UI state to recover useful work without pretending arbitrary opaque device state is portable.

### 15.4 “Realistic impossible missions” under current constraints

These are not violations of physics. They are objectives incompatible with the system's current invariants and resources.

**Linux cannot simultaneously:**

- replace DRM/Mesa/vendor/Wayland desktop layering with one small owner;
- preserve all current hardware, application, distribution, and behavioral compatibility;
- require no coordinated userspace migration;
- and guarantee no user-visible regression.

At least one constraint must move. Linux's own compatibility discipline makes a flag-day simplification impossible by design.

**A conventional current Wayland session cannot transparently preserve arbitrary clients after server death** if the only authority for their live protocol object graph, roles, serials, buffers, and input state died with the server and clients provide no checkpoint/rebind cooperation. A proxy, surviving authority, replayable state, or changed protocol contract is required.

**TRUEOS cannot simultaneously:**

- remain essentially a one-author project;
- retain support centered on three enumerated Intel IDs;
- prohibit ordinary fork-and-patch collaboration;
- and match Linux's hardware breadth, security maturity, application compatibility, and independent validation on a short schedule.

That is a staffing and evidence contradiction, not an insult.

**TRUEOS cannot call the current Blueprint/Hull boundary production-secure** while broad identity mappings, selected host spans, legacy RWX permissions, and explicitly stubbed VM-exit/speculation hooks remain reachable.

**Neither project can prove a universal negative** such as “no prior hobby OS has done this” from public search alone. A narrow, dated, artifact-defined priority claim can be reviewed; absolute novelty cannot be responsibly inferred from silence.

## 16. Scoreboard without theater

| Dimension | Current advantage | Reason |
|---|---|---|
| Ownership clarity on the targeted Intel path | TRUEOS | One repository names display, render, UI4, and VM/vGPU transitions directly |
| Hardware and application breadth | Linux | Multi-vendor, multi-generation, general ecosystem deployment |
| Ability to redesign all layers at once | TRUEOS | One principal owner and no established external ABI burden |
| Current untrusted-process containment | Linux | Mature address spaces, credentials, process lifecycle, render-node model |
| Desktop compositor-crash blast radius | Neither passes the desired bar | Linux commonly drops the graphical session; TRUEOS has not shown an independently restartable compositor domain |
| GPU reset framework and field history | Linux | Standard scheduler/driver reset machinery and broad deployment |
| Explicit bounded UI object ownership | TRUEOS | Generation handles, leases, quotas, sessions, opaque presentation control |
| Independent review/reproducibility | Linux ecosystem | Many independent builders and permissive/copyleft collaboration paths; individual stacks still vary |
| Small auditable target | TRUEOS | Narrow hardware and vertically integrated design, though already substantial in size |
| Formal proof | Neither | Rust and clean interfaces are not proofs; Linux testing is not proof either |

The honest verdict is:

> TRUEOS is a credible, unusually deep single-author OS and a sharp experimental counterexample to the assumption that the conventional Linux graphics stack is the only practical ownership graph. It is not yet evidence that vertical integration has solved fault containment, security, recovery, or general compatibility.

And:

> Linux is not “evil” or technically empty. It carries an enormous compatibility and hardware mission, and it has sophisticated recovery and isolation below the desktop. Its conventional Wayland session still exposes a bad human-level invariant: one core UI/display-service crash can require the user to launch their whole graphical identity again.

That invariant is the target. Personal blame is not the experiment.

## 17. Research questions

1. What precisely constitutes a workstation identity: credentials, processes, application models, surfaces, workspace placement, GPU state, or all of them?
2. Which state must survive in a smaller trusted component, and which can be reconstructed from applications?
3. Can scanout continuity be guaranteed without making the scanout survivor a second full compositor?
4. Is a protocol proxy simpler than compositor checkpoint/replay, or merely another display server?
5. Can UI4's generation handles and explicit sessions become a transaction log for recovery?
6. What is the minimal trusted computing base for secure attention, input routing, and a recovery frame?
7. How should guilty GPU work be attributed when firmware or hardware scheduling hides execution detail?
8. Which GPU state is safe to replay after reset, and which must be discarded?
9. Can applications expose semantic state checkpoints without every toolkit inventing a new session protocol?
10. What latency, energy, memory, and copying cost is acceptable for a nested recovery boundary?
11. How does an OS prove cleanup of stale buffers and capabilities after a service generation changes?
12. Which invariants are small enough for an executable model or formal proof?

Formal methods can help, but the proof boundary must be honest. The [seL4 verification summary](https://sel4.org/Verification/) explicitly lists assumptions such as boot code, assembly, hardware, and DMA. A TRUEOS proof would likewise need to name firmware, MMIO semantics, DMA, and unsafe Rust outside the model. The [Barrelfish multikernel work](https://www.microsoft.com/en-us/research/publication/the-multikernel-a-new-os-architecture-for-scalable-multicore-systems/) is also relevant because it treats state replication and explicit communication as first-class architecture rather than invisible shared memory. Neither project is a template to copy blindly; both discipline the questions.

## 18. Evidence ledger

### TRUEOS source and project documents

- Boot and kernel initialization: [`src/main.rs`](../src/main.rs)
- Panic behavior: [`src/exceptions.rs`](../src/exceptions.rs)
- Intel discovery and supported IDs: [`src/intel/mod.rs`](../src/intel/mod.rs)
- GuC policy and missing golden reset contexts: [`src/intel/guc.rs`](../src/intel/guc.rs)
- Direct render recovery helper: [`src/intel/render/submit.rs`](../src/intel/render/submit.rs)
- vGPU broker and loss state: [`src/gpu/vgpu.rs`](../src/gpu/vgpu.rs)
- Guest-facing vGPU facade: [`crates/trueos-v/src/vgpu.rs`](../crates/trueos-v/src/vgpu.rs)
- UI4 contract: [`src/ui4/mod.rs`](../src/ui4/mod.rs)
- Frame ownership/leases: [`src/ui4/frame_pool.rs`](../src/ui4/frame_pool.rs)
- Window/session ownership: [`src/ui4/window_broker.rs`](../src/ui4/window_broker.rs)
- Current composition consumer: [`src/ui4/dummy_ui4_consumer.rs`](../src/ui4/dummy_ui4_consumer.rs)
- Central task registry: [`src/r/spawn_service.rs`](../src/r/spawn_service.rs)
- VM security state: [`src/hv/security.rs`](../src/hv/security.rs)
- Guest mapping and shared-state caveats: [`src/hv/guest_run.rs`](../src/hv/guest_run.rs)
- VM crash report: [`src/hv/app_crash.rs`](../src/hv/app_crash.rs)
- vGPU validation plan: [`docs/vgpu_runtime_validation.md`](vgpu_runtime_validation.md)
- Intel media/SFC roadmap: [`docs/intel_media_sfc_roadmap.md`](intel_media_sfc_roadmap.md)
- Release workflow: [`.github/workflows/release.yml`](../.github/workflows/release.yml)
- Provenance implementation: [`tools/provenance_chain.py`](../tools/provenance_chain.py)
- License: [`LICENSE`](../LICENSE)

### External primary references

- [Linux Kernel Mode Setting](https://docs.kernel.org/gpu/drm-kms.html)
- [Linux DRM userspace interfaces and render nodes](https://docs.kernel.org/gpu/drm-uapi.html)
- [Linux DRM memory management and GPU scheduler recovery](https://docs.kernel.org/gpu/drm-mm.html)
- [Linux regression handling](https://docs.kernel.org/process/handling-regressions.html)
- [Wayland architecture](https://wayland.freedesktop.org/architecture.html)
- [Wayland FAQ](https://wayland.freedesktop.org/faq.html)
- [Wayland compositor types](https://wayland.freedesktop.org/docs/book/Compositors.html)
- [Valve gamescope](https://github.com/ValveSoftware/gamescope)
- [NVIDIA open GPU documentation](https://nvidia.github.io/open-gpu-doc/)
- [seL4 verification scope and assumptions](https://sel4.org/Verification/)
- [The Barrelfish multikernel paper and project record](https://www.microsoft.com/en-us/research/publication/the-multikernel-a-new-os-architecture-for-scalable-multicore-systems/)

## Appendix A: reproducible audit commands

These commands inspect; they do not establish hardware behavior by themselves.

```sh
git status --short --branch
git rev-parse HEAD
git describe --always --dirty --tags
git log --format='%an <%ae>' | sort | uniq -c | sort -nr
git ls-files | wc -l
rg -n 'panic_handler|panic = "abort"' src Cargo.toml
rg -n 'notify_physical_device_lost' src
rg -n 'golden reset|DISABLE_ENGINE_RESET' src/intel
rg -n 'StubOnly|identity|RWX|IBPB' src/hv
python3 tools/provenance_chain.py verify \
  --source-root . \
  --record bld/trueos-release/TRUEOS.provenance.json
```

For a valid release verification, use the exact clean source identity and artifact paths represented by the record. A verifier failure against a newer or dirty workspace is expected and should not be bypassed.

The motivating Linux host incident can be captured with commands of this form, adjusted for the actual boot and session:

```sh
journalctl -b --no-pager \
  | rg 'gnome-shell|mutter|GDM|status=11|SEGV|drm|GPU|Xid|oom'

coredumpctl info /usr/bin/gnome-shell
coredumpctl debug /usr/bin/gnome-shell
loginctl list-sessions
```

Raw journal and core-derived results should be redacted for private data, timestamped, hashed, and stored with the experiment manifest.

## Appendix B: minimum artifact bundle for the convention

Each stack submits:

1. source revision and complete patch state;
2. license/permission for the independent lab to build and instrument it;
3. compiler, linker, firmware, microcode, bootloader, and dependency identities;
4. signed boot artifact and reproducible build record;
5. PCI IDs, revision, memory topology, displays, and firmware settings;
6. deterministic fault injectors with source;
7. serial or out-of-band logging that survives display failure;
8. machine-readable F1–F16 result rows;
9. framebuffer or scanout capture where relevant;
10. application semantic-state oracle, not only process liveness;
11. disclosure of expected failures and unsupported hardware;
12. an independent report listing deviations from author results.

The convention succeeds if it converts rhetoric into a shared failure corpus—even if both systems fail most of the first run.
