#!/usr/bin/env python3
"""Exercise production ShaderToy row planning, payloads and retirement ordering."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main():
    operations = "src/intel/gpgpu/operations/shadertoy.rs"
    constants = "src/intel/gpgpu/rcs/constants.rs"
    # Compile the real frame orchestration/cache/planner, mocking only GPU and
    # DMA calls. The actual six generated contracts drive payload assertions.
    declarations = [(ROOT / operations).read_text().split("\nfn submit_shadertoy_rgba8_rows")[0]]
    declarations += [(ROOT / "src/intel/gpgpu/operations/shadertoy_focus.rs").read_text()]
    declarations += [(ROOT / "src/intel/gpgpu/artifacts/contract.rs").read_text().split("#[cfg(test)]")[0]]
    for name in ("mandelbrot", "cube_field", "nguyen", "palette_grid", "cosmic_strands", "protean_clouds"):
        declarations.append((ROOT / f"crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/shadertoy_{name}.contract.rs").read_text())
    declarations += [constant(constants, name) for name in (
        "SHADERTOY_POST_MARKER", "SHADERTOY_CROSS_THREAD_BYTES",
        "SHADERTOY_PER_THREAD_BYTES", "SHADERTOY_INDIRECT_BYTES",
        "SHADERTOY_PAYLOAD_OFFSET_BYTES", "SHADERTOY_UNIFORMS_OFFSET_BYTES",
        "SHADERTOY_UNIFORMS_BYTES", "DIRECT_RCS_BATCH_BYTES", "DIRECT_RCS_GPU_VA_BATCH_BASE",
    )]
    declarations += [item("src/intel/gpgpu/rcs/shadertoy.rs", name) for name in (
        "shadertoy_contract", "shadertoy_payload_layout_matches", "direct_rcs_write_shadertoy_payload")]
    source = "#![allow(dead_code)]\n" + "\n".join(declarations) + HARNESS
    with tempfile.TemporaryDirectory(prefix="trueos-shadertoy-dispatch-") as temporary:
        directory = Path(temporary)
        path = directory / "tests.rs"
        path.write_text(source)
        executable = directory / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(executable)],
                       cwd=ROOT, check=True)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


HARNESS = r'''
#[derive(Clone, Copy, Debug)]
struct GpgpuRgba8Surface { width: u32, height: u32, gpu: u64, pitch_bytes: u32, bytes: usize }
impl GpgpuRgba8Surface {
    fn is_valid(self) -> bool { self.width > 0 && self.height > 0 && self.bytes >= self.pitch_bytes as usize * self.height as usize }
}
struct GpgpuOwnedRgba8Surface { surface: GpgpuRgba8Surface, system_service: bool }
impl GpgpuOwnedRgba8Surface { fn surface(&self) -> GpgpuRgba8Surface { self.surface } }
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(v: T) -> Self { Self(std::sync::Mutex::new(v)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
fn align_up(n: usize, a: usize) -> Option<usize> { Some(n.checked_add(a-1)? & !(a-1)) }
fn allocate_font_instance_rgba8_surface(w: u32, h: u32) -> Option<GpgpuOwnedRgba8Surface> {
    TRACE.with_borrow_mut(|t| t.allocations += 1);
    let mut s = surface(w,h); s.gpu = 0xa000000; s.pitch_bytes = align_up(w as usize*4,64)? as u32;
    s.bytes = align_up(s.pitch_bytes as usize*h as usize,4096)?;
    Some(GpgpuOwnedRgba8Surface { surface: s, system_service: false })
}
fn direct_rcs_context_is_quarantined() -> bool { false }
mod libm {
    pub fn sqrt(x:f64)->f64 { x.sqrt() } pub fn ceil(x:f64)->f64 { x.ceil() }
    pub fn sqrtf(x:f32)->f32 { x.sqrt() } pub fn sinf(x:f32)->f32 { x.sin() }
    pub fn cosf(x:f32)->f32 { x.cos() }
}
#[derive(Default, Debug)]
struct GpgpuRgba8KernelResult { ok: bool, submitted: bool, marker: u32, submit_ms: u64, release: Option<u32> }
struct DirectRcsDispatchOutcome { submitted: bool, observed: u32 }
#[derive(Default)]
struct Trace { rows: Vec<(u32, u32, u32)>, failure: Option<(usize, bool)>, releases: usize, allocations: usize }
thread_local! { static TRACE: std::cell::RefCell<Trace> = Default::default(); }
fn reset(failure: Option<(usize,bool)>) {
    *SHADERTOY_FOCUS_SCRATCH.lock() = ShaderToyFocusScratch { allocation: None, quarantined: false };
    TRACE.with_borrow_mut(|t| *t = Trace { failure, ..Default::default() });
}
fn direct_rcs_now_tick() -> u64 { 0 }
fn direct_rcs_elapsed_ms_since(_: u64) -> u64 { 1 }
fn gpgpu_rgba8_release(_: GpgpuRgba8Surface) -> u32 { TRACE.with_borrow_mut(|t| t.releases += 1); 1 }
fn submit_shadertoy_rgba8_rows(_: GpgpuRgba8Surface, _: ShaderToyFrameParams,
                             pass: ShaderToyPass, row: u32, rows: u32) -> DirectRcsDispatchOutcome {
    TRACE.with_borrow_mut(|t| {
        let index = t.rows.len(); t.rows.push((pass.phase, row, rows));
        assert_eq!(t.releases, 0);
        if let Some((at, submitted)) = t.failure && at == index {
            DirectRcsDispatchOutcome { submitted, observed: 0 }
        } else { DirectRcsDispatchOutcome { submitted: true, observed: SHADERTOY_POST_MARKER } }
    })
}
fn surface(width: u32, height: u32) -> GpgpuRgba8Surface {
    GpgpuRgba8Surface { width, height, gpu: 0x12345000, pitch_bytes: width * 4 + 64, bytes: (width as usize*4+64)*height as usize }
}
fn params() -> ShaderToyFrameParams {
    ShaderToyFrameParams { version: 1, shader_id: 6, time_seconds: 8., frame_rate: 60., mouse_x: 200., mouse_y: 123., ..Default::default() }
}
#[test]
fn native_all_rows_once_and_release_only_after_last_retirement() {
    for (width,height) in [(1,1),(15,17),(16,16),(17,19),(360,640),(640,360),(2560,1440),(3840,2160)] {
        reset(None);
        let result=shadertoy_rgba8_surface_full(surface(width,height), ShaderToyFrameParams { flags: 1, ..params() });
        assert!(result.ok && result.submitted && result.release.is_some());
        TRACE.with_borrow(|t| {
            let mut next=0;
            for &(phase,first,rows) in &t.rows {
                assert_eq!(phase,0); assert_eq!(first,next); assert!(rows>0);
                assert!(u64::from(width).div_ceil(16)*16*u64::from(rows)<=SHADERTOY_DISPATCH_MAX_PIXELS); next+=rows;
            }
            assert_eq!(next,height); assert_eq!(t.releases,1); assert_eq!(t.allocations,0);
            if width==2560 { assert_eq!(t.rows.len(),29); }
        });
    }
}
#[test]
fn focused_passes_retire_in_order_and_release_full_output_once() {
    reset(None);
    assert!(shadertoy_rgba8_surface_full(surface(2560,1440),params()).ok);
    TRACE.with_borrow(|t| {
        let shade: Vec<_>=t.rows.iter().filter(|r|r.0==1).collect();
        assert_eq!(shade.len(),8); assert_eq!(t.rows.len(),12);
        assert_eq!(shade.iter().map(|r|r.2).sum::<u32>(),720);
        assert!(t.rows[..8].iter().all(|r|r.0==1));
        assert!(t.rows[8..].iter().all(|r|r.0==2));
        assert_eq!(t.rows[8..].iter().map(|r|r.2).sum::<u32>(),1440);
        assert_eq!(t.releases,1);
    });
    assert!(SHADERTOY_FOCUS_SCRATCH.lock().allocation.as_ref().unwrap().system_service);
}
#[test]
fn failures_in_either_pass_stop_and_quarantine_only_unretired_scratch() {
    for at in [0,2,7,8,10,11] { for submitted in [false,true] {
        reset(Some((at,submitted)));
        let result=shadertoy_rgba8_surface_full(surface(2560,1440),params());
        assert!(!result.ok && result.release.is_none()); assert_eq!(result.submitted,submitted);
        TRACE.with_borrow(|t| { assert_eq!(t.rows.len(),at+1); assert_eq!(t.releases,0); });
        assert_eq!(SHADERTOY_FOCUS_SCRATCH.lock().quarantined,submitted);
        if submitted {
            assert!(!shadertoy_rgba8_surface_full(surface(2560,1440),params()).ok);
            TRACE.with_borrow(|t| assert_eq!(t.rows.len(),at+1));
        }
    }}
}
#[test]
fn cache_reuses_capacity_and_updates_pitch_for_portrait_resize() {
    reset(None);
    let mut cache=SHADERTOY_FOCUS_SCRATCH.lock();
    let a=cache.surface(1280,720).unwrap();
    let b=cache.surface(720,1280).unwrap();
    assert_eq!(b.width,720); assert_eq!(b.pitch_bytes,2880);
    assert_eq!(a.gpu,b.gpu); assert!(b.is_valid());
    TRACE.with_borrow(|t| assert_eq!(t.allocations,1));
    let c=cache.surface(1920,1080).unwrap(); assert!(c.is_valid());
    TRACE.with_borrow(|t| assert_eq!(t.allocations,2));
}
#[test]
fn planner_preserves_native_small_images_and_handles_odd_or_extreme_aspects() {
    assert!(shadertoy_focus_plan(1280,720,params()).is_none());
    for (w,h) in [(2560,1440),(2561,1441),(1441,2561),(4000,300),(3840,2160),(2,500000)] {
        let p=shadertoy_focus_plan(w,h,params()).unwrap();
        assert!(p.width<=w && p.height<=h); assert!(p.width>=w.div_ceil(2) && p.height>=h.div_ceil(2));
        let [x,y,r,b]=p.focus;
        assert!(p.focus.iter().all(|v|v.is_finite())); assert!((1.0..=2.0).contains(&b));
        assert!(r>0.0 && x-r>=-0.01 && y-r>=-0.01 && x+r<=w as f32+0.01 && y+r<=h as f32+0.01);
        if w==2560 { assert_eq!((p.width,p.height),(1280,720)); }
    }
    assert!(shadertoy_focus_plan(2560,1440,ShaderToyFrameParams { flags:1,..params() }).is_none());
    assert!(shadertoy_focus_plan(2560,1440,ShaderToyFrameParams { shader_id:3,..params() }).is_none());
    assert!(ShaderToyFrameParams { flags:1,..params() }.is_valid());
    assert!(!ShaderToyFrameParams { flags:2,..params() }.is_valid());
    assert!(!ShaderToyFrameParams { shader_id:3,flags:1,..params() }.is_valid());
}
#[test]
fn projected_camera_matches_host_proof_and_stays_finite() {
    let p=ShaderToyFrameParams { time_seconds:0.,mouse_x:1280.,mouse_y:720.,..params() };
    let focus=shadertoy_protean_focus(2560,1440,p,2.0);
    for (a,b) in focus.into_iter().zip([1478.245,925.554,514.446,2.0]) { assert!((a-b).abs()<0.01,"{a} != {b}"); }
    for time in [0.,8.,120.,36000.,f32::MAX] {
        assert!(shadertoy_protean_focus(2560,1440,ShaderToyFrameParams { time_seconds:time,..params() },2.0).iter().all(|v|v.is_finite()));
    }
}
#[test]
fn invalid_extents_and_end_rows_never_dispatch() {
    assert_eq!(shadertoy_dispatch_rows(1,0,2560,1440,0),Some(409));
    assert_eq!(shadertoy_dispatch_rows(6,0,0,10,0),None);
    assert_eq!(shadertoy_dispatch_rows(6,0,16,10,10),None);
    assert_eq!(shadertoy_dispatch_rows(6,0,u32::MAX,10,0),None);
}
#[derive(Clone,Copy)]
struct DirectRcsState { batch_virt: *mut u8 }
#[test]
fn real_six_contracts_match_payload_offsets_including_source_pointer() {
    for shader_id in 1..=6 {
        let mut initial=vec![0u32;DIRECT_RCS_BATCH_BYTES/4]; let mut later=initial.clone();
        let dst=surface(1280,720); let p=ShaderToyFrameParams {shader_id,..params()};
        let pass=ShaderToyPass { phase:if shader_id==6 {1}else{0},width:2560,height:1440,source:dst,focus:[1000.,700.,500.,2.] };
        assert!(direct_rcs_write_shadertoy_payload(DirectRcsState {batch_virt:initial.as_mut_ptr().cast()},dst,p,pass,0));
        assert!(direct_rcs_write_shadertoy_payload(DirectRcsState {batch_virt:later.as_mut_ptr().cast()},dst,p,pass,51));
        let differences:Vec<_>=initial.iter().zip(&later).enumerate().filter(|(_, (a,b))|a!=b).map(|(i,_)|i).collect();
        let payload=SHADERTOY_PAYLOAD_OFFSET_BYTES/4; assert_eq!(differences,[payload+1]);
        assert_eq!(later[payload+1],51);
        let d=if shader_id==6 {18}else{16};
        assert_eq!(&later[payload+d..payload+d+3],&[1280,720,dst.pitch_bytes]);
        if shader_id==6 {assert_eq!(later[payload+16],dst.gpu as u32);}
        let u=SHADERTOY_UNIFORMS_OFFSET_BYTES/4;
        assert_eq!(later[u],2560f32.to_bits());assert_eq!(later[u+1],1440f32.to_bits());
        assert_eq!(later[u+4],200f32.to_bits()); assert_eq!(later[u+20],1000f32.to_bits());
    }
    let mut bad=SHADERTOY_PROTEAN_CLOUDS_ADLS_CPP_ABI_CONTRACT;
    bad.payload_args=SHADERTOY_MANDELBROT_ADLS_CPP_ABI_CONTRACT.payload_args;
    assert!(!shadertoy_payload_layout_matches(&bad,true));
}
'''

if __name__ == "__main__":
    main()
