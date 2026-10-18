#!/usr/bin/env python3
"""Host-test the real Dista upload builder, shader composition, and seed guard.

The tiny temporary Cargo project links the current Microfont raster library.
It executes production Rust rather than translating the upload builder to Python.
No GPU submission is performed by this test.
"""

from pathlib import Path
import importlib.util
import json
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


ROOT = Path(__file__).resolve().parents[2]


TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    use tasks::{Quality, TEXT, build};

    #[test]
    fn descriptors_cover_the_requested_two_rows_without_uploading_positions() {
        assert_eq!((TEXT, tasks::WIDTH, tasks::HEIGHT), ("ABC123/Yy%", 960, 352));
        let expected_counts = [(Draft, [3036, 2460]), (Fine, [3612, 3036]), (High, [4764, 3612])];
        for (quality, expected) in expected_counts {
            let result = build(quality);
            assert_eq!(result.row_candidates, expected);
            assert_eq!(result.indices.len(), expected.into_iter().sum::<usize>() * 3);
            assert!(result.indices.iter().all(|&index| (index as usize) < result.descriptors.len()));
            let mut referenced = vec![false; result.descriptors.len()];
            for &index in &result.indices { referenced[index as usize] = true; }
            assert!(referenced.into_iter().all(|used| used));
            let mut stamps = std::collections::BTreeSet::new();
            let mut descriptor_keys = std::collections::BTreeSet::new();
            let mut circles = [[0usize; 10]; 2];
            let mut links = [[0usize; 10]; 2];
            for &[token, stamp, segments] in &result.descriptors {
                assert!(token.is_finite() && stamp.is_finite() && segments.is_finite());
                for value in [token, stamp, segments] { assert_eq!(value, value as u32 as f32); }
                let (token, packed, segments) = (token as u32, stamp as u32, segments as u32);
                let (code, column, row) = (packed & 127, (packed >> 7) & 15, (packed >> 11) & 1);
                assert_eq!(packed >> 12, 0);
                assert!((column as usize) < TEXT.len());
                assert_eq!(code as u8, TEXT.as_bytes()[column as usize]);
                let shape = [FontShape::Meso, FontShape::Lean][row as usize];
                assert_eq!(segments as usize, quality.segments(shape));
                let disc_end = 66 * (segments + 1);
                let (site, offset) = if token < disc_end {
                    circles[row as usize][column as usize] += 1;
                    (token / (segments + 1), None)
                } else {
                    assert!(token < disc_end + 66 * 10 * 4);
                    links[row as usize][column as usize] += 1;
                    let candidate = (token - disc_end) / 4;
                    (candidate / 10, Some(candidate % 10))
                };
                let seed = microfont_real::raster::basic_seed(code as u8 as char).unwrap();
                assert_ne!(seed.pixels & (1u128 << (65 - site)), 0);
                if let Some(offset) = offset { assert!(offset < if seed.long_reach { 10 } else { 4 }); }
                assert!(descriptor_keys.insert((packed, token)), "duplicate vertex task");
                stamps.insert((row, column));
            }
            assert_eq!(stamps.len(), 20);
            for row in 0..2 {
                let segments = quality.segments([FontShape::Meso, FontShape::Lean][row]);
                for (column, ch) in TEXT.chars().enumerate() {
                    let seed = microfont_real::raster::basic_seed(ch).unwrap();
                    let sites = seed.pixels.count_ones() as usize;
                    assert_eq!(circles[row][column], sites * (segments + 1));
                    assert_eq!(links[row][column], sites * if seed.long_reach { 40 } else { 16 });
                }
            }
        }
    }

    #[test]
    fn atlas_covers_all_printable_cells_with_independent_counts_and_disjoint_abi_bits() {
        use tasks::{ATLAS_WIDTH, ATLAS_HEIGHT, GLYPH_WIDTH, GLYPH_HEIGHT, build_atlas};
        assert_eq!((ATLAS_WIDTH, ATLAS_HEIGHT), (1536, 1056));
        assert_eq!(ATLAS_AUDIT_SITES.len(), 95); // U+0020..U+007E, including blank space.
        assert_eq!(ATLAS_AUDIT_SITES[0], 0);
        for (quality_index, quality) in [Draft, Fine, High].into_iter().enumerate() {
            let mut all_variant_stamps = std::collections::BTreeSet::new();
            for (shape_row, shape) in [FontShape::Meso, FontShape::Lean].into_iter().enumerate() {
                let baseline = build_atlas(quality, shape, false);
                let direct = build_atlas(quality, shape, true);
                assert_eq!(baseline.indices, direct.indices);
                assert_eq!(baseline.row_candidates, direct.row_candidates);
                assert_eq!(baseline.descriptors.len(), direct.descriptors.len());
                for (a, b) in baseline.descriptors.iter().zip(&direct.descriptors) {
                    assert_eq!((a[0], a[2]), (b[0], b[2]));
                    assert_eq!((a[1] as u32) ^ (b[1] as u32), 1 << 16);
                }
                let segments = ATLAS_AUDIT_SEGMENTS[quality_index][shape_row];
                assert_eq!(quality.segments(shape), segments);
                let mut expected_rows = [0; 2];
                expected_rows[shape_row] = ATLAS_AUDIT_TRIANGLES[quality_index][shape_row];
                for (variant, result) in [baseline, direct].into_iter().enumerate() {
                    assert_eq!(result.row_candidates, expected_rows);
                    assert_eq!(result.indices.len(), expected_rows[shape_row] * 3);
                    let mut referenced = vec![false; result.descriptors.len()];
                    for &index in &result.indices {
                        *referenced.get_mut(index as usize).expect("atlas index in bounds") = true;
                    }
                    assert!(referenced.into_iter().all(|used| used));
                    let mut circles = [0usize; 95];
                    let mut links = [0usize; 95];
                    let mut stamps = std::collections::BTreeSet::new();
                    let mut descriptor_keys = std::collections::BTreeSet::new();
                    for &[token, packed, n] in &result.descriptors {
                        for value in [token, packed, n] {
                            assert!(value.is_finite());
                            assert_eq!(value, value as u32 as f32);
                        }
                        let (token, packed) = (token as u32, packed as u32);
                        let code = packed & 127;
                        assert!((0x21..=0x7e).contains(&code));
                        let slot = (code - 0x20) as usize;
                        let column = (packed >> 7) & 15;
                        let row = (packed >> 11) & 1;
                        let grid_row = (packed >> 12) & 7;
                        assert_eq!((column as usize, grid_row as usize), (slot % 16, slot / 16));
                        assert_eq!(row as usize, shape_row); // Shape belongs only to bit 11.
                        assert_eq!((packed >> 15) & 1, 1); // Atlas mode belongs only to bit 15.
                        assert_eq!((packed >> 16) & 1, variant as u32);
                        assert_eq!(packed >> 17, 0);
                        assert_eq!(packed, code | column << 7 | row << 11 | grid_row << 12
                            | 1 << 15 | (variant as u32) << 16);
                        assert_eq!(n as usize, segments);
                        // Every decoded glyph cell lies inside the exact shader target.
                        assert!((column + 1) * GLYPH_WIDTH <= ATLAS_WIDTH);
                        assert!((grid_row + 1) * GLYPH_HEIGHT <= ATLAS_HEIGHT);
                        let disc_end = 66 * (segments as u32 + 1);
                        let site = if token < disc_end {
                            circles[slot] += 1;
                            token / (segments as u32 + 1)
                        } else {
                            links[slot] += 1;
                            let candidate = (token - disc_end) / 4;
                            assert!(candidate < 66 * 10);
                            assert!(candidate % 10 < if ATLAS_AUDIT_LONG_REACH[slot] { 10 } else { 4 });
                            candidate / 10
                        };
                        let seed = microfont_real::raster::basic_seed(code as u8 as char).unwrap();
                        assert_ne!(seed.pixels & (1u128 << (65 - site)), 0);
                        assert!(descriptor_keys.insert((packed, token)), "duplicate atlas task");
                        stamps.insert(packed);
                    }
                    assert_eq!(stamps.len(), 94); // Blank space owns its slot, with zero tasks.
                    for packed in stamps { assert!(all_variant_stamps.insert(packed)); }
                    for slot in 0..95 {
                        let sites = ATLAS_AUDIT_SITES[slot];
                        assert_eq!(circles[slot], sites * (segments + 1));
                        assert_eq!(links[slot], sites * if ATLAS_AUDIT_LONG_REACH[slot] { 40 } else { 16 });
                        if slot > 0 { assert!(circles[slot] > 0); }
                    }
                }
            }
            // Shape and variant bits never alias another glyph's address.
            assert_eq!(all_variant_stamps.len(), 2 * 2 * 94);
        }
    }

    use Quality::{Draft, Fine, High};

    fn source_position([token, _stamp, segments]: [f32; 3]) -> [f64; 2] {
        let (token, segments) = (token as usize, segments as usize);
        let disc_end = 66 * (segments + 1);
        if token < disc_end {
            let (site, ring) = (token / (segments + 1), token % (segments + 1));
            let center = [(site % 6) as f64, (site / 6) as f64];
            if ring == 0 { return center; }
            let angle = std::f64::consts::TAU * (ring - 1) as f64 / segments as f64;
            [center[0] + angle.cos(), center[1] + angle.sin()]
        } else {
            let vertex = token - disc_end;
            let (site, offset, corner) = (vertex / 40, (vertex / 4) % 10, vertex % 4);
            let [dx, dy] = SHADER_LINK_OFFSETS[offset].map(f64::from);
            let (ax, ay) = ((site % 6) as f64, (site / 6) as f64);
            let cap = if corner < 2 { [ax, ay] } else { [ax + dx, ay + dy] };
            let side = if corner == 0 || corner == 3 { 1.0 } else { -1.0 };
            [cap[0] - dy * side, cap[1] + dx * side]
        }
    }

    #[test]
    fn every_circle_and_link_triangle_has_positive_clip_space_winding() {
        for quality in [Draft, Fine, High] {
            let result = build(quality);
            for triangle in result.indices.chunks_exact(3) {
                let [a, b, c] = std::array::from_fn::<_, 3, _>(|i| source_position(result.descriptors[triangle[i] as usize]));
                let source_area = (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0]);
                // GLSL maps source Y down to clip Y up, reversing its sign.
                assert!(source_area < 0.0, "bad winding for {triangle:?}: {source_area}");
            }
        }
    }

    #[test]
    fn quality_presets_bound_circle_error_at_the_actual_display_scale() {
        let mut previous = [usize::MAX; 2];
        for quality in [High, Fine, Draft] {
            let limit = f64::from(quality.error_micropixels()) / 1_000_000.0;
            for (row, shape) in [FontShape::Meso, FontShape::Lean].into_iter().enumerate() {
                let radius_px = FontShape::INFLUENCE_WIDTH * (-2.0 * shape.body_threshold().ln()).sqrt() * 16.0;
                let segments = quality.segments(shape);
                let error = radius_px * (1.0 - (std::f64::consts::PI / segments as f64).cos());
                assert!(error <= limit, "quality bound exceeded: {error} > {limit}");
                assert!(segments <= previous[row]);
                previous[row] = segments;
            }
        }
    }

    #[test]
    fn seed_guard_detects_both_changed_sites_and_changed_reach() {
        assert!(intel::shader::microfont_dista_seeds_current());
        for mutation in [1, 2] {
            SEED_MUTATION.with(|value| value.set(mutation));
            assert!(!intel::shader::microfont_dista_seeds_current());
        }
        SEED_MUTATION.with(|value| value.set(0));
        assert!(intel::shader::microfont_dista_seeds_current());
    }

    #[test]
    fn native_pipeline_admits_only_its_float3_task_layout_and_reuses_color_program() {
        use intel::{shader, render::*};
        let pipeline = resident_scene_shader_pipeline(ResidentSceneFragmentContract::MicrofontDista,
            false, TriangleVertexFormat::Float3, 12).unwrap();
        assert!(core::ptr::eq(pipeline, shader::microfont_dista_pipeline()));
        assert!(core::ptr::eq(pipeline.ps.code, shader::triangle_pipeline_simd16().ps.code));
        assert_ne!(pipeline.vs.code, shader::triangle_pipeline_simd16().vs.code);
        assert_eq!((pipeline.vs.meta.kernel.grf_start_register, pipeline.vs.meta.urb_entry_output_length), (2, 1));
        assert_eq!(pipeline.vs.meta.kernel.dispatch_mode, shader::DispatchMode::Simd8);
        assert_eq!((pipeline.vs.meta.kernel.push_constant_bytes, pipeline.vs.meta.kernel.binding_table_entry_count,
            pipeline.vs.meta.kernel.sampler_count), (0, 0, 0));
        assert_eq!(pipeline.ps.meta.kernel.dispatch_mode, shader::DispatchMode::Simd16);
        assert_eq!(pipeline.ps.meta.num_varying_inputs, 0);
        assert_eq!(pipeline.ps.meta.kernel.code_offset_bytes % 64, 0);
        assert!(pipeline.ps.meta.kernel.code_offset_bytes >= pipeline.vs.meta.kernel.code_size_bytes);
        assert_eq!(pipeline.vs.code.len() * 4, pipeline.vs.meta.kernel.code_size_bytes as usize);
        for (texture, format, stride) in [(true, TriangleVertexFormat::Float3, 12),
            (false, TriangleVertexFormat::Float2, 12), (false, TriangleVertexFormat::PosUv, 20),
            (false, TriangleVertexFormat::Float3, 0), (false, TriangleVertexFormat::Float3, 16)] {
            assert!(resident_scene_shader_pipeline(ResidentSceneFragmentContract::MicrofontDista,
                texture, format, stride).is_err());
        }
    }
}
'''


def harness_source() -> str:
    shader = "src/intel/shader.rs"
    primary = "src/intel/render/primary.rs"
    types = [item(shader, name) for name in (
        "DispatchMode", "ShaderKernelMetadata", "TriangleVertexShaderMetadata",
        "TrianglePixelShaderMetadata", "TriangleVertexShader", "TrianglePixelShader", "TrianglePipeline",
    )]
    functions = [item(shader, name) for name in (
        "microfont_dista_pipeline", "microfont_dista_seeds_current",
    )]
    selection = [item("src/intel/render/state.rs", "TriangleVertexFormat")]
    selection += [item(primary, name) for name in (
        "ResidentSceneFragmentContract", "resident_scene_shader_pipeline",
    )]
    # The other render contracts are not exercised here. Their functions only
    # provide branch targets needed to type-check the actual selector.
    other = "\n".join(
        f"pub(crate) fn {name}() -> &'static TrianglePipeline {{ triangle_pipeline_simd16() }}"
        for name in ("clip_position3_uv_texture_pipeline", "voxy_headless_pipeline",
                     "voxy_headless_texture_pipeline", "wc3_fixed_pipeline")
    )
    other += "\n" + "\n".join(
        f"pub(crate) fn {name}() -> Result<&'static TrianglePipeline, &'static str> {{ Ok(triangle_pipeline_simd16()) }}"
        for name in ("voxy_flat_cloud_pipeline", "voxy_figure_pipeline")
    )
    # Inject seed mutations at the library boundary, while all unmodified
    # lookups execute the real Microfont library. Thread-local mutation keeps
    # the stale-guard fault test isolated from concurrent task tests.
    header = r'''
