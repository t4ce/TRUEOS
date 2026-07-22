# Extracted USB3 modules

This folder contains the USB3 code extracted from `src/usb3` that is outside
the CrabUSB controller, Intel xHCI/root-hub discovery, device gears, and
SKHYNIX UAS path.

The live USB3 core is now limited to:

- `src/usb3/mod.rs`
- `src/usb3/lib.rs`
- `src/usb3/dev_gears.rs`
- `src/usb3/skhynix.rs`

`api.rs`, `class.rs`, `descriptor.rs`, and `hid/` remain connected with
explicit `#[path]` attributes because existing HID/input, MIDI, and USB
diagnostic consumers still use their public module identities. `bot.rs` and
`classreq.rs` were already disconnected legacy implementations and are kept
here only as source references.
