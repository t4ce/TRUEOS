#!/usr/bin/env python3
"""Exercise live HID bucket moves and one-cursor pairing on the host."""
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
    vinput = (ROOT / "crates/trueos-v/src/vinput.rs").read_text()
    # Keep production source kinds and palette definitions without linking the
    # kernel ABI or VLayer's unrelated hardware-facing modules.
    definitions = vinput[vinput.index("/// Origin of an input collection."):
                         vinput.index("/// Address of one independently clocked input device.")]
    flag = re.search(r"^pub const INPUT_COMBO_FLAG_AUTO_ASSIGNED:.*?;$", vinput, re.MULTILINE)
    assert flag
    boot = (ROOT / "src/usb3/hid/boot.rs").read_text()
    stream_discovery = "mod boot {\n" + rust_item(boot, "enum HidBootKind {")
    stream_discovery += "\n" + rust_item(boot, "fn register_hut_stream_device(")
    stream_discovery += r'''
#[test]
fn opened_hid_streams_are_pairable_before_any_input_report() {
    use super::hut::{self, HidDeviceKind, HidSourceKind};
    for (kind, slot) in [
        (HidBootKind::Keyboard, 100),
        (HidBootKind::Mouse, 101),
        (HidBootKind::Tablet, 102),
    ] {
        register_hut_stream_device(kind, 74, slot, 3);
    }
    let devices: Vec<_> = hut::devices_snapshot().into_iter()
        .filter(|device| device.id.controller_id == 74).collect();
    assert_eq!(devices.len(), 3);
    for (kind, slot) in [
        (HidDeviceKind::Keyboard, 100),
        (HidDeviceKind::Mouse, 101),
        (HidDeviceKind::Tablet, 102),
    ] {
        let device = devices.iter().find(|device| device.id.kind == kind).unwrap();
        assert_eq!((device.id.controller_id, device.id.slot_id, device.id.ep_target), (74, slot, 3));
        assert_eq!(device.source_kind, HidSourceKind::Human);
        assert!(!device.virtual_device);
        assert_ne!(device.combo_id, 0);
    }
    let keyboard = hut::keyboards_snapshot().into_iter()
        .find(|device| device.controller_id == 74).unwrap();
    assert_eq!((keyboard.modifiers, keyboard.keys, keyboard.ascii), (0, [0; 6], [0; 6]));
    assert_eq!(keyboard.key_down_bits, [0; 8]);
    let mouse = hut::mice_snapshot().into_iter()
        .find(|device| device.controller_id == 74).unwrap();
    assert_eq!((mouse.x, mouse.y, mouse.buttons_down), (0.5, 0.5, 0));
    let tablet = hut::tablets_snapshot().into_iter()
        .find(|device| device.controller_id == 74).unwrap();
    assert_eq!((tablet.x, tablet.y, tablet.x_raw, tablet.y_raw), (0.5, 0.5, 0, 0));
    assert_eq!((tablet.buttons_down, tablet.report_id), (0, 0));
    assert_ne!(mouse.combo_id, tablet.combo_id);
    let before = hut::devices_snapshot();
    register_hut_stream_device(HidBootKind::EyeTracker, 74, 103, 3);
    assert_eq!(hut::devices_snapshot(), before);
    // Real reports can update state without changing a pairing chosen while idle.
    let combo = hut::request_combo(HidSourceKind::Human, "idle pairing", None).unwrap();
    let identity = devices.iter().find(|device| device.id.kind == HidDeviceKind::Keyboard).unwrap().id;
    assert_eq!(hut::move_device_to_combo(identity, combo.combo_id), Ok(()));
    hut::upsert_keyboard_state(74, 100, 3, 0, [4, 0, 0, 0, 0, 0], [b'a', 0, 0, 0, 0, 0], HidSourceKind::Human, "human", false);
    let keyboard = hut::keyboards_snapshot().into_iter()
        .find(|device| device.controller_id == 74).unwrap();
    assert_eq!(keyboard.combo_id, combo.combo_id);
    for slot in 100..=102 { assert!(hut::remove_slot(74, slot)); }
    assert!(hut::remove_empty_combo(combo.combo_id));
}
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-hid-link-buckets-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text(
            '[package]\nname="hid-link-bucket-check"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nheapless="0.9"\nspin="0.10"\n'
        )
        (work / "src/lib.rs").write_text(
            '#![allow(dead_code)]\nextern crate self as v;\npub mod vinput {\n'
            + flag.group() + '\n' + definitions + '\n}\n'
            + f'#[path={json.dumps(str(ROOT / "src/usb3/hid/hut.rs"))}] mod hut;\n'
            + stream_discovery
        )
        subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline", "--quiet",
            "--target", "x86_64-unknown-linux-gnu",
            "--", "--test-threads=1",
        ], cwd=work, check=True)


if __name__ == "__main__":
    main()