#![allow(dead_code, unused_imports, unfulfilled_lint_expectations)]
extern crate alloc;
extern crate microfont_real;
extern crate self as microfont;
pub use microfont_real::{FontShape, FontDetail};
thread_local! { static SEED_MUTATION: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }
pub mod raster {
    pub use microfont_real::raster::{SHORT_CONNECTION_REACH, LONG_CONNECTION_REACH};
    pub fn basic_seed(character: char) -> Option<microfont_real::raster::GlyphSeed> {
        let mut seed = microfont_real::raster::basic_seed(character)?;
        if character == 'A' {
            crate::SEED_MUTATION.with(|value| match value.get() {
                1 => seed.pixels ^= 1,
                2 => seed.long_reach = !seed.long_reach,
                _ => {},
            });
        }
        Some(seed)
    }
}
'''
    spec = importlib.util.spec_from_file_location("dista_audit", ROOT / "tools/microfont-dista-bake/audit.py")
    module = importlib.util.module_from_spec(spec)
    # dataclasses resolve their module during decoration.
    import sys
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    offsets = module.load_shader_offsets()
    header += "const SHADER_LINK_OFFSETS: [[i32; 2]; 10] = " + json.dumps(offsets) + ";\n"
    # Independent Python oracle reads the Rust seed data and analytically
    # derives segment counts from the requested error. No builder totals or
    # expected triangle counts are copied from the production Rust code.
    atlas = "".join(chr(code) for code in range(0x20, 0x7f))
    audits = [module.audit(atlas, error_px=error) for error in (0.25, 0.125, 0.0625)]
    header += "const ATLAS_AUDIT_SITES: [usize; 95] = " + json.dumps(
        [glyph["sites"] for glyph in audits[0]["glyphs"]]) + ";\n"
    header += "const ATLAS_AUDIT_LONG_REACH: [bool; 95] = " + json.dumps(
        [glyph["long_reach"] for glyph in audits[0]["glyphs"]]) + ";\n"
    for name, field in [("SEGMENTS", "circle_segments"), ("TRIANGLES", "submitted_triangles")]:
        values = [[audit["shapes"][shape][field] for shape in ("Meso", "Lean")] for audit in audits]
        header += f"const ATLAS_AUDIT_{name}: [[usize; 2]; 3] = " + json.dumps(values) + ";\n"
    header += f'#[path = "{ROOT / "src/r/services/microfont_dista_tasks.rs"}"]\nmod tasks;\n'
    body = "mod intel { pub(crate) mod shader {\n" + "\n".join(types)
    body += f'\n#[path = "{ROOT / "crates/trueos-shader/generated_triangle.rs"}"] mod triangle;\n'
    body += "pub(crate) use triangle::{triangle_pipeline, triangle_pipeline_simd16};\n"
    body += f'#[path = "{ROOT / "crates/trueos-shader/generated_microfont_dista.rs"}"] mod microfont_dista;\n'
    body += "\n".join(functions) + "\n" + other
    body += "\n} pub(crate) mod render {\n" + "\n".join(selection) + "\n}}\n"
    # The production selector is private within render; its tests are emitted
    # in that module, with access to the common fixture imports and task data.
    tests = TESTS.replace("use intel::{shader, render::*};", "use crate::intel::{shader, render::*};")
    body = body.replace("\n}}\n", "\n" + tests + "\n}}\n")
    # Test code uses crate-level fixture/task names through its parent module.
    body = body.replace("pub(crate) mod render {\n", "pub(crate) mod render {\nuse crate::*;\n")
    return header + body


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-dista-tasks-") as temporary:
        directory = Path(temporary)
        (directory / "src").mkdir()
        (directory / "src/lib.rs").write_text(harness_source())
        (directory / "Cargo.toml").write_text(
            '[package]\nname="trueos-dista-task-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[features]\nmicrofont_dista_bench=[]\n'
            '[dependencies]\n'
            f'microfont_real={{package="microfont",path={json.dumps(str(ROOT / "vendor/microfont"))},default-features=false,features=["raster"]}}\n'
            'spin={version="=0.10.1",default-features=false,features=["lazy"]}\n'
        )
        subprocess.run(
            ["cargo", "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
             "--target", "x86_64-unknown-linux-gnu", "--features", "microfont_dista_bench",
             "--quiet"], cwd=directory, check=True,
        )


if __name__ == "__main__":
    main()
