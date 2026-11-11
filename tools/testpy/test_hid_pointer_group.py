#!/usr/bin/env python3
"""Exercise production relative-HID publication and physical pointer grouping."""
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def rust_item(source, marker):
    start = source.index(marker)
    end = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def main():
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    source = (ROOT / "src/usb3/hid/mod.rs").read_text()
    wire = (ROOT / "crates/trueos-v/src/vinput.rs").read_text()
    definitions = wire[wire.index("/// Origin of an input collection."):
                       wire.index("/// Address of one independently clocked input device.")]
    flag = re.search(r"^pub const INPUT_COMBO_FLAG_AUTO_ASSIGNED:.*?;$", wire, re.MULTILINE)
    assert flag
    abi = (ROOT / "crates/trueos-v/src/bp_abi.rs").read_text()
    items = [rust_item(source, "pub(crate) struct HidRuntime {"),
             rust_item(source, "impl HidRuntime {")]
    for name in ("ZERO_CURSOR_EVENT", "HID_MOUSE_NORM_PER_DELTA", "HID_KIND_KEYBOARD",
                 "HID_KIND_MOUSE", "HID_KIND_TABLET", "HID_UDP_CONTROLLER_ID",
                 "HID_CURSOR_EVENT_FLAG_DEVICE_LOST",
                 "HID_CURSOR_EVENT_FLAG_POINTER_GROUP_RETIRED"):
        item = re.search(r"^(?:pub(?:\(crate\))? )?const " + name + r":.*?;",
                         source, re.MULTILINE | re.DOTALL)
        assert item, name
        items.append(item.group())
    for marker in ("fn clamp01(", "fn runtime_mut_or_insert(",
                   "fn physical_pointer_bindings(", "fn sync_pointer_group_snapshot(",
                   "fn publish_pointer_changes(", "fn push_mouse_cursor_event(",
                   "fn sync_runtime_cursor_snapshot(", "fn handle_mouse_boot_report_inner(",
                   "pub(crate) fn handle_mouse_report_values(",
                   "pub(crate) fn remove_hid_slot(", "pub(crate) fn center_cursor("):
        items.append(rust_item(source, marker))

    harness = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
extern crate self as v;
use alloc::vec::Vec;
use spin::Mutex;
#[macro_export] macro_rules! log { ($($args:tt)*) => {}; }
#[macro_export] macro_rules! log_info { ($($args:tt)*) => {}; }
mod allcaps { pub mod input { pub const HID_MOUSE_RING_CAP: usize = 32; } }
mod keyboard { pub(crate) struct KeyboardRing; impl KeyboardRing { pub fn new() -> Self { Self } } }
mod tablet { pub(crate) struct TabletRing; impl TabletRing { pub fn new() -> Self { Self } } }
static HID_RUNTIMES: Mutex<Vec<HidRuntime>> = Mutex::new(Vec::new());
static POINTER_GROUPS: Mutex<pointer_group::GroupState> = Mutex::new(pointer_group::GroupState::new());
static EVENTS: Mutex<Vec<TrueosHidCursorEvent>> = Mutex::new(Vec::new());
fn push_cursor_event(event: TrueosHidCursorEvent) { EVENTS.lock().push(event); }
fn notify_cursor_event_ready() {}
fn now_ms_u32() -> u32 { 42 }
mod r {
    pub(crate) use crate::cursor;
    pub mod keyboard {
        pub static LOSSES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
        pub fn signal_device_lost(_:u32, _:u32) -> usize {
            LOSSES.fetch_add(1, core::sync::atomic::Ordering::SeqCst); 1
        }
    }
}

