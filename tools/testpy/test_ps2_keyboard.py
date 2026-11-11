#!/usr/bin/env python3
"""Exercise production PS/2 negotiation, decoding and shared HID keyboard sinks."""
from pathlib import Path
import json
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
use std::cell::RefCell;

#[path = @DECODER_PATH@] mod ps2_keyboard;
#[path = @CONTROLLER_PATH@] mod ps2_controller;
use ps2_keyboard::Decoder;

macro_rules! log { ($($argument:tt)*) => {}; }
pub(crate) use log;
pub(crate) use log as log_warn;
pub(crate) use log as log_info;
pub(crate) use log as log_important;

mod locale {
    pub fn current_keyboard_layout_name() -> &'static str { "de" }
}
mod allcaps {
    pub mod input { pub const HID_KEYBOARD_RING_CAP: usize = 64; }
}

#[derive(Clone, Debug, Default)]
struct Capture {
    boot: Vec<(u32, u32, u32, [u8; 8])>,
    legacy: Vec<usb2::hid::input::KeyboardEvent>,
    hut: Vec<HutCapture>,
}
#[derive(Clone, Debug)]
struct HutCapture {
    identity: (u32, u32, u32), modifiers: u8, keys: [u8; 6], ascii: [u8; 6],
    human: bool, source: String, virtual_device: bool,
}
thread_local! { static CAPTURE: RefCell<Capture> = RefCell::new(Capture::default()); }
fn capture<T>(f: impl FnOnce(&mut Capture) -> T) -> T {
    CAPTURE.with(|state| f(&mut state.borrow_mut()))
}

mod usb2 {
    pub mod hid {
        use std::cell::RefCell;
        const HID_DEBUG_REPORT_LOGS: bool = false;
        #[path = @HID_KEYBOARD_PATH@] pub mod keyboard;

        pub(crate) struct HidRuntime {
            controller_id: u32, slot_id: u32, ep_target: u32, seq: u64,
            last_nonzero_seq: u64, keyboard_modifiers: u8,
            keyboard_keys: [u8; 6], keyboard_ascii: [u8; 6],
            keyboard_ring: keyboard::KeyboardRing,
        }
        thread_local! {
            static RUNTIMES: RefCell<Vec<HidRuntime>> = const { RefCell::new(Vec::new()) };
        }

        // Capture the transport boundary, then use the real HID report handler.
        pub(crate) fn handle_keyboard_boot_report(
            controller_id: u32, slot_id: u32, ep_target: u32, data: &[u8],
        ) {
            let report: [u8; 8] = data.try_into().unwrap();
            crate::capture(|state| state.boot.push((controller_id, slot_id, ep_target, report)));
            RUNTIMES.with(|runtimes| {
                let mut runtimes = runtimes.borrow_mut();
                let index = match runtimes.iter().position(|runtime| {
                    (runtime.controller_id, runtime.slot_id, runtime.ep_target)
                        == (controller_id, slot_id, ep_target)
                }) {
                    Some(index) => index,
                    None => {
                        runtimes.push(HidRuntime {
                            controller_id, slot_id, ep_target, seq: 0, last_nonzero_seq: 0,
                            keyboard_modifiers: 0, keyboard_keys: [0; 6],
                            keyboard_ascii: [0; 6], keyboard_ring: keyboard::KeyboardRing::new(),
                        });
                        runtimes.len() - 1
                    }
                };
                let runtime = &mut runtimes[index];
                runtime.seq += 1;
                keyboard::handle_report(runtime, data, runtime.seq as u32);
            });
        }
        pub(crate) fn reset() { RUNTIMES.with(|state| state.borrow_mut().clear()); }
        pub(crate) fn remove_hid_slot(_: u32, _: u32) {
            panic!("startup admission tests must stop before HID device removal");
        }
        pub(crate) fn samples() -> Vec<keyboard::TrueosHidKeyboardSample> {
            RUNTIMES.with(|runtimes| {
                let mut samples = Vec::new();
                for runtime in runtimes.borrow_mut().iter_mut() {
                    while let Some(sample) = runtime.keyboard_ring.pop() { samples.push(sample); }
                }
                samples
            })
        }
        pub mod input {
            #[derive(Clone, Copy, Debug)]
            pub struct KeyboardEvent {
                pub slot_id: u32, pub modifiers: u8, pub keys: [u8; 6], pub ascii: [u8; 6],
            }
            pub enum InputEvent { Keyboard(KeyboardEvent) }
            pub fn push_event(event: InputEvent) {
                let InputEvent::Keyboard(keyboard) = event;
                crate::capture(|state| state.legacy.push(keyboard));
            }
        }
        pub mod hut {
            pub enum HidSourceKind { Human }
            pub fn upsert_keyboard_state(
                controller_id: u32, slot_id: u32, ep_target: u32, modifiers: u8,
                keys: [u8; 6], ascii: [u8; 6], source: HidSourceKind,
                source_tag: &str, virtual_device: bool,
            ) {
                crate::capture(|state| state.hut.push(crate::HutCapture {
                    identity: (controller_id, slot_id, ep_target), modifiers, keys, ascii,
                    human: matches!(source, HidSourceKind::Human), source: source_tag.into(),
                    virtual_device,
                }));
            }
        }
    }
}

