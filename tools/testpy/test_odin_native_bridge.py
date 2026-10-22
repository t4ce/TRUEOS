#!/usr/bin/env python3
"""Exercise the production native compute UI4 bridge with host ownership mocks."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = r'''#![allow(dead_code, unused_imports, private_interfaces)]
extern crate alloc;
extern crate self as trueos_vm;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[macro_export] macro_rules! log_info {
    (target:$target:literal; $($tokens:tt)*)=>{{let _=format_args!($($tokens)*);}}
}
pub mod vmcall {
    pub const OP_BP_UI4_NATIVE_COMPUTE_UPLOAD_V1:u64=1;
    pub const OP_BP_UI4_NATIVE_COMPUTE_RENDER_V1:u64=2;
}
mod hv {pub fn current_hull_guest_context_vm_id()->Option<u64> {None}}
mod intel {pub mod gpgpu {
    use crate::{Mutex,ui4::blueprint_ui4::{Lease,SURFACES}};
    #[derive(Debug)] pub struct GeneratedPixelProgram {pub bytes:Vec<u8>}
    impl GeneratedPixelProgram {pub fn code_len(&self)->usize {self.bytes.len()}}
    #[derive(Copy,Clone,Debug,PartialEq)]
    pub struct GeneratedPixelParamsV1 {pub origin_x:f32,pub origin_y:f32,pub step_x:f32,pub step_y:f32}
    #[derive(Copy,Clone,Debug,Eq,PartialEq)] pub struct Destination {pub lease:Lease}
    #[derive(Copy,Clone,Debug,Eq,PartialEq)] pub struct Release {pub allocation:u64,pub sequence:u64}
    #[derive(Copy,Clone,Debug,Default)]
    pub struct ResultInfo {pub ok:bool,pub submitted:bool,pub release:Option<Release>}
    pub struct Fake {
        pub admitted:Vec<Vec<u8>>,
        pub calls:Vec<(Vec<u8>,Destination,GeneratedPixelParamsV1)>,
        pub conversions:usize,
        pub result:ResultInfo,
        pub replace_lease:Option<Lease>,
    }
    impl Fake {pub const fn new()->Self {Self{admitted:Vec::new(),calls:Vec::new(),
        conversions:0,result:ResultInfo{ok:false,submitted:false,release:None},replace_lease:None}}}
    pub static FAKE:Mutex<Fake>=Mutex::new(Fake::new());
    pub fn create_generated_pixel_program(bytes:&[u8])->Result<GeneratedPixelProgram,i32> {
        FAKE.lock().admitted.push(bytes.to_vec());
        if bytes[0]==0xff {Err(-22)} else {Ok(GeneratedPixelProgram{bytes:bytes.to_vec()})}
    }
    pub fn generated_pixel_rgba8_surface_full(program:&GeneratedPixelProgram,dst:Destination,
        params:GeneratedPixelParamsV1)->ResultInfo {
        let (result,replacement)={let mut fake=FAKE.lock();
            fake.calls.push((program.bytes.clone(),dst,params));
            (fake.result,fake.replace_lease.take())};
        if let Some(lease)=replacement {SURFACES.lock()[0].write_lease=Some(lease);}
        result
    }
}}
mod ui4 {pub mod blueprint_ui4 {
    use crate::{Mutex,intel::gpgpu::{Destination,Release,FAKE}};
    pub const ERROR_INVALID:i32=-1;
    pub const ERROR_CONTEXT:i32=-2;
    pub const ERROR_NOT_FOUND:i32=-3;
    pub const ERROR_STATE:i32=-4;
    pub const ERROR_UI4:i32=-6;
    pub const ERROR_BUSY:i32=-7;
    pub type WindowOwner=u64;
    #[derive(Copy,Clone,Debug,Eq,PartialEq)] pub struct Lease {pub allocation:u64}
    pub struct Surface {
        pub owner:WindowOwner,pub window:u32,pub visual_cadence:Option<()>,
        pub gpu_submission_unretired:bool,pub write_lease:Option<Lease>,
        pub pending_gpu_release:Option<Release>,pub pending_render_release:Option<()>,
        pub native_compute:native_compute_api::NativeComputeState,
    }
    pub static SURFACES:Mutex<Vec<Surface>>=Mutex::new(Vec::new());
    pub fn surface_mut(surfaces:&mut [Surface],owner:WindowOwner,window:u32)->Option<&mut Surface> {
        surfaces.iter_mut().find(|s|s.owner==owner&&s.window==window)
    }
    fn blueprint_owner()->Option<WindowOwner> {Some(1)}
    fn guest_status(_op:u64,_a:u64,_b:u64,_payload:&[u8])->i32 {panic!("unexpected guest transport")}
    fn gpgpu_rgba_surface(lease:Lease)->Result<Destination,()> {
        FAKE.lock().conversions+=1; Ok(Destination{lease})
    }
    mod native_compute_api {
'''
    source += (ROOT / 'src/ui4/blueprint_ui4/native_compute_api.rs').read_text().replace('//!', '//')
    source += r'''
#[cfg(test)] mod tests {
    use super::*;
    use super::super::{Surface,Lease};
    use crate::intel::gpgpu::{FAKE,Fake,Release,ResultInfo};
    static TEST_LOCK:crate::Mutex<()>=crate::Mutex::new(());
    fn reset() {
        *FAKE.lock()=Fake::new();
        *SURFACES.lock()=vec![Surface{owner:1,window:7,visual_cadence:Some(()),
            gpu_submission_unretired:false,write_lease:None,pending_gpu_release:None,
            pending_render_release:None,native_compute:NativeComputeState::default()}];
    }
    fn wire()->Vec<u8> {[-2.0f32,-1.0,0.01,0.02].into_iter().flat_map(f32::to_le_bytes).collect()}
    fn register() {assert_eq!(upload_chunk(1,7,0,16,&[1;16]),0);}
    fn lease() {SURFACES.lock()[0].write_lease=Some(Lease{allocation:42});}

    #[cfg(not(feature="odin-native-gpgpu"))]
    #[test] fn disabled_feature_has_no_upload_or_render_side_effects() {
        let _guard=TEST_LOCK.lock(); reset();
        assert_eq!(upload_chunk(1,7,0,16,&[1;16]),-95);
        assert_eq!(render_wire(1,7,&wire()),-95);
        assert_eq!(unsafe {trueos_cabi_ui4_scene_native_compute_upload_v1(7,0,16,std::ptr::null(),0)},-95);
        assert_eq!(unsafe {trueos_cabi_ui4_scene_native_compute_render_v1(7,-2.0,-1.0,0.01,0.02)},-95);
        let surfaces=SURFACES.lock();
        assert!(surfaces[0].native_compute.upload.is_none());
        assert!(surfaces[0].native_compute.program.is_none());
        assert!(!surfaces[0].gpu_submission_unretired);
        assert!(FAKE.lock().admitted.is_empty()); assert!(FAKE.lock().calls.is_empty());
    }

    #[cfg(feature="odin-native-gpgpu")]
    #[test] fn foreign_owners_cannot_upload_render_or_change_registered_program() {
        let _guard=TEST_LOCK.lock(); reset(); register(); lease();
        assert_eq!(upload_chunk(2,7,0,16,&[2;16]),ERROR_NOT_FOUND);
        assert_eq!(render_wire(2,7,&wire()),ERROR_NOT_FOUND);
        let surfaces=SURFACES.lock();
        assert_eq!(surfaces[0].native_compute.program.as_ref().unwrap().bytes,[1;16]);
        assert!(!surfaces[0].gpu_submission_unretired);
        assert!(surfaces[0].pending_gpu_release.is_none());
        assert!(FAKE.lock().calls.is_empty());
    }

    #[cfg(feature="odin-native-gpgpu")]
    #[test] fn ordered_chunk_upload_is_atomic_and_never_acquires_a_frame_lease() {
        let _guard=TEST_LOCK.lock(); reset();
        let code:Vec<u8>=(0..4112).map(|i|(i%251) as u8).collect();
        assert_eq!(upload_chunk(1,7,0,code.len(),&code[..2048]),0);
        assert!(SURFACES.lock()[0].native_compute.program.is_none());
        assert_eq!(upload_chunk(1,7,2048,code.len(),&code[2048..4096]),0);
        assert!(FAKE.lock().admitted.is_empty());
        assert_eq!(upload_chunk(1,7,4096,code.len(),&code[4096..]),0);
        let surfaces=SURFACES.lock();
        assert!(surfaces[0].native_compute.upload.is_none());
        assert_eq!(surfaces[0].native_compute.program.as_ref().unwrap().bytes,code);
        assert!(surfaces[0].write_lease.is_none());
        assert!(!surfaces[0].gpu_submission_unretired);
        let fake=FAKE.lock(); assert_eq!(fake.admitted,[code]);
        assert_eq!(fake.conversions,0); assert!(fake.calls.is_empty());
    }

    #[cfg(feature="odin-native-gpgpu")]
    #[test] fn malformed_sequence_discards_staging_and_failed_admission_preserves_previous_program() {
        let _guard=TEST_LOCK.lock(); reset(); register();
        assert_eq!(upload_chunk(1,7,0,32,&[2;16]),0);
        assert_eq!(upload_chunk(1,7,8,32,&[2;16]),ERROR_INVALID);
        assert!(SURFACES.lock()[0].native_compute.upload.is_none());
        assert_eq!(upload_chunk(1,7,16,32,&[2;16]),ERROR_STATE);
        assert_eq!(upload_chunk(1,7,0,32,&[2;16]),0);
        assert_eq!(upload_chunk(1,7,16,48,&[2;16]),ERROR_INVALID);
        assert!(SURFACES.lock()[0].native_compute.upload.is_none());
        assert_eq!(upload_chunk(1,7,0,16,&[0xff;16]),-22);
        let surfaces=SURFACES.lock();
        assert!(surfaces[0].native_compute.upload.is_none());
        assert_eq!(surfaces[0].native_compute.program.as_ref().unwrap().bytes,[1;16]);
    }

    #[cfg(feature="odin-native-gpgpu")]
    #[test] fn render_requires_existing_lease_program_and_clear_release_slots() {
        let _guard=TEST_LOCK.lock(); reset(); register();
        assert_eq!(render_wire(1,7,&wire()),ERROR_STATE);
        assert_eq!(FAKE.lock().conversions,0); lease();
        SURFACES.lock()[0].pending_render_release=Some(());
        assert_eq!(render_wire(1,7,&wire()),ERROR_STATE);
        SURFACES.lock()[0].pending_render_release=None;
        SURFACES.lock()[0].pending_gpu_release=Some(Release{allocation:42,sequence:9});
        assert_eq!(render_wire(1,7,&wire()),ERROR_STATE);
        assert!(FAKE.lock().calls.is_empty());
        assert!(SURFACES.lock()[0].native_compute.program.is_some());
    }

    #[cfg(feature="odin-native-gpgpu")]
    #[test] fn successful_render_attaches_exact_completion_release_and_retains_program() {
        let _guard=TEST_LOCK.lock(); reset(); register(); lease();
        let release=Release{allocation:42,sequence:99};
        FAKE.lock().result=ResultInfo{ok:true,submitted:true,release:Some(release)};
        assert_eq!(render_wire(1,7,&wire()),0);
        let surfaces=SURFACES.lock(); let surface=&surfaces[0];
        assert_eq!(surface.pending_gpu_release,Some(release));
        assert_eq!(surface.write_lease,Some(Lease{allocation:42}));
        assert!(!surface.gpu_submission_unretired);
        assert!(surface.native_compute.program.is_some());
        let fake=FAKE.lock(); assert_eq!(fake.calls.len(),1);
        assert_eq!(fake.calls[0].1.lease,Lease{allocation:42});
        assert_eq!(fake.calls[0].2,GeneratedPixelParamsV1{
            origin_x:-2.0,origin_y:-1.0,step_x:0.01,step_y:0.02});
    }

    #[cfg(feature="odin-native-gpgpu")]
    #[test] fn ambiguous_submitted_failure_pins_lease_and_blocks_upload_and_render() {
        let _guard=TEST_LOCK.lock(); reset(); register(); lease();
        FAKE.lock().result=ResultInfo{ok:false,submitted:true,release:None};
        assert_eq!(render_wire(1,7,&wire()),ERROR_UI4);
        assert!(SURFACES.lock()[0].gpu_submission_unretired);
        assert!(SURFACES.lock()[0].pending_gpu_release.is_none());
        assert!(SURFACES.lock()[0].native_compute.program.is_some());
        assert_eq!(upload_chunk(1,7,0,16,&[2;16]),ERROR_BUSY);
        assert_eq!(render_wire(1,7,&wire()),ERROR_BUSY);
        assert_eq!(FAKE.lock().calls.len(),1);
    }

    #[cfg(feature="odin-native-gpgpu")]
    #[test] fn safe_rejection_can_retry_but_a_changed_lease_cannot_receive_release() {
        let _guard=TEST_LOCK.lock(); reset(); register(); lease();
        assert_eq!(render_wire(1,7,&wire()),ERROR_BUSY);
        assert!(!SURFACES.lock()[0].gpu_submission_unretired);
        assert!(SURFACES.lock()[0].native_compute.program.is_some());
        {let mut fake=FAKE.lock();
            fake.result=ResultInfo{ok:true,submitted:true,
                release:Some(Release{allocation:42,sequence:99})};
            fake.replace_lease=Some(Lease{allocation:43});}
        assert_eq!(render_wire(1,7,&wire()),ERROR_STATE);
        let surfaces=SURFACES.lock();
        assert!(surfaces[0].pending_gpu_release.is_none());
        assert!(surfaces[0].gpu_submission_unretired);
        assert_eq!(surfaces[0].write_lease,Some(Lease{allocation:43}));
    }

    #[cfg(feature="odin-native-gpgpu")]
    #[test] fn invalid_coordinates_and_bounds_do_not_mutate_lease_or_dispatch() {
        let _guard=TEST_LOCK.lock(); reset(); register(); lease();
        for values in [[f32::NAN,0.0,0.1,0.1],[0.0,0.0,0.0,0.1],[0.0,0.0,0.1,-0.1]] {
            let bytes:Vec<u8>=values.into_iter().flat_map(f32::to_le_bytes).collect();
            assert_eq!(render_wire(1,7,&bytes),ERROR_INVALID);
        }
        assert_eq!(render_wire(1,7,&[0;15]),ERROR_INVALID);
        assert_eq!(upload_chunk(1,7,0,16,&[0;17]),ERROR_INVALID);
        assert_eq!(upload_chunk(1,7,0,16,&[0;2049]),ERROR_INVALID);
        assert!(FAKE.lock().calls.is_empty());
        assert!(!SURFACES.lock()[0].gpu_submission_unretired);
    }
}
}}}
'''
    with tempfile.TemporaryDirectory(prefix='odin-native-bridge-') as directory:
        path = Path(directory) / 'tests.rs'
        path.write_text(source)
        for enabled in (False, True):
            binary = Path(directory) / ('enabled' if enabled else 'disabled')
            command = ['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)]
            if enabled:
                command += ['--cfg', 'feature="odin-native-gpgpu"']
            subprocess.run(command, check=True)
            subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
