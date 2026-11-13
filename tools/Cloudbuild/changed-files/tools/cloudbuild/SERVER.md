# TRUEOS Server: sh3 and services without graphics

**Configuration target:** a regular Server preset that boots with no monitor,
framebuffer, iGPU, or shader execution, and excludes the desktop graphics
implementation from compilation. It keeps sh3, selected network services,
storage, and the kernel facilities those services need.

**Current status:** the branch has useful display-independent boot and remote
terminal paths, but it does not yet implement this stripped Server preset.
Disabling the existing iGPU/Virgl switches stops device initialization; it
does not exclude UI4, graphics modules, or every GPU asset from the build.
This document records the configuration boundary and the remaining work,
based on source inspection at `9c4f452`. It is not a server image or a report
of successful boot on a blade.

## Server and Desktop from the same kernel

Server should be a top-level preset over the same feature catalog and build
pipeline as Desktop. The preset must resolve to the actual compiled
capabilities. A separate source fork is unnecessary.

| Component | Server configuration target | Desktop configuration target |
| --- | --- | --- |
| CPU, memory, SMP, timers, interrupts, PCI | Keep | Keep |
| Storage and filesystem services | Keep selected hardware/services | Keep selected hardware/services |
| Networking and cryptography | Keep selected drivers/services | Keep selected drivers/services |
| sh3 command/session model and text terminal | Keep | Keep |
| Incoming SSH administration | Keep with provisioned access policy | Independently selectable |
| Blueprint runtime and app database | Keep for selected server workloads; database may start empty | Keep |
| UI4 windows, compositor, graphical input and presentation | Exclude | Include when selected |
| Intel graphics display/render/copy/media engines and GPU submission | Exclude | Include when selected |
| Virgl/virtio GPU rendering | Exclude | Include when selected |
| GPU shaders and GPU-specific firmware/assets | Exclude | Include for selected graphics backends |
| H.264 display streaming and native GPGPU | Exclude | Independently selectable with real dependencies |
| Bundled apps | Zero or an explicitly selected compatible set | Any explicitly selected compatible set |
| USB | Independent choice for the actual boot/storage/network hardware | Independent choice |

Intel CPU, VMX, PCI, storage, and network support are separate from the Intel
graphics driver. Removing that driver must preserve the CPU and device
facilities used by an Intel server.

The architecture should also allow a future server with GPU compute and no
desktop. That is a separate combination from the GPU-free Server target here;
presentation and compute should not acquire an artificial dependency on each
other merely because the initial presets are Server and Desktop.

## What already supports running without a display

- The native UEFI loader continues when graphics initialization fails, keeps
  `DISPLAY` optional, and supplies an optional framebuffer in the handoff.
  A missing GOP or monitor is not itself a fatal loader condition. See
  [`efi_main`](../trueos-boot/src/main.rs) and
  [`BootFacts`](../trueos-boot/src/handoff.rs).
- The kernel's Intel initialization returns when the feature is disabled or
  no supported Intel display device is found. That path is in
  [`intel::init_once`](../../src/intel/mod.rs).
- The incoming port-22 sh3 listener uses the kernel SSH implementation and
  constructs a terminal with `ShowBackend::Network`. Its startup gate is
  network readiness, not compositor readiness. See
  [`shell3/net.rs`](../../src/shell3/net.rs),
  [`Shell3::new_terminal_sized_reserved`](../../src/shell3/shell3.rs), and
  the [service registry](../../src/r/services/spawn_service.rs).
- The sh3 execution pool is admitted by CPU executor/topology availability.
  The HTTP filesystem service also uses network readiness. Blueprint
  autostart's registry gate waits for background workers and VM-thread
  readiness, not UI4. Its [cold-start task](../../src/r/restart.rs) also waits
  for a mounted TRUEOSFS root before loading and launching the startup
  configuration. Incoming SSH does not depend on that storage step.
- sh3 and incoming SSH are kernel services, independent of the selectable
  `ssh` app. That buildin is the **outgoing client**. A zero-buildin image does
  not remove the incoming shell service simply because the client app was
  omitted.

These are source-level properties. Actual GPU-less boot, remote login, and
service operation still need validation on the intended machine and a test
machine with genuinely no graphics device. Running with a hidden display
window does not establish that absence.

## Why disabling the existing graphics switches is insufficient

[`src/main.rs`](../../src/main.rs) still declares `ui4`, `intel`, `gpu`,
`graphics`, and `virtio_gpu_logo` unconditionally. The current
[`Cargo.toml`](../../Cargo.toml) documents iGPU/Virgl as device admission
switches. Compiler optimization may discard some unreachable code, but this
is not an explicit, verified exclusion of those subsystems.

The remote shell has a useful text backend, but its code still shares types
and modules with graphical presentation:

- [`shell3/show/show.rs`](../../src/shell3/show/show.rs) imports UI4 window,
  frame, raster, and presentation types alongside the network backend.
