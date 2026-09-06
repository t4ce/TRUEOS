#!/usr/bin/env python3
"""Exercise production ShaderToy row planning, payloads and retirement ordering."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main():
    operations = "src/intel/gpgpu/operations/shadertoy.rs"
    constants = "src/intel/gpgpu/rcs/constants.rs"
    declarations = [constant(operations, name) for name in (
        "SHADERTOY_DISPATCH_MAX_PIXELS", "SHADERTOY_LIGHT_DISPATCH_MAX_PIXELS",
        "SHADERTOY_SHADER_NGUYEN", "SHADERTOY_SHADER_PROTEAN_CLOUDS",
    )]
    declarations += [constant(constants, name) for name in (
        "SHADERTOY_POST_MARKER", "SHADERTOY_CROSS_THREAD_BYTES",
        "SHADERTOY_PER_THREAD_BYTES", "SHADERTOY_INDIRECT_BYTES",
        "SHADERTOY_PAYLOAD_OFFSET_BYTES", "SHADERTOY_UNIFORMS_OFFSET_BYTES",
        "SHADERTOY_UNIFORMS_BYTES", "DIRECT_RCS_BATCH_BYTES",
        "DIRECT_RCS_GPU_VA_BATCH_BASE",
    )]
    declarations += [item(operations, name) for name in (
        "ShaderToyFrameParams", "shadertoy_dispatch_rows", "shadertoy_rgba8_surface_full",
    )]
    declarations.append(item("src/intel/gpgpu/rcs/shadertoy.rs", "direct_rcs_write_shadertoy_payload"))
    source = "#![allow(dead_code)]\n" + "\n".join(declarations) + HARNESS
    with tempfile.TemporaryDirectory(prefix="trueos-shadertoy-dispatch-") as temporary:
        directory = Path(temporary)
        path = directory / "tests.rs"
        path.write_text(source)
        executable = directory / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(executable)],
                       cwd=ROOT, check=True)
        subprocess.run([str(executable)], check=True)


HARNESS = r'''
#[derive(Clone, Copy)]
struct GpgpuRgba8Surface { width: u32, height: u32, gpu: u64, pitch_bytes: u32 }
impl GpgpuRgba8Surface { fn is_valid(self) -> bool { self.width > 0 && self.height > 0 } }
impl ShaderToyFrameParams { fn is_valid(self) -> bool { self.shader_id == 6 } }
#[derive(Default, Debug)]
struct GpgpuRgba8KernelResult {
    ok: bool, submitted: bool, marker: u32, submit_ms: u64, release: Option<u32>,
}
struct DirectRcsDispatchOutcome { submitted: bool, observed: u32 }
#[derive(Default)]
struct Trace { rows: Vec<(u32, u32)>, failure: Option<(usize, bool)>, releases: usize }
thread_local! { static TRACE: std::cell::RefCell<Trace> = Default::default(); }
fn direct_rcs_now_tick() -> u64 { 0 }
fn direct_rcs_elapsed_ms_since(_: u64) -> u64 { 1 }
fn gpgpu_rgba8_release(_: GpgpuRgba8Surface) -> u32 {
    TRACE.with_borrow_mut(|t| t.releases += 1); 1
}
fn submit_shadertoy_rgba8_rows(_: GpgpuRgba8Surface, _: ShaderToyFrameParams,
                             row: u32, rows: u32) -> DirectRcsDispatchOutcome {
    TRACE.with_borrow_mut(|t| {
        let index = t.rows.len();
        t.rows.push((row, rows));
        if let Some((at, submitted)) = t.failure && at == index {
            DirectRcsDispatchOutcome { submitted, observed: 0 }
        } else {
            DirectRcsDispatchOutcome { submitted: true, observed: SHADERTOY_POST_MARKER }
        }
    })
}
fn surface(width: u32, height: u32) -> GpgpuRgba8Surface {
    GpgpuRgba8Surface { width, height, gpu: 0x12345000, pitch_bytes: width * 4 + 64 }
}
fn params() -> ShaderToyFrameParams {
    ShaderToyFrameParams { shader_id: 6, time_seconds: 8., mouse_x: 200., mouse_y: 123.,
                           ..Default::default() }
}

#[test]
fn all_rows_once_and_release_only_after_last_retirement() {
    for (width, height) in [(1, 1), (15, 17), (16, 16), (17, 19), (360, 640),
                            (640, 360), (2560, 1440), (3840, 2160)] {
        TRACE.with_borrow_mut(|t| *t = Trace::default());
        let result = shadertoy_rgba8_surface_full(surface(width, height), params());
        assert!(result.ok && result.submitted && result.release.is_some());
        TRACE.with_borrow(|t| {
            let mut next = 0;
            for &(first, rows) in &t.rows {
                assert_eq!(first, next);
                assert!(rows > 0);
                assert!(u64::from(width).div_ceil(16) * 16 * u64::from(rows)
                        <= SHADERTOY_DISPATCH_MAX_PIXELS);
                next += rows;
            }
            assert_eq!(next, height);
            assert_eq!(t.releases, 1);
            if width == 2560 { assert_eq!(t.rows.len(), 29); }
        });
    }
}

#[test]
fn incomplete_or_rejected_batch_stops_without_publishing_partial_frame() {
    for submitted in [false, true] {
        TRACE.with_borrow_mut(|t| *t = Trace { failure: Some((2, submitted)), ..Default::default() });
        let result = shadertoy_rgba8_surface_full(surface(2560, 1440), params());
        assert!(!result.ok && result.release.is_none());
        // Retired earlier rows must not turn a pre-submit rejection into an
        // unretired submission; accepted failures must retain that distinction.
        assert_eq!(result.submitted, submitted);
        TRACE.with_borrow(|t| { assert_eq!(t.rows.len(), 3); assert_eq!(t.releases, 0); });
    }
}

#[test]
fn invalid_extents_and_end_rows_never_dispatch() {
    assert_eq!(shadertoy_dispatch_rows(1, 2560, 1440, 0), Some(409));
    assert_eq!(shadertoy_dispatch_rows(3, 2560, 1440, 0), Some(51));
    assert_eq!(shadertoy_dispatch_rows(6, 0, 10, 0), None);
    assert_eq!(shadertoy_dispatch_rows(6, 16, 0, 0), None);
    assert_eq!(shadertoy_dispatch_rows(6, 16, 10, 10), None);
    assert_eq!(shadertoy_dispatch_rows(6, u32::MAX, 10, 0), None);
    assert_eq!(shadertoy_dispatch_rows(6, 16, u32::MAX, u32::MAX - 1), Some(1));
}

#[derive(Clone, Copy)]
struct DirectRcsState { batch_virt: *mut u8 }
struct Contract { cross_thread_data_bytes: u32, per_thread_data_bytes: u32 }
fn shadertoy_contract(_: u32) -> Option<Contract> {
    Some(Contract { cross_thread_data_bytes: 96, per_thread_data_bytes: 96 })
}

#[test]
fn real_payload_offsets_only_global_y_and_preserves_full_image_inputs() {
    let mut initial = vec![0u32; DIRECT_RCS_BATCH_BYTES / 4];
    let mut later = initial.clone();
    let dst = surface(2560, 1440);
    assert!(direct_rcs_write_shadertoy_payload(
        DirectRcsState { batch_virt: initial.as_mut_ptr().cast() }, dst, params(), 0));
    assert!(direct_rcs_write_shadertoy_payload(
        DirectRcsState { batch_virt: later.as_mut_ptr().cast() }, dst, params(), 51));
    let differences: Vec<_> = initial.iter().zip(&later).enumerate()
        .filter(|(_, (a, b))| a != b).map(|(i, _)| i).collect();
    assert_eq!(differences, [SHADERTOY_PAYLOAD_OFFSET_BYTES / 4 + 1]);
    assert_eq!(later[differences[0]], 51);
    let payload = SHADERTOY_PAYLOAD_OFFSET_BYTES / 4;
    assert_eq!(&later[payload + 16..payload + 19], &[2560, 1440, dst.pitch_bytes]);
    let uniforms = SHADERTOY_UNIFORMS_OFFSET_BYTES / 4;
    assert_eq!(later[uniforms], 2560f32.to_bits());
    assert_eq!(later[uniforms + 1], 1440f32.to_bits());
    assert_eq!(later[uniforms + 4], 200f32.to_bits());
}
'''

if __name__ == "__main__":
    main()
