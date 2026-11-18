#!/usr/bin/env python3
"""Check the rectangle diagnostic's sampled commands and pixel reference."""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item


ROOT = Path(__file__).resolve().parents[2]
RCS = "src/intel/gpgpu/rcs/"
OPS = "src/intel/gpgpu/operations/rectangle_dispatch.rs"


def harness_source() -> str:
    declarations = ["#![allow(dead_code)]"]
    for name in (
        "RCS_RING_BASE", "PIPE_CONTROL_CMD", "PIPE_CONTROL_CS_STALL",
        "PIPE_CONTROL_FLUSH_ENABLE", "PIPE_CONTROL_POST_SYNC_WRITE_TIMESTAMP",
        "DIRECT_RCS_TIMESTAMP_BITS", "DIRECT_RCS_TIMESTAMP_MASK",
    ):
        declarations.append(constant(RCS + "constants.rs", name))
    for name in ("RECT_DISPATCH_WIDTH", "RECT_DISPATCH_LEFT", "RECT_DISPATCH_BAND",
                 "RECT_DISPATCH_BACKGROUND", "RECT_DISPATCH_COLOR", "RECT_DISPATCH_PAIRS"):
        declarations.append(constant(OPS, name))
    for name in ("direct_rcs_push", "direct_rcs_push_pipe_control_full",
                 "direct_rcs_push_pipe_control_timestamp_at", "direct_rcs_timestamp_delta_ticks"):
        declarations.append(item(RCS + "commands.rs", name))
    for name in ("RectDispatchProbe", "direct_rcs_push_rect_dispatch_sample"):
        declarations.append(item(RCS + "worklists.rs", name))
    for name in ("RectDispatchSample", "rectangle_dispatch_reference", "rectangle_dispatch_percentiles"):
        declarations.append(item(OPS, name))
    operations = (ROOT / OPS).read_text()
    begin = operations.index("impl RectDispatchSample {")
    declarations.append(operations[begin:operations.index("\n}\n", begin) + 2])
    return "\n".join(declarations) + "\n" + TESTS


TESTS = r'''
#[test]
fn counter_packets_store_both_halves_in_owned_ppgtt() {
    let mut batch = [0; 20];
    let mut cursor = 0;
    assert!(direct_rcs_push_rect_dispatch_sample(
        &mut batch, &mut cursor, 0x1_0200_0000, 16,
        RectDispatchProbe::Counter { skip_walker: false },
    ));
    assert_eq!(cursor, 14);
    assert_eq!(&batch[..6], &[0x7a00_0004, 0x0010_0002, 0, 0, 0, 0]);
    assert_eq!(&batch[6..14], &[
        0x1208_0002, 0x290, 0x0200_0040, 1,
        0x1208_0002, 0x294, 0x0200_0044, 1,
    ]);
}

#[test]
fn timing_uses_a_separate_stall_and_no_register_reads() {
    let mut batch = [0; 20];
    let mut cursor = 0;
    assert!(direct_rcs_push_rect_dispatch_sample(
        &mut batch, &mut cursor, 0x1_0200_0000, 18, RectDispatchProbe::Timing,
    ));
    assert_eq!(cursor, 12);
    assert_eq!(&batch[..12], &[
        0x7a00_0004, 0x0010_0000, 0, 0, 0, 0,
        0x7a00_0004, 0x0010_c080, 0x0200_0048, 1, 0, 0,
    ]);
    assert!(direct_rcs_push_rect_dispatch_sample(
        &mut batch, &mut cursor, 0, 16, RectDispatchProbe::Disabled,
    ));
    assert_eq!(cursor, 12);
}

#[test]
fn validates_full_width_snapshots_and_ordered_timestamps() {
    let sample = |before, after| RectDispatchSample { before, after, host_us: 0 };
    assert_eq!(sample(0x91_ffff_fff0, 0x92_0000_00f0).lanes(), Ok(256));
    assert_eq!(sample(0, 0).lanes(), Ok(0));
    assert!(sample(0xffff_ffff_0000_0000, 256).lanes().is_err());
    assert!(sample(0, 0x0000_0000_ffff_ffff).lanes().is_err());
    assert_eq!(sample((1 << 36) - 4, 12).gpu_ticks(), Ok(16));
    assert_eq!(sample(17, 17).gpu_ticks(), Ok(0));
    assert!(sample(17, 0).gpu_ticks().is_err());
    assert!(sample(17, 16).gpu_ticks().is_err());
}

#[test]
fn pixel_reference_covers_the_band_and_guards() {
    for rows in [4, 32] {
        for index in 0..64 * 32 {
            let expected = if index / 64 < rows && (8..24).contains(&(index % 64)) {
                0xFFCC_8844
            } else {
                0xFF11_2233
            };
            assert_eq!(rectangle_dispatch_reference(index, rows, false), expected);
            assert_eq!(rectangle_dispatch_reference(index, rows, true), 0xFF11_2233);
        }
    }
    let descending = core::array::from_fn(|index| 24 - index as u64);
    assert_eq!(rectangle_dispatch_percentiles(descending), (12, 23));
}
'''


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="trueos-rectangle-dispatch-") as scratch:
        path = Path(scratch)
        (path / "tests.rs").write_text(harness_source())
        subprocess.run(["rustc", "--edition=2024", "--test", str(path / "tests.rs"),
                        "-o", str(path / "tests")], check=True, cwd=ROOT)
        subprocess.run([str(path / "tests")], check=True)
