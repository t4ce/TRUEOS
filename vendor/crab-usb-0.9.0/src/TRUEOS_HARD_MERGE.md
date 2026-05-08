# TRUEOS hard-merge notes for crab-usb 0.9.0

This v9 vendor copy has the easy/medium TRUEOS carry-forward applied:

- TRUEOS/zkvm target gating is treated like the kernel target (`kmod`, `no_std`, no libusb).
- Public debug telemetry names from the old TRUEOS 0.6.2 vendor are present again.
- xHCI command, endpoint submit/completion, endpoint configuration, and probe progress now feed that telemetry.

The remaining migration work is deliberately not patched here because it needs TRUEOS-side policy decisions.

## 1. Topology and stable IDs

Old TRUEOS 0.6.2 carried a local `topology` module with route/path based handles and `DeviceInfo::stable_id()`.
Upstream 0.9.0 does not carry that public model. Treat this as hard merge, not a vendor mechanics patch.

Human decisions:

- Preserve the exact old stable ID format, or define a new TRUEOS-side ID derived from v9 probe data.
- Decide whether this belongs inside `crab-usb` again or in `src/usb2` as an OS inventory layer.
- Decide how hubs should participate in the ID path now that v9 exposes `ProbedDevice::Hub`.

Main TRUEOS users to port:

- `src/usb2/crabusb_service.rs`
- `src/usb2/device/pen.rs`
- `src/usb2/hid/boot.rs`
- `src/usb2/hid/mediacontrol.rs`
- `src/usb2/sound/mod.rs`
- `src/usb2/video/cam.rs`

## 2. Endpoint compatibility layer

Upstream 0.9.0 intentionally moved to unified `Endpoint`, `TransferRequest`, `RequestId`, and `TransferCompletion`.
Old TRUEOS code still expects typed endpoints:

- `EndpointBulkIn`
- `EndpointBulkOut`
- `EndpointInterruptIn`
- `EndpointIsoIn`
- `EndpointKind`
- `DetachedTransfer`

Human decisions:

- Build a short-lived compatibility shim inside the vendor crate, or port TRUEOS directly to v9's queue API.
- For mass storage and pen paths, decide whether detached polling should map directly to `RequestId`/`poll_request`.
- For video/audio, prefer direct v9 queue API if possible because v9 already contains newer ISO/CAM transfer work.

Main TRUEOS users to port:

- `src/usb2/api.rs`
- `src/usb2/mass.rs`
- `src/usb2/device/pen.rs`
- `src/usb2/video/cam.rs`
- `src/usb2/sound/mod.rs`

## 3. Probe inventory semantics

`USBHost::probe_devices()` now returns `Vec<ProbedDevice>` and can report hubs separately from leaf devices.
Old TRUEOS inventory paths expect device info with local topology extensions.

Human decisions:

- Should hubs appear in user-visible TRUEOS inventory/logs?
- Should class drivers see only `ProbedDevice::Device`, with hubs kept internal?
- Should opening a hub ever be supported, or always filtered?

Main TRUEOS user to port:

- `src/usb2/crabusb_service.rs`

## 4. Descriptor policy

TRUEOS already skips optional descriptor reads for specific QEMU HID devices on the OS side.
Do not accidentally move those optional reads down into the vendor crate during the v9 port.

Vendor v9 still performs mandatory enumeration reads: device descriptor base, device descriptor, configuration descriptor, and set configuration.
If a QEMU path needs fewer mandatory reads, that is a separate hard decision because it changes enumeration behavior, not just optional logging.

Main TRUEOS users to keep in mind:

- `src/usb2/descriptor.rs`
- `src/usb2/crabusb_service.rs`
- `src/usb2/hid/boot.rs`

## 5. xHCI recovery behavior

Old TRUEOS 0.6.2 had extra xHCI/hub recovery and lifecycle logging around slot cleanup, endpoint reset, hub rearm, stream diagnostics, and descriptor/config command phases.
The v9 copy keeps the medium telemetry surface, but does not yet reapply every old recovery behavior.

Human decisions:

- Which recovery paths are required for real hardware versus old investigation scaffolding?
- Should `debug_reset_endpoint` / `debug_close_slot` remain public vendor APIs?
- Should hub port rearm behavior be restored, or replaced by v9's current hub flow?

Main vendor areas if restored:

- `src/backend/kmod/kcore.rs`
- `src/backend/kmod/hub/*`
- `src/backend/kmod/xhci/device.rs`
- `src/backend/kmod/xhci/hub.rs`
- `src/backend/kmod/xhci/host.rs`
