# Extracted USB3 modules

This folder contains the USB3 implementation extracted from `src/usb3`.
`src/usb3/mod.rs` remains as the stable module entry point and connects the
live files here with explicit `#[path]` attributes. The relocation therefore
does not change module identities, public paths, controller behavior, device
ownership, HID/MIDI input, UAS storage, or diagnostic behavior.

The live extracted modules are:

- `api.rs`
- `class.rs`
- `descriptor.rs`
- `dev_gears.rs`
- `hid/`
- `lab.rs`
- `lib.rs`
- `skhynix.rs`

Some live compatibility and experimental surfaces are intentionally dormant.
Their `dead_code` diagnostics are scoped off at the module declarations in
`src/usb3/mod.rs`; the implementations remain compiled and available instead
of being deleted or rewritten.

`bot.rs` and `classreq.rs` were already disconnected legacy implementations
and remain here as source references.