mod r {
    pub mod keyboard {
        use std::cell::RefCell;

        // The only heapless operations used by production unique_boot_keys.
        struct Vec<T, const N: usize>(std::vec::Vec<T>);
        impl<T: PartialEq, const N: usize> Vec<T, N> {
            fn new() -> Self { Self(std::vec::Vec::new()) }
            fn contains(&self, value: &T) -> bool { self.0.contains(value) }
            fn push(&mut self, value: T) -> Result<(), T> {
                if self.0.len() == N { Err(value) } else { self.0.push(value); Ok(()) }
            }
        }
        impl<T, const N: usize> IntoIterator for Vec<T, N> {
            type Item = T;
            type IntoIter = std::vec::IntoIter<T>;
            fn into_iter(self) -> Self::IntoIter { self.0.into_iter() }
        }

        @PHYSICAL_CONSTANTS@
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default)]
        @OUTPUT_EVENT@
        @ZERO_EVENT@;
        #[derive(Clone, Copy, Debug)]
        @OUTPUT_RING@
        @OUTPUT_RING_IMPL@
        @PHYSICAL_HELPERS@

        struct State {
            previous: std::vec::Vec<((u32, u32, u32), (u8, [u8; 6]))>,
            ring: KeyboardOutputRing,
        }
        impl State {
            fn new() -> Self {
                Self { previous: std::vec::Vec::new(), ring: KeyboardOutputRing::new() }
            }
        }
        thread_local! { static STATE: RefCell<State> = RefCell::new(State::new()); }

        // The shared apply_report boundary: retain prior reports per identity
        // and exercise production physical transitions against the real ring.
        pub fn apply_report(
            controller: u32, slot: u32, endpoint: u32, time: u32, seq: u32,
            modifiers: u8, keys: [u8; 6], ascii: [u8; 6],
        ) {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                let identity = (controller, slot, endpoint);
                let index = state.previous.iter().position(|entry| entry.0 == identity);
                let (old_modifiers, old_keys) = index
                    .map(|index| state.previous[index].1).unwrap_or((0, [0; 6]));
                assert!(push_physical_report_into(
                    &mut state.ring, controller, slot, endpoint, time, seq,
                    old_modifiers, old_keys, modifiers, keys, ascii,
                ));
                if let Some(index) = index { state.previous[index].1 = (modifiers, keys); }
                else { state.previous.push((identity, (modifiers, keys))); }
            });
        }
        pub(crate) fn reset() { STATE.with(|state| *state.borrow_mut() = State::new()); }
        pub(crate) fn events() -> std::vec::Vec<TrueosKeyboardOutputEvent> {
            STATE.with(|state| {
                let state = state.borrow();
                assert!(state.ring.write_seq < MAX_KEYBOARD_OUTPUT_EVENTS as u64);
                state.ring.buf[..state.ring.write_seq as usize].to_vec()
            })
        }
    }
}

@IDENTITY_CONSTANTS@
@PUBLISH@

#[allow(unused_variables, unused_imports)]
mod startup_admission {
    use super::{Decoder, publish, CONTROLLER_ID, SLOT_ID, ENDPOINT};
    use std::cell::Cell;
    use core::future::Future;
    use core::task::{Context, Poll, Waker};

    @STARTUP_CONSTANTS@
    thread_local! {
        static FIRMWARE: Cell<Option<bool>> = const { Cell::new(None) };
        static INITIALIZE_CALLS: Cell<usize> = const { Cell::new(0) };
    }
    fn firmware_has_controller() -> Option<bool> { FIRMWARE.with(Cell::get) }
    struct Ports;
    impl Ports {
        fn status(&mut self) -> u8 { panic!("failed admission must not poll scan data"); }
        fn read_data(&mut self) -> u8 { panic!("failed admission must not read scan data"); }
    }
    struct Duration;
    impl Duration { fn from_millis(_: u64) -> Self { Self } }
    struct Timer;
    impl Timer { async fn after(_: Duration) {} }
    mod ps2_controller {
        pub(super) struct Error { stage: &'static str, reason: &'static str }
        pub(super) async fn initialize(_: &mut super::Ports) -> Result<(), Error> {
            super::INITIALIZE_CALLS.with(|calls| calls.set(calls.get() + 1));
            Err(Error { stage: "test probe", reason: "no keyboard admitted" })
        }
    }

