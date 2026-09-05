#!/usr/bin/env python3
"""Host-test production V3 decoding and retained draw-range admission.

Optionally pass QuadTexture's generated tile_scene.json to verify the actual
persisted TRS rows through the kernel decoder and draw-template builder.
"""

from pathlib import Path
import json
import re
import subprocess
import sys
import tempfile

from test_clip_position3_uv_texture import ROOT, item, constant
from test_retained_material import harness_source


def source() -> str:
    abi = "crates/trueos-v/src/vgpu.rs"
    sdk = "../TRUEOS-Blueprints/crates/trueos-v/src/vgpu.rs"
    for name in ("RetainedDrawRange", "RetainedFrameSubmitV3", "RetainedTransformSeed"):
        assert item(abi, name) == item(sdk, name), f"SDK mirror drift: {name}"
    for name in ("MAX_RETAINED_SCENE_INSTANCES", "MAX_RETAINED_SCENE_DRAWS"):
        assert constant(abi, name) == constant(sdk, name), f"SDK mirror drift: {name}"
    for path in ("crates/trueos-vm/src/vmcall.rs", "src/hv/vmcall.rs"):
        assert re.search(r"OP_BP_VGPU_RETAINED_FRAME_SUBMIT_V3:\s*u32\s*=\s*0x17A;", (ROOT / path).read_text())
    return harness_source() + "\n" + "\n".join((
        item("src/gpu/vgpu.rs", "retained_scene_descriptor_valid"),
        item("src/gpu/vgpu.rs", "decode_retained_scene_seeds"),
        item("src/intel/render/resources.rs", "picasso_retained_draw_templates"),
    )) + r'''
use vgpu::*;

fn descriptor() -> RetainedFrameSubmitV3 {
    RetainedFrameSubmitV3 {
        seed_buffer: 1, seed_count: 257, draw_count: 3,
        draws: [
            RetainedDrawRange { first_index: 0, index_count: 1812 },
            RetainedDrawRange { first_index: 1812, index_count: 636 },
            RetainedDrawRange { first_index: 2448, index_count: 36 },
            RetainedDrawRange::default(),
        ],
        ..RetainedFrameSubmitV3::default()
    }
}

#[test]
fn descriptor_rejects_overflow_oversize_and_ambiguous_inline_inputs() {
    let good = descriptor();
    assert!(retained_scene_descriptor_valid(&good));
    for seed_count in [1, MAX_RETAINED_SCENE_INSTANCES as u32] {
        assert!(retained_scene_descriptor_valid(&RetainedFrameSubmitV3 { seed_count, ..good }));
    }
    for seed_count in [0, MAX_RETAINED_SCENE_INSTANCES as u32 + 1, u32::MAX] {
        assert!(!retained_scene_descriptor_valid(&RetainedFrameSubmitV3 { seed_count, ..good }));
    }
    for draw_count in [0, MAX_RETAINED_SCENE_DRAWS as u32 + 1, u32::MAX] {
        assert!(!retained_scene_descriptor_valid(&RetainedFrameSubmitV3 { draw_count, ..good }));
    }
    for seed_offset in [1, 2, u64::MAX - 3] {
        assert!(!retained_scene_descriptor_valid(&RetainedFrameSubmitV3 { seed_offset, ..good }));
    }
    assert!(retained_scene_descriptor_valid(&RetainedFrameSubmitV3 { seed_offset: 64, ..good }));
    let mut bad = good;
    bad.seed_buffer = 0;
    assert!(!retained_scene_descriptor_valid(&bad));
    bad = good;
    bad.reserved[1] = 1;
    assert!(!retained_scene_descriptor_valid(&bad));
    bad = good;
    bad.frame.frame.seed_count = 1;
    assert!(!retained_scene_descriptor_valid(&bad));
    bad = good;
    bad.frame.frame.seeds[3].flags = 1;
    assert!(!retained_scene_descriptor_valid(&bad));
    bad = good;
    bad.draws[3].index_count = 3;
    assert!(!retained_scene_descriptor_valid(&bad));
}

#[test]
fn seed_decoder_checks_every_float_and_exact_row_boundaries() {
    let values: [f32; 14] = [1., 2., 3., 4., 5., 6., 0., 0., 0., 1., 7., 8., 9., 10.];
    let mut row = Vec::new();
    for value in values { row.extend_from_slice(&value.to_le_bytes()); }
    row.extend_from_slice(&2u32.to_le_bytes());
    row.extend_from_slice(&((59u32 << 16) | 4).to_le_bytes());
    let expected = RetainedTransformSeed {
        translation: [1., 2., 3.], scale: [4., 5., 6.], rotation: [0., 0., 0., 1.],
        local_radius: 7., previous_translation: [8., 9., 10.], draw_group: 2, flags: (59 << 16) | 4,
    };
    assert_eq!(decode_retained_scene_seeds(&row), Some(vec![expected]));
    assert_eq!(decode_retained_scene_seeds(&row.repeat(512)).unwrap().len(), 512);
    for bytes in [vec![], row[..63].to_vec(), row.repeat(513), [row.clone(), vec![0]].concat()] {
        assert!(decode_retained_scene_seeds(&bytes).is_none());
    }
    for field in 0..14 {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut bad = row.clone();
            bad[field * 4..field * 4 + 4].copy_from_slice(&invalid.to_le_bytes());
            assert!(decode_retained_scene_seeds(&bad).is_none(), "field={field}");
        }
    }
    row[40..44].copy_from_slice(&(-1.0f32).to_le_bytes());
    assert!(decode_retained_scene_seeds(&row).is_none());
}

#[test]
fn seed_decoder_rejects_rows_the_gpu_would_skip_but_allows_collapsed_scales() {
    let mut row = [0u8; 64];
    // The GPU accepts a zero scale and normalizes a non-unit quaternion.
    row[36..40].copy_from_slice(&2.0f32.to_le_bytes());
    assert!(decode_retained_scene_seeds(&row).is_some());
    for scale in 3..6 {
        let mut bad = row;
        bad[scale * 4..scale * 4 + 4].copy_from_slice(&(-0.01f32).to_le_bytes());
        assert!(decode_retained_scene_seeds(&bad).is_none());
    }
    for quaternion_w in [0.0f32, 0.000_000_1, f32::MAX] {
        let mut bad = row;
        bad[36..40].copy_from_slice(&quaternion_w.to_le_bytes());
        assert!(decode_retained_scene_seeds(&bad).is_none());
    }
}

fn slot(group: u32, index: u32) -> RetainedTransformSeed {
    RetainedTransformSeed { draw_group: group, flags: index << 16, ..RetainedTransformSeed::default() }
}

#[test]
fn interleaved_floor_rows_have_disjoint_compaction_ranges() {
    let mut seeds = vec![slot(0, 0)];
    let mut counts = [0; 3];
    for row in 0..16 {
        for col in 0..16 {
            let group = if row == 0 || col == 0 || row == 15 || col == 15 { 2 } else { 1 };
            seeds.push(slot(group as u32, counts[group]));
            counts[group] += 1;
        }
    }
    assert_eq!(picasso_retained_draw_templates(2484, &seeds, &descriptor().draws[..3]).unwrap(), [
        [1812, 0, 0, 0, 1, 0], [636, 1812, 0, 1, 196, 0], [36, 2448, 0, 197, 60, 0],
    ]);
}

#[test]
fn old_four_helmet_instances_keep_one_full_mesh_draw() {
    let seeds: Vec<_> = (0..4).map(|s| slot(0, s)).collect();
    let draws = [RetainedDrawRange { first_index: 0, index_count: 185424 }];
    assert_eq!(picasso_retained_draw_templates(185424, &seeds, &draws).unwrap(), [[185424, 0, 0, 0, 4, 0]]);
}

#[test]
fn draw_templates_reject_out_of_mesh_ranges_duplicate_slots_and_empty_groups() {
    let good = RetainedDrawRange { first_index: 0, index_count: 3 };
    for bad in [RetainedDrawRange::default(), RetainedDrawRange { first_index: 1, index_count: 3 },
                RetainedDrawRange { first_index: u32::MAX, index_count: 1 }] {
        assert!(picasso_retained_draw_templates(3, &[slot(0, 0)], &[bad]).is_err());
    }
    for seeds in [vec![], vec![slot(1, 0)], vec![slot(0, 1)], vec![slot(0, 0), slot(0, 0)],
                  vec![slot(0, 0), slot(0, 2)], (0..513).map(|s| slot(0, s)).collect()] {
        assert!(picasso_retained_draw_templates(3, &seeds, &[good]).is_err());
    }
    assert!(picasso_retained_draw_templates(3, &[slot(0, 0)], &[]).is_err());
    assert!(picasso_retained_draw_templates(3, &[slot(0, 0)], &[good; 5]).is_err());
    assert!(picasso_retained_draw_templates(3, &[slot(0, 0)], &[good; 2]).is_err());
    let seeds: Vec<_> = (0..512).map(|s| slot(s / 128, s % 128)).collect();
    let templates = picasso_retained_draw_templates(3, &seeds, &[good; 4]).unwrap();
    assert_eq!(templates.last().unwrap()[3..5], [384, 128]);
}
'''


def main() -> None:
    tests = source()
    if len(sys.argv) == 2:
        manifest = Path(sys.argv[1]).resolve()
        meta = json.loads(manifest.read_text())
        ranges = ",".join(f"RetainedDrawRange {{ first_index: {first}, index_count: {count} }}" for first, count in meta["draw_ranges"])
        tests += f'''
#[test]
fn persisted_quadtexture_floor_is_admitted_by_the_actual_kernel_helpers() {{
    let seeds = decode_retained_scene_seeds(include_bytes!({json.dumps(str(manifest.with_suffix('.seeds')))})).unwrap();
    assert_eq!(seeds.len(), 257);
    let templates = picasso_retained_draw_templates({meta['index_count']}, &seeds, &[{ranges}]).unwrap();
    assert_eq!(templates, [[1812, 0, 0, 0, 1, 0], [636, 1812, 0, 1, 196, 0], [36, 2448, 0, 197, 60, 0]]);
}}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-retained-scene-tests-") as temporary:
        directory = Path(temporary)
        rust_source, executable = directory / "tests.rs", directory / "tests"
        rust_source.write_text(tests)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust_source), "-o", str(executable)], cwd=ROOT, check=True)
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
