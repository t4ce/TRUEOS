#!/usr/bin/env python3
"""Check the shared HID queue after retiring the dedicated QJS mouse stream."""
from pathlib import Path
import json
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-hid-input-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text(
            '[package]\nname="hid-input-check"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nheapless="0.8"\nspin="0.10"\n'
        )
        (work / "src/lib.rs").write_text(
            '#![allow(dead_code)]\n'
            f'#[path={json.dumps(str(ROOT / "src/usb3/hid/input.rs"))}] mod input;\n'
            r'''
#[test]
fn shared_mouse_poll_and_hot_unplug_keep_their_queue_semantics() {
    use input::*;
    push_event(InputEvent::Mouse(MouseEvent {
        slot_id: 7, buttons: 1, dx: 3, dy: -2, wheel: 1, has_wheel: true,
    }));
    let mouse = mouse_poll();
    assert_eq!((mouse.slot_id, mouse.buttons, mouse.x, mouse.y), (7, 1, 3, -2));
    assert_eq!((mouse.dx, mouse.dy, mouse.wheel), (3, -2, 1));
    let mouse = mouse_poll();
    assert_eq!((mouse.dx, mouse.dy, mouse.wheel), (0, 0, 0));
    push_event(InputEvent::Tablet(TabletEvent { slot_id: 8, ..Default::default() }));
    assert_eq!(remove_slot(7), 1);
    assert!(pop_mouse_event().is_none());
    let mouse = mouse_poll();
    assert_eq!((mouse.slot_id, mouse.buttons), (0, 0));
    assert_eq!(pop_tablet_event().unwrap().slot_id, 8);
    assert_eq!(remove_slot(7), 0);
}
'''
        )
        subprocess.run(["cargo", "test", "--offline", "--quiet"], cwd=work, check=True)


if __name__ == "__main__":
    main()