    @KEYBOARD_TASK@

    fn complete(future: impl Future<Output = ()>) {
        let mut future = core::pin::pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(future.as_mut().poll(&mut context), Poll::Ready(())));
    }

    #[test]
    fn firmware_hints_never_replace_the_bounded_hardware_probe() {
        for hint in [Some(false), Some(true), None] {
            super::reset();
            FIRMWARE.with(|firmware| firmware.set(hint));
            INITIALIZE_CALLS.with(|calls| calls.set(0));
            complete(keyboard_task());
            INITIALIZE_CALLS.with(|calls| {
                assert_eq!(calls.get(), 1, "firmware hint {hint:?} bypassed the hardware probe");
            });
            super::capture(|state| {
                assert!(state.boot.is_empty(), "failed probe enrolled a HID keyboard");
                assert!(state.hut.is_empty());
                assert!(state.legacy.is_empty());
            });
            assert!(crate::r::keyboard::events().is_empty());
        }
    }
}

fn reset() {
    capture(|state| *state = Capture::default());
    usb2::hid::reset();
    r::keyboard::reset();
}
fn feed(decoder: &mut Decoder, bytes: &[u8]) {
    for &byte in bytes {
        if decoder.feed(byte) { publish(decoder); }
        if decoder.finish_momentary() { publish(decoder); }
    }
}

#[test]
fn ps2_registers_as_a_physical_hid_keyboard_in_a_separate_identity_namespace() {
    reset();
    publish(&Decoder::new());
    capture(|state| {
        assert_eq!(state.boot, [(0x4938_3034, 0x4938_0001, 0, [0; 8])]);
        assert_eq!(state.legacy.len(), 1);
        assert_eq!(state.legacy[0].slot_id, 0x4938_0001);
        assert_eq!(state.hut.len(), 1);
        let hut = &state.hut[0];
        assert_eq!(hut.identity, (0x4938_3034, 0x4938_0001, 0));
        assert!(hut.human && !hut.virtual_device);
        assert_eq!(hut.source, "human");
    });
    assert!(r::keyboard::events().is_empty());
}

#[test]
fn shift_and_a_reach_sample_legacy_hut_and_physical_rings_without_typematic_duplicates() {
    reset();
    let mut decoder = Decoder::new();
    publish(&decoder);
    feed(&mut decoder, &[0x12, 0x1C, 0x1C, 0xF0, 0x1C, 0xF0, 0x12]);
    capture(|state| {
        assert_eq!(state.boot.len(), 5);
        assert_eq!(state.boot[2].3, [2, 0, 4, 0, 0, 0, 0, 0]);
        assert_eq!(state.boot[4].3, [0; 8]);
        assert_eq!(state.legacy.len(), 5);
        assert_eq!(state.legacy[2].ascii, [b'A', 0, 0, 0, 0, 0]);
        assert_eq!(state.hut.len(), 5);
        assert_eq!(state.hut[2].modifiers, 2);
        assert_eq!(state.hut[2].keys[0], 4);
        assert_eq!(state.hut[2].ascii[0], b'A');
    });
    let samples = usb2::hid::samples();
    assert_eq!(samples.len(), 5);
    assert_eq!((samples[2].slot_id, samples[2].keys[0], samples[2].ascii[0]),
        (0x4938_0001, 4, b'A'));
    let events = r::keyboard::events();
    assert_eq!(events.iter().map(|event| (event.key_code, event.flags)).collect::<Vec<_>>(),
        [(0xE1, 1), (4, 1), (4, 0), (0xE1, 0)]);
    assert_eq!(events[1].codepoint, u32::from(b'A'));
    assert!(events.iter().all(|event| {
        (event.controller_id, event.slot_id, event.ep_target) == (CONTROLLER_ID, SLOT_ID, ENDPOINT)
            && event.kind == 4
    }));
}

#[test]
fn pause_publishes_a_make_and_release_even_without_a_following_scan_byte() {
    reset();
    let mut decoder = Decoder::new();
    feed(&mut decoder, &[0xE1, 0x14, 0x77, 0xE1, 0xF0, 0x14, 0xF0, 0x77]);
    capture(|state| {
        assert_eq!(state.boot.len(), 2);
        assert_eq!(state.boot[0].3, [0, 0, 0x48, 0, 0, 0, 0, 0]);
        assert_eq!(state.boot[1].3, [0; 8]);
    });
    assert_eq!(r::keyboard::events().iter().map(|event| (event.key_code, event.flags))
        .collect::<Vec<_>>(), [(0x48, 1), (0x48, 0)]);
}

