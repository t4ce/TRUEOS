#!/usr/bin/env python3
"""Compile and test the production typed input wire codecs against ABI records."""

from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(path: str, name: str) -> str:
    source = (ROOT / path).read_text()
    matches = list(re.finditer(rf"^(?:pub(?:\([^)]*\))?\s+)?struct\s+{name}\b", source, re.MULTILINE))
    if len(matches) != 1:
        raise ValueError(f"{path}: expected one {name}, found {len(matches)}")
    start = matches[0].start()
    attributes = re.search(r"(?:^#[^\n]*\n)+\Z", source[:start], re.MULTILINE)
    if attributes:
        start = attributes.start()
    ending = re.search(r"^}\s*\n", source[matches[0].end():], re.MULTILINE)
    if not ending:
        raise ValueError(f"{path}: no closing brace for {name}")
    return source[start:matches[0].end() + ending.end()]


def function(name: str, path: str = "src/r/io/input_cabi.rs") -> str:
    source = (ROOT / path).read_text()
    start = re.search(rf'^(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(?:extern "C"\s+)?fn {name}\b', source, re.MULTILINE)
    if start is None:
        raise ValueError(f"missing function {name}")
    opening = source.index("{", start.start())
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start.start():end]


def main() -> None:
    records = {
        "TrueosHidCursorEvent": item("crates/trueos-v/src/bp_abi.rs", "TrueosHidCursorEvent"),
        "MouseMotionCommand": item("crates/trueos-v/src/bp_abi.rs", "MouseMotionCommand"),
        "KeyboardControlCommand": item("crates/trueos-v/src/bp_abi.rs", "KeyboardControlCommand"),
        "TrueosKeyboardOutputEvent": item("src/r/keyboard.rs", "TrueosKeyboardOutputEvent"),
        "MidiInputEventV1": item("src/usb3/hid/midi.rs", "MidiInputEventV1"),
        "TrueosHidHutKeyboardState": item("src/usb3/hid/hut.rs", "TrueosHidHutKeyboardState").replace(
            "#[derive(Copy, Clone, Debug)]", "#[derive(Copy, Clone, Debug, Default)]"
        ),
        "TrueosInputCombo": item("crates/trueos-v/src/bp_abi.rs", "TrueosInputCombo"),
    }
    bridge = "\n".join(function(name) for name in [
        "host_input_hut_snapshot_v1", "guest_input_hut_snapshot_v1",
        "guest_hid_hut_read_keyboards", "guest_input_combo_read", "trueos_cabi_input_combo_read",
    ])
    keyboard_cabi = function("trueos_cabi_hid_hut_read_keyboards", "src/usb3/hid/mod.rs")
    harness = f'''#![allow(dead_code)]
extern crate alloc;
extern crate self as v;
extern crate self as trueos_vm;
pub mod vinput {{ {records["TrueosHidCursorEvent"]}
{records["MouseMotionCommand"]}
{records["KeyboardControlCommand"]}
{records["TrueosInputCombo"]} }}
mod usb2 {{ pub mod hid {{ pub use crate::vinput::TrueosHidCursorEvent; pub use crate::usb3::hid::hut; }} }}
mod r {{ pub mod keyboard {{ {records["TrueosKeyboardOutputEvent"]} }}
pub mod io {{ pub mod input_cabi {{ pub(crate) use crate::guest_hid_hut_read_keyboards; }} }} }}
mod usb3 {{ pub mod hid {{
pub mod midi {{ {records["MidiInputEventV1"]} }}
pub mod hut {{
const HID_HUT_SOURCE_TAG_MAX: usize = 32;
std::thread_local! {{ pub static KEYBOARD_COUNT: std::cell::Cell<u32> = const {{ std::cell::Cell::new(32) }}; }}
{records["TrueosHidHutKeyboardState"]}
pub fn keyboards_snapshot() -> Vec<TrueosHidHutKeyboardState> {{
    (1..=KEYBOARD_COUNT.with(|count| count.get())).map(|slot_id| TrueosHidHutKeyboardState {{ slot_id, combo_id: slot_id, virtual_device: 1, ..Default::default() }}).collect()
}}
pub fn read_keyboards_snapshot(out: &mut [TrueosHidHutKeyboardState]) -> usize {{
    let snapshot = keyboards_snapshot();
    let count = out.len().min(snapshot.len());
    out[..count].copy_from_slice(&snapshot[..count]); count
}}
pub fn combos_snapshot() -> Vec<crate::vinput::TrueosInputCombo> {{
    (1..=32).map(|combo_id| crate::vinput::TrueosInputCombo {{ combo_id, color_id: (combo_id % 16) as u8, ..Default::default() }}).collect()
}}
}}
{keyboard_cabi}
}} }}
#[path = "{ROOT / 'src/r/io/input_wire.rs'}"] mod input_wire;
{bridge}
fn input_combo_info(combo: &vinput::TrueosInputCombo) -> vinput::TrueosInputCombo {{ *combo }}
'''
    harness += r'''
mod hv { pub fn current_hull_guest_context_vm_id() -> Option<u8> { Some(0) } }
pub mod vmcall {
    pub const STATUS_OK: u32 = 0;
    pub const OP_BP_INPUT_HUT_SNAPSHOT_V1: u32 = 0x257;
    std::thread_local! { static RESPONSE: std::cell::RefCell<(u32, u64, Vec<u8>, usize)> = Default::default(); }
    std::thread_local! { pub static SERVE_HOST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    std::thread_local! { pub static CALLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
    pub fn set(status: u32, count: u64, payload: Vec<u8>, reported_len: usize) {
        RESPONSE.with(|value| *value.borrow_mut() = (status, count, payload, reported_len));
    }
    pub fn call_with_payload_reported_len(op: u32, kind: u64, cap: u64, req: &[u8], out: &mut [u8]) -> (u32, u64, usize) {
        assert_eq!(op, OP_BP_INPUT_HUT_SNAPSHOT_V1);
        assert!(kind == 1 || kind == 2);
        assert!(cap <= 32);
        assert!(req.is_empty());
        CALLS.with(|value| value.set(value.get() + 1));
        if SERVE_HOST.with(|value| value.get()) {
            let (count, len) = crate::host_input_hut_snapshot_v1(kind as u32, cap as u32, out).unwrap();
            return (STATUS_OK, count as u64, len);
        }
        RESPONSE.with(|value| {
            let (status, count, payload, reported_len) = &*value.borrow();
            let len = out.len().min(payload.len());
            out[..len].copy_from_slice(&payload[..len]);
            (*status, *count, *reported_len)
        })
    }
}

#[test]
fn public_guest_cabi_reads_follow_changes_in_host_hut_registry() {
    vmcall::SERVE_HOST.with(|value| value.set(true));
    vmcall::CALLS.with(|value| value.set(0));
    let mut calls = 0;
    for count in [0, 2, 32, 1, 0] {
        usb3::hid::hut::KEYBOARD_COUNT.with(|value| value.set(count));
        assert_eq!(unsafe { usb3::hid::trueos_cabi_hid_hut_read_keyboards(std::ptr::null_mut(), 0) }, count);
        calls += 1;
        assert_eq!(vmcall::CALLS.with(|value| value.get()), calls, "count probe must cross VM boundary");
        let mut keyboards = vec![usb3::hid::hut::TrueosHidHutKeyboardState::default(); count as usize];
        if count != 0 {
            assert_eq!(unsafe { usb3::hid::trueos_cabi_hid_hut_read_keyboards(keyboards.as_mut_ptr(), count) }, count);
            calls += 1;
            assert_eq!(vmcall::CALLS.with(|value| value.get()), calls, "snapshot read must cross VM boundary");
            assert_eq!(keyboards[count as usize - 1].slot_id, count);
        }
    }
    assert_eq!(unsafe { trueos_cabi_input_combo_read(std::ptr::null_mut(), 0) }, 32);
    calls += 1;
    assert_eq!(vmcall::CALLS.with(|value| value.get()), calls);
    let mut combos = vec![vinput::TrueosInputCombo::default(); 32];
    assert_eq!(unsafe { trueos_cabi_input_combo_read(combos.as_mut_ptr(), 32) }, 32);
    calls += 1;
    assert_eq!(vmcall::CALLS.with(|value| value.get()), calls);
    assert_eq!(combos[31].combo_id, 32);
    assert_eq!(combos[31].color_id, 0);
    vmcall::SERVE_HOST.with(|value| value.set(false));
}

#[test]
fn host_snapshots_keep_zero_probe_output_untouched_and_admit_all_32_records() {
    for (kind, size) in [(1, 96), (2, 88)] {
        let mut bytes = vec![0xa5; 4040];
        assert_eq!(host_input_hut_snapshot_v1(kind, 0, &mut bytes), Ok((32, 0)));
        assert!(bytes.iter().all(|byte| *byte == 0xa5));
        assert_eq!(host_input_hut_snapshot_v1(kind, 32, &mut bytes), Ok((32, 32 * size)));
        assert!(bytes[32 * size..].iter().all(|byte| *byte == 0xa5));
        for (index, record) in bytes[..32 * size].chunks_exact(size).enumerate() {
            let id = if kind == 1 { input_wire::decode_hut_keyboard(record).unwrap().slot_id }
                     else { input_wire::decode_input_combo(record).unwrap().combo_id };
            assert_eq!(id, index as u32 + 1);
        }
        assert_eq!(host_input_hut_snapshot_v1(kind, 1, &mut bytes), Ok((1, size)));
        let mut short = vec![0xa5; size - 1];
        assert_eq!(host_input_hut_snapshot_v1(kind, 32, &mut short), Ok((0, 0)));
        assert!(short.iter().all(|byte| *byte == 0xa5));
        assert_eq!(host_input_hut_snapshot_v1(kind, 33, &mut bytes), Err(-22));
    }
    assert_eq!(host_input_hut_snapshot_v1(0, 0, &mut []), Err(-22));
}

#[test]
fn guest_reads_all_32_records_and_count_probes_without_writing() {
    for (kind, size) in [(1, 96), (2, 88)] {
        let mut bytes = vec![0; 32 * size];
        let (count, len) = host_input_hut_snapshot_v1(kind, 32, &mut bytes).unwrap();
        vmcall::set(0, count as u64, bytes, len);
        if kind == 1 {
            let mut out = vec![usb3::hid::hut::TrueosHidHutKeyboardState::default(); 32];
            assert_eq!(unsafe { guest_hid_hut_read_keyboards(out.as_mut_ptr(), u32::MAX) }, 32);
            assert_eq!(out[31].slot_id, 32);
            assert_eq!(out[31].virtual_device, 1);
        } else {
            let mut out = vec![vinput::TrueosInputCombo::default(); 32];
            assert_eq!(unsafe { guest_input_combo_read(out.as_mut_ptr(), u32::MAX) }, 32);
            assert_eq!(out[31].combo_id, 32);
        }
    }
    vmcall::set(0, 32, vec![], 0);
    let mut keyboard = usb3::hid::hut::TrueosHidHutKeyboardState { slot_id: 99, ..Default::default() };
    assert_eq!(unsafe { guest_hid_hut_read_keyboards(&mut keyboard, 0) }, 32);
    assert_eq!(keyboard.slot_id, 99);
    assert_eq!(unsafe { guest_hid_hut_read_keyboards(std::ptr::null_mut(), 0) }, 32);
    let mut combo = vinput::TrueosInputCombo { combo_id: 99, ..Default::default() };
    assert_eq!(unsafe { guest_input_combo_read(&mut combo, 0) }, 32);
    assert_eq!(combo.combo_id, 99);
    assert_eq!(unsafe { guest_input_combo_read(std::ptr::null_mut(), 0) }, 32);
}

#[test]
fn malformed_guest_responses_never_write_caller_records() {
    for (status, count, reported_len) in [(1, 1, 96), (0, 1, 95), (0, 1, 97), (0, 2, 192), (0, 33, 3168), (0, u64::MAX, 96), (0, 1, 4041)] {
        vmcall::set(status, count, vec![0; 4040], reported_len);
        let mut keyboard = usb3::hid::hut::TrueosHidHutKeyboardState { slot_id: 99, virtual_device: 1, ..Default::default() };
        assert_eq!(unsafe { guest_hid_hut_read_keyboards(&mut keyboard, 1) }, 0);
        assert_eq!(keyboard.slot_id, 99);
        assert_eq!(keyboard.virtual_device, 1);
    }
    vmcall::set(0, 1, vec![0; 88], 88);
    let mut combo = vinput::TrueosInputCombo { combo_id: 99, ..Default::default() };
    assert_eq!(unsafe { guest_input_combo_read(&mut combo, 0) }, 0);
    assert_eq!(combo.combo_id, 99);
    assert_eq!(unsafe { guest_input_combo_read(std::ptr::null_mut(), 1) }, 0);
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-input-wire-") as temp:
        source = Path(temp) / "harness.rs"
        binary = Path(temp) / "tests"
        source.write_text(harness)
        build = subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)],
                               text=True, capture_output=True)
        if build.returncode:
            print(build.stdout + build.stderr)
            raise SystemExit(build.returncode)
        raise SystemExit(subprocess.run([str(binary)], check=False).returncode)


if __name__ == "__main__":
    main()
