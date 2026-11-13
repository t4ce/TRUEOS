#!/usr/bin/env python3
"""Run Spirit's real terminal background state and control-page writer on the host."""
from pathlib import Path
import argparse
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, item, constant


VFX = "src/spirit/Spirit_VFX.rs"
GPU = "src/intel/gpgpu/operations/spirit_vfx.rs"
source = (ROOT / VFX).read_text()
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--timeline", type=Path, help="Export production working/ready/acknowledged controls as CSV")
args = parser.parse_args()


def implementation(name):
    match = re.search(rf"^impl {name} \{{.*?^}}", source, re.M | re.S)
    if not match:
        raise ValueError(f"missing implementation: {name}")
    return match[0]


parts = [item(VFX, name) for name in (
    "SpiritVfxSliderSpec", "slider", "SpiritVfxBackgroundEffect", "SpiritVfxRgb8",
    "SpiritVfxAlphaBackground", "SpiritVfxTransition", "TerminalBackgroundVfxState",
    "codex_knot_effect", "transitioned_background", "control_range_at", "terminal_background_tests",
)]
parts += [implementation(name) for name in (
    "SpiritVfxBackgroundEffect", "SpiritVfxRgb8", "SpiritVfxTransition", "TerminalBackgroundVfxState",
)]
parts += [constant(VFX, name) for name in (
    "ALPHA_BACKGROUND_TRANSITION_CONTROLS", "POWER_MODE_VFX_TRANSITION_MS", "TERMINAL_VFX_TRANSITION_MS",
    "CODEX_VFX_SCALE", "CODEX_VFX_EXIT_SCALE", "CODEX_VFX_OPACITY", "CODEX_VFX_INTENSITY_CYCLE_MS",
)]
parts += [constant(GPU, "SPIRIT_VFX_BACKGROUND_PRESENT_SCALE")]
parts += ["const SPIRIT_BACKGROUND_PRESENT_SCALE: f32 = SPIRIT_VFX_BACKGROUND_PRESENT_SCALE;"]
declarations = "\n".join(parts)
# The host tests need no serde runtime or kernel hardware. Keep the real data
# layout, enums, methods and tests; omit only serialization derives/attributes.
declarations = declarations.replace(", Serialize, Deserialize", "")
declarations = re.sub(r"^#\[serde[^\n]*\]\n", "", declarations, flags=re.M)
if args.timeline:
    declarations += '''
pub fn activity_timeline() -> String {
    use std::fmt::Write;
    use trueos_terminal::program_status::ProgramStatus;
    let mut state = TerminalBackgroundVfxState::INACTIVE;
    let mut output = String::from("time,id,speed,intensity,scale,opacity,status\\n");
    for frame in 0..=480u64 {
        let now = frame * 1000 / 30;
        let (status, label) = if now < 8000 { (ProgramStatus::Working, "Working") }
            else if now < 13000 { (ProgramStatus::Done, "Ready - unread") }
            else { (ProgramStatus::Idle, "Acknowledged") };
        if let Some((effect, level)) = state.sample(Some(status), now) {
            let (bg, scale) = state.presentation(effect, level, now);
            writeln!(&mut output, "{:.4},{},{},{},{},{},{}", now as f32 / 1000.0,
                effect as u8, bg.speed, bg.intensity, scale, bg.opacity, label).unwrap();
        } else {
            writeln!(&mut output, "{:.4},0,0,0,1.4,0,{}", now as f32 / 1000.0, label).unwrap();
        }
    }
    output
}
'''

harness = f'''#![allow(dead_code)]
extern crate self as trueos_terminal;
#[path = "{ROOT / 'crates/trueos-terminal/src/program_status.rs'}"]
pub mod program_status;
mod libm {{ pub fn cosf(x: f32) -> f32 {{ x.cos() }} }}
mod presentation {{
use crate::libm;
{declarations}
}}
'''