- [`shell3/service.rs`](../../src/shell3/service.rs) contains graphical input
  queues and window handling alongside terminal worker management.
- [`shell3/tui.rs`](../../src/shell3/tui.rs) mixes terminal routes with UI4
  windows, drag/drop, and presentation notifications.
- sh3's GPU/font commands and several kernel service registrations directly
  reference the graphics implementation.

The generated Blueprint C-ABI resolver is another real boundary:
[`build.rs`](../../build.rs) scans source files for exports and emits paths to
them. Removing modules without making this generation aware of the compiled
capabilities would leave references to excluded implementations.

Packaging also needs a change. The UEFI loader treats missing GuC firmware
as nonfatal, but the preparation script, [bundle writer](../trueos-boot/bundle.py),
and [update-bundle validator](../../src/update_bundle.rs) currently include or
require `EFI/BOOT/tgl_guc_70.bin`. A server archive without GPU firmware must
be supported consistently by preparation, installation, and update validation.
Dropping the file in only one place would produce an incompatible archive.

## Implementation boundary

1. **Separate sh3 sessions from their frontend.** Keep command evaluation,
   terminal cells, SSH transport, session ownership, and server-side app
   launch independent of UI4. Compile windows, fonts/rasterization, pointer
   input, and graphical diagnostics only with the presentation feature.
   Preserve the CPU service executors even if their current names mention UI.
2. **Make graphics a real compilation choice.** Gate module declarations,
   implementation dependencies, shader payloads, graphic assets, and service
   registrations. The Server selection must compile with the excluded
   implementation unavailable, rather than depending on early returns from
   its initialization functions.
3. **Describe the compiled capability set at API boundaries.** CPU, filesystem,
   networking, and terminal operations remain available. Required graphics
   imports must fail clearly during capability/prebind validation when absent;
   optional capability queries may report unsupported. Preserve published
   ABI signatures, and do not return fabricated success from graphics stubs.
4. **Select server services independently of apps.** Keeping a kernel shell
   does not imply bundling a desktop, store, client, or explorer. A service
   preset should not automatically launch desktop/media/input services.
   Add actual dependencies to the catalog and check only selected apps.
5. **Align boot, installation, and updates with the profile.** Keep the
   no-framebuffer handoff valid, omit unused GPU firmware from server
   packaging, and make archive validation understand the resolved
   configuration. Record those choices in build provenance.
6. **Expose Server once the contract is implemented and tested.** Resolve it
   through the same manifest, signature, queue, worker, and receipt flow as
   other presets. Zero buildins remains a regular independent option.

## Hardware and administration details to settle

The current [network device registry](../../src/net/mod.rs) includes specific
virtio, Intel e1000/PCH, Realtek, USB, and Wi-Fi paths. The
[Intel e1000 table](../../src/net/e1000.rs) is a concrete device-ID list, not a
promise to support every Intel server NIC. A blade's actual network and
storage devices must match working drivers; no-display boot alone does not
establish that compatibility.

The loader emits bounded COM1 boot diagnostics. The kernel's ongoing UART
logging is currently enabled through emulator detection, so it is not yet a
general physical-server serial logging configuration. There is also no
interactive sh3 serial frontend. Network administration, runtime serial logs,
and serial recovery are separate capabilities to validate.

The current [SSH boot policy](../../src/shell3/ssh_boot.rs) is explicitly for
development and allows SSH `none` authentication. A server release needs an
explicit administrator authentication policy and host-identity provisioning.
License proof and permission to administer a running server are separate
decisions; uploading a licensing key must not silently grant remote login.

The current [SSH session implementation](../../src/shell3/ssh.rs) supports an
interactive PTY shell. It rejects exec, subsystem, and environment requests;
`ssh host command` and SFTP therefore are not provided by this transport yet.
Choose and validate any additional remote-administration interfaces as part
of the server service contract.

## Validation and size measurements

Acceptance requires both build evidence and runtime evidence:

- Build Server with UI4 and GPU implementations excluded, and inspect the
  dependency/link output and packaged assets for the expected omissions.
- Boot without a graphics device, GOP framebuffer, monitor, or GPU firmware;
  verify remote sh3 and the selected services independently of any UI gate.
- Exercise zero apps and a small compatible app set, plus cold boot, restart,
  and update behavior. Keep the normal Desktop build working.
- Test the actual blade NIC/storage and the intended administrator login and
  recovery path.

Measure kernel ELF, compressed download, and runtime memory separately. Use
matched source/compiler/dependency inputs and the same app selection for
Server-versus-Desktop comparisons. The existing **20,388 kB** derived
kernel/assets reference still comes from the full graphics-capable release;
it is not a measured Server baseline. Keep graphics-removal savings unmeasured
until paired builds supply them, as described in
[Build size estimates](SIZE_ESTIMATES.md).