#[test]
fn actual_publication_handoff_buttons_split_virtual_and_removal() {
    use hut::{HidDeviceId, HidDeviceKind, HidSourceKind};
    let mouse = HidDeviceId { kind: HidDeviceKind::Mouse, controller_id:77, slot_id:2, ep_target:1 };
    let pad = HidDeviceId { slot_id:3, ..mouse };
    hut::upsert_keyboard_state(77,1,1,0,[0;6],[0;6],HidSourceKind::Human,"human",false);
    let report = |slot, buttons, dx| handle_mouse_report_values(77,slot,1,buttons,dx,0,0,false);
    let take = || core::mem::take(&mut *EVENTS.lock());
    report(2,1,64);
    let first = take();
    assert_eq!(first.len(),1);
    assert_eq!((first[0].controller_id,first[0].slot_id,first[0].buttons_down),(77,2,1));
    assert_eq!(first[0].x,0.5+64.0/1024.0); // first motion applied exactly once
    report(3,0,32);
    let next = take();
    assert_eq!((next[0].slot_id,next[0].buttons_down),(2,1));
    assert_eq!(next[0].x,0.5+96.0/1024.0);
    assert_eq!(next[0].flags & (1<<2),0); // idle pad cannot release mouse drag
    let raw = hut::mice_snapshot();
    assert_eq!(raw.len(),2);
    assert_eq!(raw[0].x,0.5+64.0/1024.0);
    assert_eq!(raw[1].x,0.5+32.0/1024.0);
    assert_eq!(raw[0].combo_id,raw[1].combo_id);
    let human = raw[0].combo_id;
    report(3,2,0); assert_eq!(take()[0].buttons_down,3);
    report(2,0,0); assert_eq!(take()[0].buttons_down,2);
    report(2,0,0); assert_eq!(take()[0].flags & (1<<2),0);

    // UDP's boot decoder initially registers Human metadata; its first frame
    // must already be independent and its buttons must not enter the union.
    handle_mouse_boot_report_inner(HID_UDP_CONTROLLER_ID,99,0,&[4,1,0]);
    let udp = take();
    assert_eq!((udp[0].controller_id,udp[0].slot_id,udp[0].buttons_down),(HID_UDP_CONTROLLER_ID,99,4));
    report(2,0,0); assert_eq!(take()[0].buttons_down,2);
    hut::upsert_mouse_state(88,90,0,0.2,0.3,8,HidSourceKind::Human,"virtual human",true);
    push_mouse_cursor_event(TrueosHidCursorEvent {controller_id:88,slot_id:90,hid_kind:HID_KIND_MOUSE,
        x:0.2,y:0.3,buttons_down:8,..ZERO_CURSOR_EVENT},0,0,0.2,0.3);
    assert_eq!(take()[0].controller_id,88);
    report(2,0,0); assert_eq!(take()[0].buttons_down,2);

    center_cursor(77,2,1);
    report(3,2,16);
    assert_eq!(take()[0].x,0.5+16.0/1024.0);
    let split = hut::request_combo(HidSourceKind::Human,"split",None).unwrap().combo_id;
    hut::move_device_to_combo(pad,split).unwrap();
    report(3,2,8);
    let split_events = take();
    assert_eq!(split_events.len(),2);
    assert_eq!((split_events[0].slot_id,split_events[0].buttons_down),(2,0));
    assert_eq!((split_events[1].slot_id,split_events[1].buttons_down),(3,2));
    assert_eq!(split_events[0].x,0.5+16.0/1024.0);
    assert_eq!(split_events[1].x,0.5+24.0/1024.0);
    hut::move_device_to_combo(pad,human).unwrap();
    report(3,2,0);
    let merged = take();
    assert_eq!(merged.len(),2);
    assert_eq!((merged[0].slot_id,merged[0].flags),(3,HID_CURSOR_EVENT_FLAG_POINTER_GROUP_RETIRED));
    assert_eq!((merged[1].slot_id,merged[1].buttons_down),(2,2));
    assert_eq!(merged[1].x,0.5+16.0/1024.0);
    assert_eq!(r::keyboard::LOSSES.load(core::sync::atomic::Ordering::SeqCst),0);

    remove_hid_slot(mouse.controller_id,mouse.slot_id);
    let promoted = take();
    assert_eq!(promoted.len(),1);
    assert_eq!((promoted[0].slot_id,promoted[0].buttons_down),(3,2));
    assert_eq!(promoted[0].flags & HID_CURSOR_EVENT_FLAG_DEVICE_LOST,0);
    assert_eq!(promoted[0].x,0.5+16.0/1024.0);
    assert_eq!(hut::keyboards_snapshot().len(),1);
    remove_hid_slot(pad.controller_id,pad.slot_id);
    let removed = take();
    assert_eq!(removed.len(),1);
    assert_eq!((removed[0].slot_id,removed[0].flags),(3,HID_CURSOR_EVENT_FLAG_POINTER_GROUP_RETIRED));
    assert_eq!(r::keyboard::LOSSES.load(core::sync::atomic::Ordering::SeqCst),2);
}
'''
    harness += '\npub mod vinput {\n' + flag.group() + '\n' + definitions + '\n}\n'
    harness += '#[derive(Copy,Clone,Debug,Default)]\n' + rust_item(abi, "pub struct TrueosHidCursorEvent {")
    for module, path in (("hut", "src/usb3/hid/hut.rs"),
                         ("input", "src/usb3/hid/input.rs"),
                         ("mouse", "src/usb3/hid/mouse.rs"),
                         ("pointer_group", "src/usb3/hid/pointer_group.rs"),
                         ("cursor", "src/r/cursor.rs")):
        harness += f'\n#[path={json.dumps(str(ROOT / path))}] pub(crate) mod {module};\n'
    harness += '\n' + '\n'.join(items)
    with tempfile.TemporaryDirectory(prefix="trueos-hid-pointer-group-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text(
            '[package]\nname="hid-pointer-group-check"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nheapless="0.9"\nspin="0.10"\n'
        )
        (work / "src/lib.rs").write_text(harness)
        subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline", "--quiet",
            "--target", "x86_64-unknown-linux-gnu", "--", "--test-threads=1",
        ], cwd=work, check=True)


if __name__ == "__main__":
    main()
