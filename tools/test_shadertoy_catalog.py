#!/usr/bin/env python3
"""Host-execute catalog state/lifecycle and shared audio upload ordering."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import ROOT, item, constant


def main():
    gpu = ROOT / 'src/intel/gpgpu/operations'
    code = '#![allow(dead_code)]\n' + HARNESS.replace('@ROOT@', str(ROOT)).replace('@PROGRAM_ID@', item('src/intel/gpgpu/artifacts/shadertoy_package.rs', 'program_id'))
    code += (gpu / 'shadertoy.rs').read_text().split('// Bound each non-preemptible')[0]
    code += (gpu / 'shadertoy_catalog.rs').read_text()
    code += (gpu / 'cloud_brush.rs').read_text()
    code += item('src/intel/gpgpu/operations/cpp_audio_visualizer.rs', 'cpp_audio_visualizer_rgba8_surface_full')
    particle = 'src/intel/gpgpu/operations/particle_craft.rs'
    code += '\n'.join(item(particle, name) for name in ('particle_craft_catalog_divisor', 'particle_craft_render_divisor', 'particle_craft_backing_extent', 'particle_craft_sample_extent'))
    source = (ROOT / particle).read_text()
    code += source[source.index('#[derive(Copy, Clone, Debug)]\nstruct ParticleCraftRenderPlan'):source.index('/// Stable host-side')]
    code += '\n'.join(constant(particle, name) for name in (
        'PARTICLE_CRAFT_FRAME_WIDTH', 'PARTICLE_CRAFT_FRAME_HEIGHT', 'PARTICLE_CRAFT_RENDER_DIVISOR',
        'PARTICLE_CRAFT_TILE_SAMPLE_WIDTH', 'PARTICLE_CRAFT_TILE_SAMPLE_HEIGHT',
        'PARTICLE_CRAFT_MAX_SAMPLE_WIDTH', 'PARTICLE_CRAFT_MAX_SAMPLE_HEIGHT',
        'PARTICLE_CRAFT_MAX_TILE_COLUMNS', 'PARTICLE_CRAFT_MAX_TILE_ROWS'))
    code += TESTS
    with tempfile.TemporaryDirectory(prefix='trueos-shadertoy-catalog-') as temp:
        path = Path(temp) / 'tests.rs'
        path.write_text(code)
        binary = Path(temp) / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


HARNESS = r'''
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(v:T)->Self { Self(std::sync::Mutex::new(v)) }
    fn lock(&self)->std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
mod aud { pub mod audio_visualizer {
    use super::super::Mutex;
    pub mod audio_visualizer_tap {
        pub static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        pub fn set_enabled(v:bool) { ENABLED.store(v,std::sync::atomic::Ordering::SeqCst); }
        pub fn enabled()->bool { ENABLED.load(std::sync::atomic::Ordering::SeqCst) }
    }
    include!("@ROOT@/src/aud/audio_visualizer_subscriptions.rs");
    pub struct AudioVisualizerFrame;
    pub fn snapshot()->AudioVisualizerFrame { AudioVisualizerFrame }
} }
mod intel { pub mod gpgpu { pub const CPP_CLOUD_BRUSH_POINT_CAPACITY:usize=32; } }
mod shadertoy_package {
    @PROGRAM_ID@
}
#[derive(Default)]
struct Trace { modes:Vec<u32>, brushes:Vec<Vec<u32>>, particle_flags:Vec<u32>, particle_allocations:usize,
    particle_drops:usize, reject_particle:bool, resident:bool, quarantine:bool, uploads:usize, submits:usize }
static TRACE: Mutex<Trace> = Mutex::new(Trace { modes:Vec::new(),brushes:Vec::new(),particle_flags:Vec::new(),
    particle_allocations:0,particle_drops:0,reject_particle:false,resident:true,quarantine:false,uploads:0,submits:0 });
fn direct_rcs_context_is_quarantined()->bool { TRACE.lock().quarantine }
fn upload_shadertoy_kernel(_:u32)->Option<()> { TRACE.lock().resident.then_some(()) }
#[derive(Copy,Clone)]
struct GpgpuRgba8Surface { gpu:u64, pitch_bytes:u32, width:u32,height:u32 }
impl GpgpuRgba8Surface { fn is_valid(self)->bool { self.width>0 && self.height>0 } }
#[derive(Default)]
struct GpgpuRgba8KernelResult { ok:bool,submitted:bool,marker:u32,submit_ms:u64,release:Option<()> }
fn ok()->GpgpuRgba8KernelResult { GpgpuRgba8KernelResult { ok:true, ..Default::default() } }
fn shadertoy_rgba8_surface_full(_:GpgpuRgba8Surface,_:ShaderToyFrameParams)->GpgpuRgba8KernelResult { ok() }
fn cpp_demo_rgba8_surface_full(_:GpgpuRgba8Surface,_:f32,mode:u32,_:u32,brush:&[u32])->GpgpuRgba8KernelResult {
    let mut t=TRACE.lock(); t.modes.push(mode);t.brushes.push(brush.to_vec());ok()
}
struct GpgpuOwnedParticleCraftState;
impl GpgpuOwnedParticleCraftState { fn allocate()->Option<Self> { TRACE.lock().particle_allocations+=1;Some(Self) } }
impl Drop for GpgpuOwnedParticleCraftState { fn drop(&mut self) { TRACE.lock().particle_drops+=1; } }
const PARTICLE_CRAFT_FLAG_RESET:u32=1;
struct ParticleCraftParamsV1 { flags:u32, dt:f32 }
impl ParticleCraftParamsV1 { fn arc_forge(_:f32,dt:f32,_:u32)->Self { Self {flags:8,dt} } }
fn particle_craft_rgba8_frame_scaled(_: &mut GpgpuOwnedParticleCraftState,s:GpgpuRgba8Surface,p:ParticleCraftParamsV1,divisor:u32)->GpgpuRgba8KernelResult {
    assert!(ParticleCraftRenderPlan::new(s.width,s.height,divisor).is_some());
    assert!((0.001..=0.05).contains(&p.dt)); TRACE.lock().particle_flags.push(p.flags);
    if TRACE.lock().reject_particle { GpgpuRgba8KernelResult::default() } else { ok() }
}
static DIRECT_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
const CPP_AUDIO_VISUALIZER_POST_MARKER:u32=42;
fn direct_rcs_now_tick()->u64 {0}
fn direct_rcs_elapsed_ms_since(_:u64)->u64 {1}
#[derive(Copy,Clone)]
struct AudioBuffer {gpu:u64}
fn cpp_audio_visualizer_buffer_once()->Option<AudioBuffer> {
    assert!(DIRECT_RCS_SUBMIT_LOCK.0.try_lock().is_err()); Some(AudioBuffer{gpu:1})
}
fn cpp_audio_visualizer_write_snapshot(_:AudioBuffer,_:&aud::audio_visualizer::AudioVisualizerFrame) {
    assert!(DIRECT_RCS_SUBMIT_LOCK.0.try_lock().is_err());TRACE.lock().uploads+=1;
}
struct CppAudioVisualizerRgba8Params {audio_gpu:u64,dst_gpu:u64,dst_pitch_bytes:u32,dst_width:u32,dst_height:u32,
    time_seconds:f32,frame:u32,flags:u32}
struct Outcome {observed:u32,submitted:bool}
fn submit_cpp_audio_visualizer_rgba8(_:GpgpuRgba8Surface,_:AudioBuffer,_:CppAudioVisualizerRgba8Params)->Outcome {
    assert!(DIRECT_RCS_SUBMIT_LOCK.0.try_lock().is_err());TRACE.lock().submits+=1;
    Outcome{observed:42,submitted:true}
}
fn gpgpu_rgba8_release(_:GpgpuRgba8Surface) {}
fn surface()->GpgpuRgba8Surface { GpgpuRgba8Surface{gpu:2,pitch_bytes:2560,width:640,height:360} }
fn params(id:u32)->ShaderToyFrameParams { ShaderToyFrameParams {version:1,shader_id:id,frame_rate:30.,..Default::default()} }
fn reset() { *TRACE.lock()=Trace{resident:true,..Default::default()}; }
'''

TESTS = r'''
#[test]
fn rejected_first_particle_submission_keeps_reset_pending_for_retry() {
    reset();let mut a=ShaderToyRuntimeState::new(1);
    TRACE.lock().reject_particle=true;
    assert!(!a.render(surface(),params(15)).ok);
    TRACE.lock().reject_particle=false;
    assert!(a.render(surface(),params(15)).ok);
    assert!(a.render(surface(),params(15)).ok);
    assert_eq!(TRACE.lock().particle_flags,vec![9,9,8]);
    assert_eq!(TRACE.lock().particle_allocations,1);
}

#[test]
fn particles_reuse_the_existing_sample_budget_through_all_resize_shapes() {
    for (w,h) in [(640,360),(640,400),(1280,800),(2560,1440),(2561,1441),(1441,2561),(800,640)] {
        let backing=particle_craft_backing_extent(w,h);
        let expected=particle_craft_sample_extent(backing.0,backing.1);
        let plan=ParticleCraftRenderPlan::new(w,h,particle_craft_catalog_divisor(w,h)).unwrap();
        assert_eq!((plan.sample_width,plan.sample_height),expected);
        assert_eq!((plan.tile_columns,plan.tile_rows),(expected.0.div_ceil(32),expected.1.div_ceil(32)));
    }
    for (w,h,d) in [(0,100,2),(100,0,2),(100,100,0),(100,100,3),(u32::MAX,100,1)] {
        assert!(ParticleCraftRenderPlan::new(w,h,d).is_none());
    }
}

#[test]
fn audio_tap_survives_other_windows_and_preview_stop() {
    use aud::audio_visualizer::{audio_visualizer_tap as tap,set_enabled};
    reset(); set_enabled(false);
    let mut a=ShaderToyRuntimeState::new(1);let mut b=ShaderToyRuntimeState::new(2);
    assert!(!tap::enabled());
    assert!(a.render(surface(),params(7)).ok);assert!(b.render(surface(),params(7)).ok);
    set_enabled(false); assert!(tap::enabled());
    a.render(surface(),params(1));assert!(tap::enabled());
    b.stop_audio();assert!(!tap::enabled());
    b.render(surface(),params(7));set_enabled(true);drop(b);assert!(tap::enabled());
    set_enabled(false);assert!(!tap::enabled());
}
#[test]
fn audio_upload_is_serialized_and_quarantine_prevents_any_write() {
    reset();let snapshot=aud::audio_visualizer::snapshot();
    let r=cpp_audio_visualizer_rgba8_surface_full(surface(),0.,0,&snapshot);
    assert!(r.ok && r.submitted && r.release.is_some());assert_eq!(TRACE.lock().uploads,1);
    TRACE.lock().quarantine=true;
    assert!(!cpp_audio_visualizer_rgba8_surface_full(surface(),0.,1,&snapshot).submitted);
    assert_eq!(TRACE.lock().uploads,1);assert_eq!(TRACE.lock().submits,1);
}
#[test]
fn all_gallery_modes_use_original_mode_numbers_and_brush_is_window_scoped() {
    reset();let mut a=ShaderToyRuntimeState::new(1);let mut b=ShaderToyRuntimeState::new(2);
    for id in 8..=14 { assert!(a.render(surface(),params(id)).ok); }
    assert_eq!(TRACE.lock().modes,(0..=6).collect::<Vec<_>>());
    let mut p=params(14);p.flags=2;p.mouse_x=320.;p.mouse_y=180.;
    a.render(surface(),p); assert_eq!(a.brush.count,1);
    a.render(surface(),p); assert_eq!(a.brush.count,1);
    p.mouse_x=640.;p.mouse_y=0.;a.render(surface(),p);
    assert!(a.brush.count>1);assert_eq!(a.brush.points[a.brush.count-1],u32::MAX);
    b.render(surface(),params(14));assert_eq!(b.brush.count,0);
    a.render(surface(),params(1));a.render(surface(),params(14));assert_eq!(a.brush.count,0);
}
#[test]
fn particles_initialize_per_window_and_reset_on_reentry() {
    reset();let mut a=ShaderToyRuntimeState::new(1);let mut b=ShaderToyRuntimeState::new(2);
    for _ in 0..2 { a.render(surface(),params(15)); }
    b.render(surface(),params(15));
    assert_eq!(TRACE.lock().particle_flags,vec![9,8,9]);
    a.render(surface(),params(1));assert_eq!(TRACE.lock().particle_drops,1);
    a.render(surface(),params(15));assert_eq!(TRACE.lock().particle_allocations,3);
    drop(a);drop(b);assert_eq!(TRACE.lock().particle_drops,3);
}
#[test]
fn invalid_or_unregistered_programs_do_not_acquire_runtime_state() {
    reset();let mut a=ShaderToyRuntimeState::new(1);
    TRACE.lock().resident=false;assert!(!a.render(surface(),params(7)).ok);assert!(a.audio.is_none());
    for id in 1..=15 {
        assert!(params(id).is_valid());
        for flags in [1,2,3,4] { assert_eq!(ShaderToyFrameParams{flags,..params(id)}.is_valid(),
            (id==6 && flags==1)||(id==14 && flags==2)); }
    }
    for id in [0,16,31,32,u32::MAX] { assert!(!params(id).is_valid()); }
}
'''

if __name__ == '__main__':
    main()
