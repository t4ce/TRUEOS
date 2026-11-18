#!/usr/bin/env python3
"""Run native mediated glyph-cursor tests without UI hardware.

The kernel does not link as a host test crate. Keep the production mouse
station and its inline tests, removing only its executor task and clock/
viewport discovery. Import the actual shared-atlas sampler, HID injection,
slot-4 glyph drawing and UI4 visual cursor routing. Hardware registries and
presentation wakeups are counted by the fixture.
"""

import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).with_name("mouse_visual_cursors")
HOST = "x86_64-unknown-linux-gnu"


def block(source: str, marker: str) -> str:
    """Extract a named method/impl with balanced braces from these fixtures."""
    assert source.count(marker) == 1, marker
    start = source.index(marker)
    end = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    mouse = (ROOT / "src/r/services/mouse_motion_service.rs").read_text()
    mouse = mouse.replace("use trueos_time::{Duration, Timer};", "")
    mouse = mouse.replace(block(mouse, "impl From<v::vinput::MouseMotionCommand>"), "")
    task_start = mouse.index("#[trueos_executor::task]")
    task_end = mouse.index("fn validate_command", task_start)
    mouse = mouse[:task_start] + mouse[task_end:]
    mouse = mouse.replace(block(mouse, "fn viewport_dimensions()"),
                          "fn viewport_dimensions() -> (u32,u32) { (320,200) }")
    mouse = mouse.replace(block(mouse, "fn uptime_ms()"), "fn uptime_ms() -> u64 { 0 }")
    mouse += "\npub(crate) fn fixture_tick(now_ms: u64) { STATION.lock().tick(now_ms); }\n"

    atlas_path = "src/r/services/microfont_mask_service.rs"
    atlas_source = (ROOT / atlas_path).read_text()
    constants_start = atlas_source.index("pub(crate) const ATLAS_COLUMNS")
    constants_end = atlas_source.index("#[derive(Clone, Copy, Debug, PartialEq)]")
    font = "use alloc::vec::Vec;\n" + atlas_source[constants_start:constants_end]
    font += "\n".join(item(atlas_path, name)
                      for name in ("glyph_slot", "AlphaAtlasSource", "OwnedAlphaAtlas", "ready_atlas"))
    font += block(atlas_source, "impl OwnedAlphaAtlas {") + "\n"
    atlas_static = re.search(r"^static ALPHA_ATLAS:.*?;$", atlas_source, re.MULTILINE)
    assert atlas_static
    font += atlas_static.group() + "\n"
    font += (FIXTURES / "atlas_fixture.rs").read_text()

    slot_path = "src/ui4/slot4_service.rs"
    slot = (ROOT / slot_path).read_text()
    render = "use alloc::{sync::Arc, vec::Vec};\nuse core::ops::Range;\n"
    render += "use crate::graphics::primitives::Rgba8;\n"
    render += "type Slot4Rects=Vec<crate::intel::LiveOverlayRect>;\n"
    render += item(slot_path, "Slot4Scene") + block(slot, "impl Slot4Scene {") + "\n"
    for name in ("push_cursor_glyph", "push_cursor_text", "push_overlay_rect",
                 "changed_scene_damage", "changed_rect_damage", "overlay_rect_eq",
                 "add_overlay_rect_damage"):
        render += item(slot_path, name)
    render += """
pub(crate) fn draw_test_glyph(
    x: u32,
    y: u32,
    glyph: &crate::mouse_motion_service::MouseControlGlyph,
    color: Rgba8,
    text_color: Rgba8,
    width: u32,
    height: u32,
) -> Slot4Rects {
    let mut rects = Slot4Rects::new();
    push_cursor_text(&mut rects, x, y, glyph, color, text_color, width, height);
    rects
}

pub(crate) fn glyph_damage(
    previous: &[Slot4Rects],
    current: &[Slot4Rects],
) -> crate::intel::CompositionDamageRegion {
    let make_scene = |groups: &[Slot4Rects]| {
        let mut scene = Slot4Scene::new();
        for rects in groups {
            let start = scene.rects.len();
            scene.rects.extend_from_slice(rects);
            scene.glyphs.push(start..scene.rects.len());
        }
        scene
    };
    changed_scene_damage(&make_scene(previous), &make_scene(current))
}
"""
    for name in ("moving_glyph_damages_old_and_new_label_bounds",
                 "glyph_cursor_centers_and_clips_prepared_runs_on_slot_four",
                 "glyph_recolor_only_changes_coverage_modulated_tint",
                 "text_cursor_recolor_keeps_nickname_rects_and_damage_outside_it_unchanged",
                 "creeping_dash_damages_only_its_removed_and_added_pixel_strips"):
        render += "#[test]\n" + block(slot, f"fn {name}()") + "\n"

    broker_path = "src/ui4/input_broker.rs"
    broker = (ROOT / broker_path).read_text()
    routing = item(broker_path, "visual_only_cursor")
    routing += item(broker_path, "normalized_visual_cursor_pixel")
    routing += item(broker_path, "normalized_to_pixel")
    routing += "impl InputBroker {\n"
    for name in ("cursor_index", "refresh_remote_input_route", "retire_pointer_group", "retire_hid_slot"):
        routing += block(broker, f"fn {name}(") + "\n"
    # Keep real event dispatch through the end of the visual-only branch.
    # Interactive gestures depend on the live window manager.
    visual_dispatch = block(broker, "fn process_cursor(").split(
        "        if !super::spirit_access::cursor_event_allowed", 1,
    )
    assert len(visual_dispatch) == 2
    routing += visual_dispatch[0] + 'panic!("visual test reached interactive input routing");\n}\n'
    routing += block(broker, "fn keyboard_route_index(") + "\n"
    routing += block(broker, "fn select_frame(") + "\n}\n"
    routing += item(broker_path, "visual_only_cursor_tests")

    with tempfile.TemporaryDirectory(prefix="trueos-mouse-visual-cursors-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text('''[package]
name = "trueos-visual-cursor-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
heapless = "0.9.1"
spin = "0.10.1"
serde_json = "1.0.154"
libm = "0.2.16"
microflake = { path = ''' + json.dumps(str(ROOT / "crates/microflake")) + ''' }
microfont = { path = ''' + json.dumps(str(ROOT / "vendor/microfont")) + ''', default-features = false, features = ["raster"] }
''')
        fixture_lock = FIXTURES / "Cargo.lock"
        if fixture_lock.is_file():
            (work / "Cargo.lock").write_bytes(fixture_lock.read_bytes())
        for name, source in (("mouse", mouse), ("font", font), ("render", render),
                             ("routing", routing)):
            (work / "src" / f"{name}.rs").write_text(source)
        (work / "src/moving_markers.rs").write_bytes((FIXTURES / "moving_markers.rs").read_bytes())
        hid = item("crates/trueos-v/src/bp_abi.rs", "TrueosHidCursorEvent")
        hid_path = "src/usb3/hid/mod.rs"
        hid += "\n".join(re.findall(
            r"^pub(?:\(crate\))? const HID_CURSOR_EVENT_FLAG_[A-Z_]+: u32 = [^;]+;",
            (ROOT / hid_path).read_text(), re.MULTILINE,
        )) + "\n"
        hid += item(hid_path, "inject_virtual_cursor_event")
        hid += item(hid_path, "inject_virtual_cursor_report")
        (work / "src/hid_injection.rs").write_text(hid)
        (work / "src/damage.rs").write_bytes((ROOT / "src/ui4/damage.rs").read_bytes())
        (work / "src/button_outline_geometry.rs").write_bytes((ROOT / "src/ui4/button_outline_geometry.rs").read_bytes())
        (work / "src/lib.rs").write_text((FIXTURES / "harness.rs").read_text()
            + '\n#[path="button_outline_geometry.rs"] mod button_outline_geometry;\n'
            + '#[path=' + json.dumps(str(ROOT / "src/r/services/shell3_hex_font.rs")) + '] mod hex;\n')
        build = subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline",
            "--target", HOST, "--no-run", "--message-format=json",
        ], cwd=work, text=True, capture_output=True)
        if build.returncode:
            print(build.stdout + build.stderr)
            raise SystemExit(build.returncode)
        artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
        binaries = [entry["executable"] for entry in artifacts
                    if entry.get("reason") == "compiler-artifact"
                    and entry.get("profile", {}).get("test") and entry.get("executable")]
        assert len(binaries) == 1
        binary = binaries[0]
        listing = subprocess.check_output([binary, "--list"], text=True)
        names = [line.removesuffix(": test") for line in listing.splitlines() if line.endswith(": test")]
        assert names
        failed = []
        for name in names:
            result = subprocess.run([binary, name, "--exact"], text=True, capture_output=True)
            print(("PASS " if result.returncode == 0 else "FAIL ") + name)
            if result.returncode:
                print(result.stdout + result.stderr)
                failed.append(name)
        print(f"{len(names) - len(failed)} passed; {len(failed)} failed")
        raise SystemExit(bool(failed))


if __name__ == "__main__":
    main()
