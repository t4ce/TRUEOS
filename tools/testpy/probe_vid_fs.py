#!/usr/bin/env python3
"""Probe local vid fs assets without connecting to hardware or transcoding.

Compile the production MP4 demuxer, access-unit collector, AVC parser, DPB,
resource binding and command builder on the host. Compare sample count, visible
extent, colour metadata and MP4 timing against ffprobe. A constant edit-list
origin shift is normalized, as playback also subtracts the first display PTS. GPU addresses are synthetic;
this does NOT check decoded pixels, DMA, GPU completion or presentation pacing.

Requires rustc and ffprobe. Example:
  python3 tools/probe_vid_fs.py tools/vid/screenrec.mp4 \
      tools/vid/x31_head_movie.annexb.h264
"""

import argparse
from fractions import Fraction
import json
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item
from test_rdp_pipeline import block


VID = "src/intel/media/hw_vid.rs"
PIC = "src/intel/media/hw_pic.rs"


def harness():
    source = (ROOT / VID).read_text()
    result = r'''
#![allow(dead_code, unused_variables, unfulfilled_lint_expectations)]
extern crate alloc;
use alloc::{string::String, vec::Vec};
use core::fmt::Write;
use std::io::Write as IoWrite;
use std::sync::atomic::{AtomicU16, Ordering};
#[macro_export] macro_rules! log { ($($t:tt)*) => {}; }
#[macro_export] macro_rules! log_info { ($($t:tt)*) => {}; }
// Heap inspection is diagnostic only; the probe uses the host allocator.
mod allocators { pub fn host_heap_integrity_bounded() {} }
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(v: T) -> Self { Self(std::sync::Mutex::new(v)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod intel { pub(crate) use crate::h264_cmd as xelp_media_avc_decode_recipe; }
static AVC_DPB: [Mutex<AvcDpbState>; 3] = [const { Mutex::new(AvcDpbState::new()) }; 3];
static AVC_PRESENTATION_HOLDS: [AtomicU16; 3] = [const { AtomicU16::new(0) }; 3];
'''
    result += f'#[path = {json.dumps(str(ROOT / "src/intel/media/h264_cmd.rs"))}] mod h264_cmd;\n'
    result += constant(VID, "H264_TRUEOSFS_VIDEO_SOFT_CAP_BYTES") + "\n"
    # Contiguous pure demux and AU types/helpers; fail explicitly if moved.
    result += source[source.index("#[derive(Clone, Copy, Debug)]\nstruct Mp4Box"):
                     source.index("struct H264IndexedFrame")]
    result += item(VID, "H264MemoryNalReader")
    result += block(VID, "impl H264MemoryNalReader {")
    for name in ["h264_find_start_code", "h264_slice_first_mb_in_slice",
                 "h264_read_first_ue_from_ebsp", "h264_ebsp_bit"]:
        result += item(VID, name)
    result += constant(PIC, "AVC_DPB_RETAINED_REFS") + "\n"
    for name in ["AvcDpbEntry", "AvcDpbState", "avc_dpb_entry_older",
                 "avc_frame_num_wrap", "avc_apply_ref_list_modifications",
                 "align_up_usize", "AvcDpbProbeLayout", "avc_dpb_probe_layout",
                 "avc_prepare_reference_state", "avc_commit_decoded_reference",
                 "avc_dmv_region_offset", "avc_dmv_slot_gpu_addr", "avc_scratch_bindings"]:
        result += item(PIC, name)
    result += block(PIC, "impl AvcDpbState {")
    probe = (ROOT / "tools/vid_fs_probe.rs").read_text()
    start = source.index("    while let Some(nal) = reader.next_nal().await {",
                         source.index("async fn h264_i_p_playback_probe_with_reader"))
    end = source.index("    if !sample_timing.is_empty()", start)
    # Use the exact playback collector, substituting only the synchronous
    # entry point of the same in-memory reader. Keep its counters intact.
    collector = source[start:end].replace("reader.next_nal().await", "reader.try_take_nal()")
    # Scheduling/cancellation belong to the live session harness, not the pure
    # demux/parser probe; retain the production collector itself verbatim.
    collector = collector.replace("        if nal_count % 64 == 0 {\n            Timer::after_millis(1).await;\n        }\n", "")
    collector = collector.replace("        if session.is_cancelled() {\n            break;\n        }\n", "")
    assert probe.count("    // PRODUCTION_ACCESS_UNIT_COLLECTOR") == 1
    result += probe.replace("    // PRODUCTION_ACCESS_UNIT_COLLECTOR", collector)
    return result


def check_ffprobe(asset, rows):
    reference = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-select_streams", "v:0", "-show_packets",
        "-show_entries", "packet=dts,pts,duration:stream=width,height,time_base,color_range,color_space",
        "-of", "json", str(asset),
    ], text=True))
    stream, = reference["streams"]
    packets = reference["packets"]
    assert len(rows) == len(packets), (asset, "frame count", len(rows), len(packets))
    time_base = Fraction(stream["time_base"])
    origin_shift = Fraction(0)
    first = list(map(int, rows[0].split()))
    if first[5]:
        origin_shift = Fraction(first[2], first[5]) - int(packets[0]["dts"]) * time_base
    for index, (row, packet) in enumerate(zip(rows, packets)):
        width, height, dts, pts, duration, timescale, full_range, matrix = map(int, row.split())
        assert (width, height) == (stream["width"], stream["height"]), (asset, index, "extent")
        if "color_range" in stream:
            assert bool(full_range) == (stream["color_range"] == "pc"), (asset, index, "range")
        if stream.get("color_space") == "bt709":
            assert matrix == 1, (asset, index, "BT.709 matrix", matrix)
        if timescale:
            for name, actual in [("dts", dts), ("pts", pts), ("duration", duration)]:
                expected = int(packet[name]) * time_base
                if name != "duration":
                    expected += origin_shift
                assert Fraction(actual, timescale) == expected, (asset, index, name, actual, expected)
    print(f"ffprobe agreement: {len(rows)} frames, visible extent, colour metadata and normalized MP4 timing", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("assets", type=Path, nargs="+")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="trueos-vid-fs-") as directory:
        directory = Path(directory)
        source, binary, metadata = (directory / name for name in ["probe.rs", "probe", "frames.tsv"])
        source.write_text(harness())
        subprocess.run(["rustc", "--edition=2024", "-O", str(source), "-o", str(binary)], check=True)
        for asset in args.assets:
            print(f"asset: {asset}", flush=True)
            subprocess.run([str(binary), str(asset.resolve()), str(metadata)], check=True)
            check_ffprobe(asset, metadata.read_text().splitlines())


if __name__ == "__main__":
    main()