#[test]
fn ps2_releases_do_not_release_an_independent_usb_keyboard() {
    reset();
    usb2::hid::handle_keyboard_boot_report(4, 1, 3, &[0, 0, 5, 0, 0, 0, 0, 0]);
    let mut decoder = Decoder::new();
    feed(&mut decoder, &[0x1C, 0xF0, 0x1C]);
    usb2::hid::handle_keyboard_boot_report(4, 1, 3, &[0; 8]);
    let events = r::keyboard::events();
    assert_eq!(events.iter().map(|event| (event.controller_id, event.slot_id,
        event.key_code, event.flags)).collect::<Vec<_>>(), [
        (4, 1, 5, 1), (CONTROLLER_ID, SLOT_ID, 4, 1),
        (CONTROLLER_ID, SLOT_ID, 4, 0), (4, 1, 5, 0),
    ]);
}

#[test]
fn ps2_physical_positions_use_the_existing_locale_conversion() {
    reset();
    let mut decoder = Decoder::new();
    feed(&mut decoder, &[0x35]); // Physical Y position; German layout is Z.
    capture(|state| {
        assert_eq!(state.legacy[0].keys[0], 0x1C);
        assert_eq!(state.legacy[0].ascii[0], b'z');
    });
    assert_eq!(r::keyboard::events()[0].codepoint, u32::from(b'z'));
}

#[test]
fn clearing_a_damaged_scan_stream_releases_the_shared_physical_state() {
    reset();
    let mut decoder = Decoder::new();
    feed(&mut decoder, &[0x14, 0x1C, 0xE0, 0xF0]);
    assert!(decoder.clear());
    publish(&decoder);
    let events = r::keyboard::events();
    assert_eq!(events.iter().map(|event| (event.key_code, event.flags))
        .collect::<Vec<_>>(), [(0xE0, 1), (4, 1), (4, 0), (0xE0, 0)]);
}
'''


def constants(source, names):
    return "\n".join(
        re.search(rf"(?:pub )?const {name}:.*?;", source)[0]
        for name in names
    )


def main():
    ps2 = (ROOT / "src/r/io/ps2.rs").read_text()
    physical = (ROOT / "src/r/keyboard.rs").read_text()
    harness = HARNESS
    for marker, path in [
        ("@DECODER_PATH@", "src/r/io/ps2_keyboard.rs"),
        ("@CONTROLLER_PATH@", "src/r/io/ps2_controller.rs"),
        ("@HID_KEYBOARD_PATH@", "src/usb3/hid/keyboard.rs"),
    ]:
        harness = harness.replace(marker, json.dumps(str(ROOT / path)))
    replacements = {
        "@IDENTITY_CONSTANTS@": constants(ps2, ["CONTROLLER_ID", "SLOT_ID", "ENDPOINT"]),
        "@PUBLISH@": item(ps2, "fn publish("),
        "@STARTUP_CONSTANTS@": constants(ps2, [
            "OUTPUT_FULL", "AUX_DATA", "RECEIVE_ERROR", "POLL_MS", "DRAIN_LIMIT",
        ]),
        "@KEYBOARD_TASK@": item(ps2, "pub(crate) async fn keyboard_task("),
        "@PHYSICAL_CONSTANTS@": constants(physical, [
            "MAX_KEYBOARD_OUTPUT_EVENTS", "KEYBOARD_OUTPUT_FLAG_PRESS", "KEYBOARD_OUTPUT_KIND_PHYSICAL",
        ]),
        "@OUTPUT_EVENT@": item(physical, "pub struct TrueosKeyboardOutputEvent"),
        "@ZERO_EVENT@": item(physical, "const ZERO_KEYBOARD_OUTPUT_EVENT:"),
        "@OUTPUT_RING@": item(physical, "struct KeyboardOutputRing"),
        "@OUTPUT_RING_IMPL@": item(physical, "impl KeyboardOutputRing"),
        "@PHYSICAL_HELPERS@": "\n".join(item(physical, signature) for signature in [
            "fn physical_key_event(", "fn key_is_valid_boot_usage(",
            "fn push_physical_report_into(", "fn unique_boot_keys(", "fn key_is_down(",
        ]),
    }
    for marker, value in replacements.items():
        harness = harness.replace(marker, value)
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-ps2-keyboard-") as directory:
        directory = Path(directory)
        source = directory / "tests.rs"
        source.write_text(harness)
        executable = directory / "tests"
        subprocess.run([
            "rustc", "+" + channel, "--edition=2024", "--test", str(source), "-o", str(executable),
        ], check=True, cwd=directory)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
