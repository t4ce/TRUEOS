#!/usr/bin/env python3
"""Host-test primary counter packets and the native secondary opening contract."""
from pathlib import Path
import re
import subprocess
import tempfile

import test_clip_position3_uv_texture as production

ROOT = Path(__file__).resolve().parents[2]
production.ROOT = ROOT
constant, item = production.constant, production.item


def native_secondary_opening_source() -> str:
    """Execute the real encoder's opening, retaining its nested emitters.

    The full encoder also needs hardware, shaders, and hundreds of unrelated
    state packets. Extract exactly its opening commands; only logging and the
    warm-state holder are replaced for this hardware-free integration test.
    """
    encoder = item("src/intel/render/pipeline.rs", "encode_triangle_probe_batch")
    helpers = []
    for name in ("push", "push_addr", "push_pipe_control", "push_pipe_control_full",
                 "push_store_data_imm"):
        matches = list(re.finditer(
            rf"^    fn {name}\([\s\S]*?^    }}", encoder, re.MULTILINE))
        if len(matches) != 1:
            raise ValueError(f"native opening: expected one production helper {name}")
        helpers.append(matches[0].group())
    start = encoder.index("    batch_dwords.fill(0);")
    end = encoder.index("\n    if device_is_gfx12(warm.device_id) {", start)
    return """
fn encode_native_secondary_opening(
    batch_dwords: &mut [u32], device_id: u16, result_gpu_addr: u64,
) -> Result<usize, &'static str> {
    struct OpeningWarm { device_id: u16 }
    let warm = OpeningWarm { device_id };
    let mut cursor = 0usize;
    fn log_batch_offset(_: usize, _: &str) {}
""" + "\n".join(helpers) + "\n" + encoder[start:end] + """
    Ok(cursor * core::mem::size_of::<u32>())
}
"""


SECONDARY_OPENING_TESTS = r"""
#[cfg(test)]
mod native_secondary_opening_tests {
    use super::*;

    const RESULT_GPU: u64 = 0x1_0084_0000;

    fn opening(device_id: u16) -> ([u32; 64], usize) {
        let mut batch = [0u32; 64];
        let bytes = encode_native_secondary_opening(
            &mut batch[RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS..], device_id, RESULT_GPU,
        ).unwrap();
        (batch, bytes)
    }

    #[test]
    fn production_opening_and_breadcrumb_validator_agree_for_each_generation() {
        for (device_id, invalidate_header) in [
            (0x4680, 0x7A00_0404), (0x5690, 0x7A00_0404), (0x1912, 0x7A00_0004),
        ] {
            let (mut batch, bytes) = opening(device_id);
            let prefix = RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS;
            assert_eq!(bytes, 17 * 4);
            assert_eq!(&batch[prefix..prefix + 6],
                &[0x7A00_0204, 0x0010_10A0, 0, 0, 0, 0]);
            assert_eq!(&batch[prefix + 6..prefix + 12],
                &[invalidate_header, 0x0014_0C1C, 0, 0, 0, 0]);
            let used = finish_resident_secondary_breadcrumbs(
                &mut batch, bytes, 2, RESULT_GPU, device_id,
            ).unwrap();
            assert_eq!(used, bytes + prefix * 4);
            let entry_gpu = RESULT_GPU + (RESULT_SLOT_BATCH_ENTRY_DWORD * 4) as u64;
            assert_eq!(&batch[..prefix], &[
                MI_STORE_DATA_IMM_GGTT_DW1, entry_gpu as u32, (entry_gpu >> 32) as u32,
                resident_secondary_marker(RCS_EXEC_RESULT_DRAW_BATCH_ENTRY, 2).unwrap(),
            ]);
            let marker = prefix + RESIDENT_SECONDARY_POST_OPENING_MARKER_DWORD;
            let post_gpu = RESULT_GPU + (RESULT_SLOT_POST_OPENING_DWORD * 4) as u64;
            assert_eq!(&batch[marker..marker + 4], &[
                MI_STORE_DATA_IMM_GGTT_DW1, post_gpu as u32, (post_gpu >> 32) as u32,
                resident_secondary_marker(RCS_EXEC_RESULT_DRAW_POST_OPENING, 2).unwrap(),
            ]);
        }
    }

    #[test]
    fn validator_rejects_unrelated_header_bits_without_rewriting_breadcrumbs() {
        for device_id in [0x4680, 0x5690, 0x1912] {
            for packet_offset in [0, RESIDENT_SECONDARY_OPENING_PIPE_CONTROL_DWORDS] {
                let (mut batch, bytes) = opening(device_id);
                batch[RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS + packet_offset] |= 1 << 8;
                let before = batch;
                assert_eq!(finish_resident_secondary_breadcrumbs(
                    &mut batch, bytes, 0, RESULT_GPU, device_id,
                ), Err("scene-frame-secondary-opening-layout"));
                assert_eq!(batch, before);
            }
        }
    }

    #[test]
    fn validator_checks_flags_addresses_and_immediate_payload_of_both_controls() {
        for device_id in [0x4680, 0x5690, 0x1912] {
            for packet_offset in [0, RESIDENT_SECONDARY_OPENING_PIPE_CONTROL_DWORDS] {
                for dword in 1..6 {
                    let (mut batch, bytes) = opening(device_id);
                    batch[RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS + packet_offset + dword] ^= 1;
                    let before = batch;
                    assert_eq!(finish_resident_secondary_breadcrumbs(
                        &mut batch, bytes, 0, RESULT_GPU, device_id,
                    ), Err("scene-frame-secondary-opening-layout"));
                    assert_eq!(batch, before);
                }
            }
        }
    }

    #[test]
    fn validator_rejects_an_opening_encoded_for_the_wrong_generation() {
        for (encoded_device, validated_device) in [
            (0x4680, 0x1912), (0x5690, 0x1912), (0x1912, 0x4680), (0x1912, 0x5690),
        ] {
            let (mut batch, bytes) = opening(encoded_device);
            let before = batch;
            assert_eq!(finish_resident_secondary_breadcrumbs(
                &mut batch, bytes, 0, RESULT_GPU, validated_device,
            ), Err("scene-frame-secondary-opening-layout"));
            assert_eq!(batch, before);
        }
    }
}
"""


