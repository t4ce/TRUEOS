#!/usr/bin/env python3
"""Exercise production cursor/route color resolution without GPU hardware."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item
from test_mouse_visual_cursors import block

ROOT = Path(__file__).resolve().parents[2]


def main():
    palette = (ROOT / "crates/trueos-v/src/vinput.rs").read_text()
    broker = (ROOT / "src/ui4/input_broker.rs").read_text()
    source = r'''
extern crate self as v;
use std::sync::atomic::{AtomicU8, AtomicBool, Ordering};
static COMBO: AtomicU8 = AtomicU8::new(255);
static OVERRIDE: AtomicBool = AtomicBool::new(false);
pub mod graphics { pub mod primitives {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Rgba8(pub u8, pub u8, pub u8, pub u8);
    impl Rgba8 { pub const fn new(r:u8,g:u8,b:u8,a:u8)->Self { Self(r,g,b,a) } }
}}
pub mod r {
    pub mod cursor { pub const HID_KIND_VIRTUAL_CURSOR:u8=4; pub const HID_KIND_TABLET:u8=2; }
    pub mod services { pub mod mouse_motion_service {
        pub fn cursor_visual_color(_:u32)->Option<crate::graphics::primitives::Rgba8> {
            crate::OVERRIDE.load(crate::Ordering::Relaxed).then_some(crate::graphics::primitives::Rgba8::new(255,55,255,255))
        }
    }}
}
pub mod usb2 { pub mod hid { pub mod hut {
    pub fn combo_color_for_cursor(_:u32,_:u32,_:u32,_:bool)->Option<u8> {
        let value=crate::COMBO.load(crate::Ordering::Relaxed);
        (value!=255).then_some(value)
    }
}}}
#[derive(Clone,Copy)]
pub struct Ui4CursorSource {controller_id:u32,slot_id:u32,ep_target:u32,hid_kind:u8}
'''
    source += "pub mod vinput {\n" + item("crates/trueos-v/src/vinput.rs", "InputComboColor")
    source += block(palette, "impl InputComboColor {") + "\n}\n"
    source += "pub mod ui4 { pub mod cursor_frame_inout { use crate::Ui4CursorSource;\n"
    source += item("src/ui4/cursor_frame_inout.rs", "cursor_color")
    source += item("src/ui4/cursor_frame_inout.rs", "input_combo_color_rgba")
    source += "}\n pub mod input_broker { use crate::Ui4CursorSource; pub struct CursorRoute { pub source:Ui4CursorSource }\nimpl CursorRoute {\n"
    source += block(broker, "fn color(&self)") + "\n}\n"
    source += r'''
#[test]
fn stationary_route_follows_late_binding_and_subsequent_palette_edits() {
    crate::OVERRIDE.store(false,crate::Ordering::Relaxed);
    crate::COMBO.store(255,crate::Ordering::Relaxed);
    let route=CursorRoute { source:Ui4CursorSource {controller_id:0,slot_id:1442840577,ep_target:0,hid_kind:crate::r::cursor::HID_KIND_VIRTUAL_CURSOR} };
    let _unbound=route.color();
    crate::COMBO.store(crate::vinput::InputComboColor::Coral as u8,crate::Ordering::Relaxed);
    assert_eq!(route.color(),crate::graphics::primitives::Rgba8::new(255,64,64,255));
    crate::COMBO.store(crate::vinput::InputComboColor::Mint as u8,crate::Ordering::Relaxed);
    assert_eq!(route.color(),crate::graphics::primitives::Rgba8::new(32,224,128,255));
}
#[test]
fn explicit_virtual_visual_overrides_remain_independent_of_physical_palette() {
    crate::COMBO.store(crate::vinput::InputComboColor::Azure as u8,crate::Ordering::Relaxed);
    crate::OVERRIDE.store(true,crate::Ordering::Relaxed);
    let mut route=CursorRoute { source:Ui4CursorSource {controller_id:0,slot_id:7,ep_target:0,hid_kind:crate::r::cursor::HID_KIND_VIRTUAL_CURSOR} };
    assert_eq!(route.color(),crate::graphics::primitives::Rgba8::new(255,55,255,255));
    route.source.hid_kind=1;
    assert_eq!(route.color(),crate::graphics::primitives::Rgba8::new(32,168,255,255));
}''' + "}}\n"
    with tempfile.TemporaryDirectory(prefix="trueos-combo-cursor-color-") as directory:
        work = Path(directory)
        path = work / "test.rs"
        path.write_text(source)
        subprocess.run(["rustc", "+nightly-2026-07-10", "--edition=2024", "--test", str(path), "-o", str(work / "tests")], check=True, cwd=work)
        subprocess.run([str(work / "tests"), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