gpu_parts = [item(GPU, name) for name in (
    "SpiritVfxControl", "SpiritVfxBuffer", "spirit_vfx_bounded", "spirit_vfx_shader_defaults",
    "spirit_vfx_write_control",
)]
gpu_parts += [constant("src/intel/gpgpu/rcs/constants.rs", name) for name in (
    "SPIRIT_VFX_CONTROL_MAGIC", "SPIRIT_VFX_CONTROL_VERSION",
)]
gpu_parts += [constant(GPU, "SPIRIT_VFX_BACKGROUND_PRESENT_SCALE")]
harness += '''
fn dma_flush(_: *mut u8, _: usize) {}
mod control_page {
struct GpgpuRgba8Surface { width: u32, height: u32, pitch_bytes: u32 }
''' + "\n".join(gpu_parts) + '''
#[test]
fn knot_ids_and_scale_reach_the_gpu_without_changing_the_existing_abi() {
    let mut page = [0u32; 33];
    let buffer = SpiritVfxBuffer { phys: 0, gpu: 0, virt: page.as_mut_ptr().cast(), bytes: 132 };
    let mut control = SpiritVfxControl {
        revision: 1, background_mode: 12, clock_seconds_of_day: 0,
        background_phase_override: None, background_scale: 1.4,
        background_opacity: 0.75, background_speed: 3.0, background_intensity: 4.0,
        background_color_a: 0xff688d, background_color_b: 0xfff26c,
        position_x: 0.0, position_y: 0.0, rotation_radians: 0.0,
        alpha_cutoff: 0.02, edge_fade_pixels: 0.0, sampling: 0,
        shader_mode: 1, shader_parameters: [12.0, 2.5, 0.0, 0.0],
        fx_color_a: 0, fx_color_b: 0,
    };
    for (id, requested_scale, expected_scale) in [
        (12, 1.4, 1.4), (12, 1.54, 1.54), (13, 1.54, 1.54), (13, 3.0, 3.0), (12, f32::NAN, 1.4),
        (12, 50.0, 3.0), (3, 0.5, 1.5625), (11, 0.5, 1.5625),
        (9, 3.0, SPIRIT_VFX_BACKGROUND_PRESENT_SCALE),
        (99, 3.0, SPIRIT_VFX_BACKGROUND_PRESENT_SCALE),
    ] {
        control.background_mode = id; control.background_scale = requested_scale;
        spirit_vfx_write_control(buffer, 60, 1024,
            GpgpuRgba8Surface { width: 256, height: 256, pitch_bytes: 1024 }, control, 60);
        assert_eq!(page[3], if id == 99 { 0 } else { id });
        assert_eq!(f32::from_bits(page[6]), expected_scale);
        if (2..=11).contains(&id) {
            assert_eq!(0.32 * 256.0 * f32::from_bits(page[6]), 128.0,
                "reference background ring fills the full 256px cursor canvas");
        }
        assert_eq!(f32::from_bits(page[13]), 1.0, "native sprite scale is independent of animated background scale");
        assert_eq!(f32::from_bits(page[14]), 0.0, "no legacy half-turn mirror correction");
        assert_eq!(f32::from_bits(page[31]), 0.0, "native bottom row is not feathered away");
        assert_eq!(f32::from_bits(page[5]), 0.75);
        assert_eq!(f32::from_bits(page[7]), 3.0);
        assert_eq!(f32::from_bits(page[8]), 4.0);
        assert_eq!((page[17], page[24], page[25], page[26], page[27]), (1, 256, 256, 1024, 1024));
    }
    control.rotation_radians = f32::NAN;
    control.edge_fade_pixels = f32::NAN;
    spirit_vfx_write_control(buffer, 60, 1024,
        GpgpuRgba8Surface { width: 256, height: 256, pitch_bytes: 1024 }, control, 60);
    assert_eq!((f32::from_bits(page[13]), f32::from_bits(page[14]), f32::from_bits(page[31])), (1.0, 0.0, 0.0));
}
}
'''

with tempfile.TemporaryDirectory(prefix="trueos-spirit-codex-vfx-") as temporary:
    path = Path(temporary) / "tests.rs"
    binary = Path(temporary) / "tests"
    path.write_text(harness + ("\nfn main() { print!(\"{}\", presentation::activity_timeline()); }\n" if args.timeline else ""))
    subprocess.run(["rustc", "--edition=2024", *([] if args.timeline else ["--test"]), str(path), "-o", str(binary)], check=True)
    if args.timeline:
        args.timeline.parent.mkdir(parents=True, exist_ok=True)
        args.timeline.write_bytes(subprocess.check_output([str(binary)]))
        print(args.timeline)
    else:
        subprocess.run([str(binary)], check=True)