def harness_source(secondary_finisher: str | None = None) -> str:
    primary = "src/intel/render/primary.rs"
    constants = "src/intel/render/constants.rs"
    names = [
        "GPU_VA_BATCH_BASE", "RESIDENT_SCENE_PRIMARY_BATCH_BYTES",
        "RESIDENT_SCENE_SECONDARY_BATCH_BYTES", "MI_BATCH_BUFFER_START_GEN8",
        "MI_BATCH_2ND_LEVEL", "MI_BATCH_PPGTT", "MI_STORE_DATA_IMM_GGTT_DW1",
        "MI_BATCH_BUFFER_END", "MI_NOOP", "RESULT_SLOT_SCENE_FRAME_DWORD",
        "RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_LO", "RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_HI",
        "PIPE_CONTROL_CMD", "PIPE_CONTROL_CS_STALL", "PIPE_CONTROL_STALL_AT_SCOREBOARD",
        "PIPE_CONTROL_SCENE_COLOR_RELEASE_HEADER_BITS", "PIPE_CONTROL_SCENE_COLOR_RELEASE_BITS",
        "PIPE_CONTROL_SCENE_RELEASE_MARKER_BITS", "PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER",
        "PIPE_CONTROL_DEPTH_CACHE_FLUSH", "PIPE_CONTROL_DC_FLUSH_ENABLE",
        "PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH", "PIPE_CONTROL_DEPTH_STALL",
        "PIPE_CONTROL_TILE_CACHE_FLUSH", "PIPE_CONTROL_FLUSH_ENABLE",
        "PIPE_CONTROL_L3_FABRIC_FLUSH", "PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE",
        "PIPE_CONTROL_L3_READ_ONLY_CACHE_INVALIDATE_HEADER", "PIPE_CONTROL_VF_CACHE_INVALIDATE",
        "PIPE_CONTROL_TLB_INVALIDATE", "PIPE_CONTROL_FLUSH_BITS", "PIPE_CONTROL_INVALIDATE_BITS",
        "PIPELINE_SELECT_3D", "RESULT_SLOT_BATCH_ENTRY_DWORD", "RCS_EXEC_RESULT_DRAW_BATCH_ENTRY",
        "RESIDENT_SCENE_MAX_DRAWS", "RESULT_OA_BEGIN_DWORD",
    ]
    declarations = [constant(constants, name) for name in names]
    declarations += [constant(primary, name) for name in (
        "RESULT_SLOT_SECONDARY_RETURN_DWORD", "RCS_EXEC_RESULT_SECONDARY_RETURN_BASE",
        "PICASSO_PIPELINE_STATS_BEGIN_DWORD", "PICASSO_PIPELINE_STATS_END_DWORD",
        "PICASSO_PIPELINE_STATS_LIMIT_DWORD", "PICASSO_PIPELINE_STAT_REGISTERS",
        "RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS", "RESIDENT_SECONDARY_OPENING_PIPE_CONTROL_DWORDS",
        "RESIDENT_SECONDARY_POST_OPENING_MARKER_DWORD", "RESULT_SLOT_POST_OPENING_DWORD",
        "RCS_EXEC_RESULT_DRAW_POST_OPENING",
    )]
    declarations.append(constant("src/intel/render/pipeline.rs", "RESULT_SLOT_DEPTH_STATE_WA_DWORD"))
    declarations += [item(primary, name) for name in (
        "emit_picasso_pipeline_stat_snapshot", "picasso_pipeline_stats_delta",
        "encode_resident_scene_primary_commands", "picasso_pipeline_stats_tests",
        "resident_secondary_marker",
    )]
    declarations += [item("src/intel/render/submit.rs", name) for name in (
        "device_is_gfx125", "device_is_gfx12",
    )]
    declarations.append(item("src/intel/render/pipeline.rs", "render_pipe_control_packet"))
    declarations.append(secondary_finisher if secondary_finisher is not None else
                        item(primary, "finish_resident_secondary_breadcrumbs"))
    declarations += [native_secondary_opening_source(), SECONDARY_OPENING_TESTS]
    stats = ROOT / "src/intel/stats.rs"
    source = '#![allow(dead_code)]\nmod intel { pub mod stats { include!("' + str(stats) + '"); } }\n'
    source += "\n".join(declarations)
    return source


def main() -> None:
    source = harness_source()
    with tempfile.TemporaryDirectory(prefix="trueos-picasso-stats-tests-") as temporary:
        directory = Path(temporary)
        rust_source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        rust_source.write_text(source)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(rust_source), "-o", str(executable)],
            cwd=ROOT, check=True,
        )
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
