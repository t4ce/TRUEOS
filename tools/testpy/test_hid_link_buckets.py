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
    assert_eq!(mouse.combo_id, tablet.combo_id);
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
    gamepad = (ROOT / "src/r/services/gamepad_control_service.rs").read_text()
    gamepad = gamepad.replace("use trueos_time::{Duration, Timer};", "")
    for marker in (
        "impl From<v::vinput::GamepadControlCommand>",
        "#[trueos_executor::task]\npub(crate) async fn gamepad_control_service_task()",
    ):
        gamepad = gamepad.replace(rust_item(gamepad, marker), "")
    gamepad = gamepad.replace(rust_item(gamepad, "fn uptime_ms()"), "fn uptime_ms() -> u64 { 0 }")
    gamepad += r'''
#[cfg(test)]
mod link_lifecycle_tests {
    use super::*;
    use crate::hut::{self, HidDeviceId, HidDeviceKind, HidSourceKind};

    fn identity(slot_id: u32) -> HidDeviceId {
        HidDeviceId { kind: HidDeviceKind::Gamepad, controller_id: 0, slot_id, ep_target: 0 }
    }

    #[test]
    fn idle_gamepads_are_visible_programs_preserve_pairings_and_release_removes_devices() {
        let mut station = GamepadControlStation::new();
        let principal = GamepadControlPrincipal::Vm(22);
        let first = station.request(principal, "pad one").unwrap();
        let second = station.request(principal, "pad two").unwrap();
        let device = hut::devices_snapshot().into_iter()
            .find(|device| device.id == identity(first.slot_id)).unwrap();
        assert_eq!(device.source_kind, HidSourceKind::Ai);
        assert_eq!(device.source_tag.as_str(), "pad one");
        assert!(device.virtual_device);
        assert_eq!(device.combo_id, 0);
        let combo = hut::request_combo(HidSourceKind::Human, "desk", None).unwrap();
        assert_eq!(hut::move_device_to_combo(device.id, combo.combo_id), Ok(()));
        let before = hut::devices_snapshot();
        station.submit(principal, first.handle, GamepadControlCommand {
            opcode: GAMEPAD_CONTROL_OPCODE_SET, buttons_set: 2, left_x: 123,
            ..GamepadControlCommand::default()
        }).unwrap();
        station.tick(0);
        assert_eq!(station.gamepads[0].snapshot.buttons_down, 2);
        assert_eq!(station.gamepads[0].snapshot.left_x, 123);
        assert_eq!(hut::devices_snapshot(), before);
        // Re-enrollment resolves the chosen combo instead of resetting it.
        assert!(hut::register_gamepad_device(0, first.slot_id, 0, HidSourceKind::Ai, "pad one", true));
        assert_eq!(hut::devices_snapshot(), before);
        assert_eq!(station.release(GamepadControlPrincipal::Vm(23), first.handle), Err(GamepadControlError::Denied));
        assert_eq!(hut::devices_snapshot(), before);
        station.release(principal, first.handle).unwrap();
        assert!(!hut::devices_snapshot().iter().any(|device| device.id == identity(first.slot_id)));
        assert_eq!(hut::combos_snapshot().iter().find(|entry| entry.combo_id == combo.combo_id).unwrap().gamepad_slot_id, 0);
        assert!(hut::remove_empty_combo(combo.combo_id));
        assert_eq!(station.release_principal(principal), 1);
        assert!(!hut::devices_snapshot().iter().any(|device| device.id == identity(second.slot_id)));
    }

    #[test]
    fn failed_hut_enrollment_rolls_back_mediated_station_allocation() {
        let mut filled = 0;
        for slot in 1..=256 {
            if !hut::register_gamepad_device(88, slot, 1, HidSourceKind::Ai, "capacity", true) {
                break;
            }
            filled = slot;
        }
        assert!(filled > 0 && filled < 256);
        let mut station = GamepadControlStation::new();
        assert_eq!(station.request(GamepadControlPrincipal::Kernel, "blocked"), Err(GamepadControlError::Capacity));
        assert!(station.gamepads.is_empty());
        assert!(hut::remove_slot(88, 1));
        let device = station.request(GamepadControlPrincipal::Kernel, "available").unwrap();
        station.release(GamepadControlPrincipal::Kernel, device.handle).unwrap();
        for slot in 2..=filled { assert!(hut::remove_slot(88, slot)); }
    }
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-hid-link-buckets-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text(
            '[package]\nname="hid-link-bucket-check"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nheapless="0.9"\nspin="0.10"\nserde_json="1"\nlibm="0.2"\n'
        )
        (work / "src/gamepad_control_service.rs").write_text(gamepad)
        (work / "src/lib.rs").write_text(
            '#![allow(dead_code)]\nextern crate alloc;\nextern crate self as v;\npub mod vinput {\n'
            + flag.group() + '\n' + definitions + '\n}\n'
            + f'#[path={json.dumps(str(ROOT / "src/usb3/hid/hut.rs"))}] pub(crate) mod hut;\n'
            + stream_discovery
            + '\n#[macro_export] macro_rules! log_info { ($($args:tt)*) => {}; }\n'
            + 'mod usb2 { pub(crate) mod hid { pub(crate) use crate::hut; } }\n'
            + 'mod gamepad_control_service;\n'
        )
        subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline", "--quiet",
            "--target", "x86_64-unknown-linux-gnu",
            "--", "--test-threads=1",
        ], cwd=work, check=True)


if __name__ == "__main__":
    main()
