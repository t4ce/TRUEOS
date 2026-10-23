#!/usr/bin/env python3
"""Exercise production runtime pixel upload/envelope/retirement without hardware."""
from pathlib import Path
import argparse
import os
import shutil
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--dump-envelope', type=Path)
    parser.add_argument('--body', type=Path, help='also admit a body emitted by odin-son')
    args = parser.parse_args()
    constants = 'src/intel/gpgpu/rcs/constants.rs'
    commands = 'src/intel/gpgpu/rcs/commands.rs'
    payloads = 'src/intel/gpgpu/rcs/payloads.rs'
    declarations = [constant(constants, name) for name in (
        'DIRECT_RCS_BATCH_BYTES', 'DIRECT_RCS_RESULT_BYTES', 'COPY_RECT_IDD_BYTES',
        'COPY_RECT_SURFACE_STATE_DWORDS', 'IDD_THREAD_PREEMPTION_DISABLE',
        'GPGPU_WALKER_GROUP_THREADS', 'RENDER_MOCS_INDEX', 'RENDER_MOCS_TABLE_INDEX_MASK',
        'SURFTYPE_BUFFER', 'SURFACE_FORMAT_RAW', 'GPGPU_VFE_DW5_UOS',
        'MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD', 'GPGPU_WALKER_CMD', 'MEDIA_STATE_FLUSH_CMD',
        'UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS', 'MI_BATCH_BUFFER_END', 'MI_NOOP',
        'PIPE_CONTROL_CMD', 'PIPE_CONTROL_HDC_PIPELINE_FLUSH', 'PIPE_CONTROL_FLUSH_ENABLE',
        'PIPE_CONTROL_CS_STALL', 'PIPE_CONTROL_DC_FLUSH_ENABLE', 'PIPE_CONTROL_L3_FABRIC_FLUSH',
        'PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE', 'PIPE_CONTROL_FLUSH_BITS',
        'MI_STORE_DATA_IMM_GGTT_DW1',
    )]
    declarations += [item(constants, 'direct_rcs_encode_mocs_index')]
    declarations += [item(payloads, name) for name in (
        'direct_rcs_write_interface_descriptor_at', 'direct_rcs_write_buffer_surface_state',
        'direct_rcs_write_buffer_surface_state_with_mocs_index',
    )]
    declarations += [item(commands, name) for name in (
        'direct_rcs_push', 'direct_rcs_push_pipe_control_full', 'direct_rcs_push_pipe_control',
        'direct_rcs_push_pipe_control_post_sync_marker_at',
        'direct_rcs_push_store_marker_at', 'direct_rcs_push_store_marker_for_state',
    )]
    declarations += [item('src/intel/gpgpu/rcs/worklists.rs', 'direct_rcs_push_gpgpu_dispatch_epilogue')]
    source = HARNESS.replace('DECLARATIONS', '\n'.join(declarations))
    source = source.replace('VALIDATOR', str(ROOT / 'src/gpu/native_xelp.rs'))
    source = source.replace('OPERATIONS', str(ROOT / 'src/intel/gpgpu/operations/generated_pixel.rs'))
    with tempfile.TemporaryDirectory(prefix='trueos-odin-pixel-') as directory:
        directory = Path(directory)
        envelope = directory / 'envelope.bin'
        (directory / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'),
                        '-o', str(directory / 'test')], check=True, timeout=20)
        extra_env = {'GENERATED_PIXEL_TEST_DUMP': str(envelope)}
        if args.body:
            extra_env['GENERATED_PIXEL_COMPILER_BODY'] = str(args.body.resolve())
        subprocess.run([str(directory / 'test'), '--test-threads=1'], check=True,
                       env={**os.environ, **extra_env}, timeout=20)
        if shutil.which('iga64'):
            assembly = directory / 'envelope.asm'
            assembly.write_text(subprocess.run(['iga64', '-p=12p1', '-d', str(envelope)],
                                              text=True, capture_output=True, check=True).stdout)
            for line in assembly.read_text().splitlines():
                assert not ('mul (' in line and line.count(':ud') == 3), \
                    'Xe-LP forbids dword*dword integer MUL with a direct dword GRF destination'
            roundtrip = directory / 'roundtrip.bin'
            subprocess.run(['iga64', '-p=12p1', '-a', '-Xforce-no-compact', str(assembly),
                            '-o', str(roundtrip)], check=True)
            assert roundtrip.read_bytes() == envelope.read_bytes(), 'native envelope IGA mismatch'
        if args.dump_envelope:
            args.dump_envelope.write_bytes(envelope.read_bytes())
    print('PASS native pixel body admission, immutable upload, coordinates/tails, RGBA packing, '
          'SIMD8 payload, exact release and quarantine. Hardware dispatch is mocked.')


HARNESS = r'''
#![allow(dead_code,unused_variables,private_interfaces)]
mod gpu { #[path="VALIDATOR"] pub(crate) mod native_xelp; }
mod phys {pub fn phys_to_virt(p:usize)->usize{p}}
#[macro_export] macro_rules! log_important {($($tt:tt)*)=>{}}
#[macro_export] macro_rules! log_error {($($tt:tt)*)=>{}}
mod intel {
    #[derive(Clone, Copy)] pub struct Dev {device_id:u16, revision_id:u8}
    fn claimed_device()->Option<Dev>{Some(Dev{device_id:0x4680,revision_id:12})}
    fn dma_flush(_: *mut u8,_:usize){}
    fn mmio_read(_: Dev,_:usize)->u32{0}
    mod gpgpu {
        use std::sync::{Mutex, OnceLock};
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        #[derive(Clone,Copy)] struct DirectRcsGpuVa {batch:u64,result:u64}
        #[derive(Clone,Copy)] struct DirectRcsState {batch_virt:*mut u8,result_virt:*mut u8,gpu_va:DirectRcsGpuVa}
        unsafe impl Send for DirectRcsState {} unsafe impl Sync for DirectRcsState {}
        #[derive(Clone,Copy)] struct GpgpuRgba8Surface {phys:u64,gpu:u64,bytes:usize,width:u32,height:u32,pitch_bytes:u32}
        impl GpgpuRgba8Surface {fn is_valid(self)->bool{self.width>0&&self.height>0&&self.pitch_bytes>=self.width*4&&self.bytes>=self.pitch_bytes as usize*self.height as usize}}
        #[derive(Default)] struct GpgpuRgba8KernelResult {ok:bool,submitted:bool,marker:u32,submit_ms:u64,release:Option<(u64,usize)>}
        fn gpgpu_rgba8_release(dst:GpgpuRgba8Surface)->(u64,usize){(dst.phys,dst.bytes)}
        fn direct_rcs_now_tick()->u64{0}
        fn direct_rcs_elapsed_ms_since(_:u64)->u64{0}
        fn sha256_digest(code:&[u8])->[u8;32]{[code[0];32]}
        fn digest_hex(_: &[u8;32])->String{String::new()}
        struct Activity {acthd:u32,ring_head:u32,ring_tail:u32,ipeir:u32,ipehr:u32,eir:u32,instdone:u32,instps:u32}
        fn activity_snapshot()->Activity{Activity{acthd:0,ring_head:0,ring_tail:0,ipeir:0,ipehr:0,eir:0,instdone:0,instps:0}}
        fn direct_rcs_read_result_slot(s:DirectRcsState,slot:usize)->u32{unsafe{s.result_virt.cast::<u32>().add(slot).read_volatile()}}
        static DIRECT_RCS_SUBMIT_LOCK:Mutex<()>=Mutex::new(());
        static STATE:OnceLock<DirectRcsState>=OnceLock::new();
        static LOST:AtomicBool=AtomicBool::new(false);
        static TIMEOUT:AtomicBool=AtomicBool::new(false);
        static AMBIGUOUS:AtomicBool=AtomicBool::new(false);
        static SUBMITS:AtomicUsize=AtomicUsize::new(0);
        static ROWS:Mutex<Vec<(u32,u32)>>=Mutex::new(Vec::new());
        struct Submission {ambiguous:bool}
        impl Submission {fn may_have_submitted(&self)->bool{true} fn can_poll(&self)->bool{!self.ambiguous}}
        fn direct_rcs_context_is_quarantined()->bool{LOST.load(Ordering::SeqCst)}
        fn direct_rcs_state_once(_:super::Dev)->Option<DirectRcsState>{
            if direct_rcs_context_is_quarantined(){return None}
            Some(*STATE.get_or_init(||DirectRcsState{
                batch_virt:Box::leak(vec![0u32;DIRECT_RCS_BATCH_BYTES/4].into_boxed_slice()).as_mut_ptr().cast(),
                result_virt:Box::leak(vec![0u32;DIRECT_RCS_RESULT_BYTES/4].into_boxed_slice()).as_mut_ptr().cast(),
                gpu_va:DirectRcsGpuVa{batch:0x200000,result:0x300000},
            }))
        }
        fn direct_rcs_forcewake(_:super::Dev)->bool{true}
        fn direct_rcs_map_state(_:super::Dev,_:DirectRcsState)->bool{true}
        fn direct_rcs_init_ppgtt(_:DirectRcsState)->bool{true}
        fn direct_rcs_map_ppgtt_destination(_:DirectRcsState,_:u64,_:u64,_:usize,direct:bool)->bool{assert!(direct);true}
        fn direct_rcs_push_gpgpu_prologue_with_instruction_base(_: &mut[u32],_:&mut usize,b:u64,i:u64,_:u32)->bool{assert_eq!(b,i);true}
        fn direct_rcs_submit_batch_state(_:super::Dev,s:DirectRcsState)->Submission {
            SUBMITS.fetch_add(1,Ordering::SeqCst);
            let batch=unsafe{std::slice::from_raw_parts(s.batch_virt.cast::<u32>(),DIRECT_RCS_BATCH_BYTES/4)};
            let walker=batch.iter().position(|&w|w==GPGPU_WALKER_CMD).unwrap();
            let first_row=batch[GENERATED_PIXEL_PAYLOAD/4+6];
            ROWS.lock().unwrap().push((first_row,batch[walker+10]));
            Submission{ambiguous:AMBIGUOUS.load(Ordering::SeqCst)}
        }
        fn direct_rcs_poll_result_slot_timeout_ms(_:DirectRcsState,slot:usize,want:u32,_:u64)->u32{
            assert_eq!(slot,0);if TIMEOUT.load(Ordering::SeqCst){0}else{want}
        }
        fn quarantine_direct_rcs_context(_: &str){LOST.store(true,Ordering::SeqCst);}
        DECLARATIONS
        include!("OPERATIONS");
        fn body()->Vec<u8>{[0x00030161u32,0x04050aa0,0x00460105,0].into_iter().flat_map(u32::to_le_bytes).collect()}
        fn params()->GeneratedPixelParamsV1{GeneratedPixelParamsV1{origin_x:-2.,origin_y:-1.,step_x:0.125,step_y:0.25}}
        fn surface(w:u32,h:u32)->GpgpuRgba8Surface{
            let pitch=(w*4+63)&!63;let data=vec![0xffu8;(pitch*h)as usize].leak();
            GpgpuRgba8Surface{phys:data.as_ptr()as u64,gpu:0x800000,bytes:data.len(),width:w,height:h,pitch_bytes:pitch}
        }
        #[test]fn admission_owns_and_rejects_unsafe_bodies(){
            let mut bytes=body();let program=create_generated_pixel_program(&bytes).unwrap();
            let owned=program.kernel.clone();bytes[0]=0x31;assert_eq!(program.kernel,owned);
            assert!(!generated_pixel_body_valid(&bytes));
            for reg in [0,3,112,127] {let mut bad=body();bad[7]=reg;assert!(!generated_pixel_body_valid(&bad));}
            let mut bad=body();bad[9]=88;assert!(!generated_pixel_body_valid(&bad));
            let bad_mul=[0x00030141u32,0x04050220,0x02460105,0x00460205].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
            assert!(!generated_pixel_body_valid(&bad_mul),"Xe-LP dword*dword MUL needs explicit lowering");
            assert!(!generated_pixel_body_valid(&[]));
            assert!(!generated_pixel_body_valid(&vec![0;GENERATED_PIXEL_MAX_BODY_BYTES+16]));
            let cmp=[0x00030170u32,0x05058aa0,0x3a460105,0].into_iter().flat_map(u32::to_le_bytes);
            let code:Vec<_>=cmp.chain(body()).collect();assert!(generated_pixel_body_valid(&code));
        }
        #[test]fn admits_actual_compiler_body_when_supplied(){
            if let Ok(path)=std::env::var("GENERATED_PIXEL_COMPILER_BODY"){
                let bytes=std::fs::read(path).unwrap();
                let program=create_generated_pixel_program(&bytes).expect("compiler body must match native profile");
                assert_eq!(program.code_len(),bytes.len());
            }
        }
        #[test]fn payload_walker_exact_release_and_batch_coverage(){
            ROWS.lock().unwrap().clear();
            let s=direct_rcs_state_once(super::claimed_device().unwrap()).unwrap();
            let program=create_generated_pixel_program(&body()).unwrap();let dst=surface(17,3);
            assert!(encode_generated_pixel(s,&program,dst,params(),1,2));
            let batch=unsafe{std::slice::from_raw_parts(s.batch_virt.cast::<u32>(),DIRECT_RCS_BATCH_BYTES/4)};
            assert_eq!(batch[GENERATED_PIXEL_IDD/4],GENERATED_PIXEL_CODE_OFFSET as u32);
            assert_eq!(batch[GENERATED_PIXEL_IDD/4+5],3<<16);assert_eq!(batch[GENERATED_PIXEL_IDD/4+7],0);
            assert_eq!(&batch[GENERATED_PIXEL_PAYLOAD/4+8..GENERATED_PIXEL_PAYLOAD/4+16],&[0,1,2,3,4,5,6,7]);
            assert_eq!(batch[GENERATED_PIXEL_PAYLOAD/4+6],1);
            let walker=batch.iter().position(|&w|w==GPGPU_WALKER_CMD).unwrap();
            assert_eq!(batch[walker+4],0);assert_eq!(batch[walker+7],3);assert_eq!(batch[walker+10],2);
            assert_eq!(batch[walker+13],255);assert_eq!(batch[walker+14],255);
            assert_eq!(batch[walker-1],GENERATED_PIXEL_WALKER_COOKIE);
            assert_eq!(batch[walker+18],GENERATED_PIXEL_AFTER_WALKER_COOKIE);
            let epilogue=walker+21;assert_eq!(batch[epilogue],PIPE_CONTROL_CMD|PIPE_CONTROL_HDC_PIPELINE_FLUSH);
            assert_eq!(batch[epilogue+7]&PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE,PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE);
            let result=generated_pixel_rgba8_surface_full(&program,dst,params());
            assert!(result.ok);assert_eq!(result.release,Some((dst.phys,dst.bytes)));
            ROWS.lock().unwrap().clear();let big=surface(1025,131);
            assert!(generated_pixel_rgba8_surface_full(&program,big,params()).ok);
            let mut next=0;for &(first,rows) in ROWS.lock().unwrap().iter(){assert_eq!(first,next);next+=rows;}
            assert_eq!(next,big.height);
            for &(_,rows) in ROWS.lock().unwrap().iter(){assert!(big.width.div_ceil(8)*rows<=4096);}
        }
        #[test]fn timeout_and_ambiguous_submission_pin_carrier_without_release(){
            let program=create_generated_pixel_program(&body()).unwrap();let dst=surface(17,3);
            TIMEOUT.store(true,Ordering::SeqCst);
            let result=generated_pixel_rgba8_surface_full(&program,dst,params());
            assert!(!result.ok&&result.submitted&&result.release.is_none());assert!(direct_rcs_context_is_quarantined());
            let count=SUBMITS.load(Ordering::SeqCst);assert!(!generated_pixel_rgba8_surface_full(&program,dst,params()).ok);
            assert_eq!(SUBMITS.load(Ordering::SeqCst),count);
            LOST.store(false,Ordering::SeqCst);TIMEOUT.store(false,Ordering::SeqCst);AMBIGUOUS.store(true,Ordering::SeqCst);
            let result=generated_pixel_rgba8_surface_full(&program,dst,params());
            assert!(result.submitted&&result.release.is_none());assert!(direct_rcs_context_is_quarantined());
            LOST.store(false,Ordering::SeqCst);AMBIGUOUS.store(false,Ordering::SeqCst);
        }
        #[test]fn visible_frame_hash_ignores_pitch_padding(){
            let dst=surface(3,2);let before=generated_pixel_surface_fnv(dst);
            unsafe{(dst.phys as *mut u8).add(12).write(0);}
            assert_eq!(generated_pixel_surface_fnv(dst),before);
            unsafe{(dst.phys as *mut u8).add(dst.pitch_bytes as usize).write(0);}
            assert_ne!(generated_pixel_surface_fnv(dst),before);
        }
        fn execute_envelope(kernel:&[u8],width:u32,pitch:u32,first:u32,gx:u32,gy:u32)->Vec<(u32,u32)>{
            let mut r=[[0u32;8];128];r[0][1]=gx;r[0][6]=gy;
            r[1]=[(-2f32).to_bits(),(-1f32).to_bits(),0.125f32.to_bits(),0.25f32.to_bits(),width,pitch,first,0];
            for lane in 0..8{r[2][lane]=lane as u32;}
            let mut flags=[false;8];let mut stores=Vec::new();
            for bytes in kernel.chunks_exact(16){
                let w:[u32;4]=core::array::from_fn(|i|u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap()));
                let op=w[0]&127;
                if op==1||w==crate::gpu::native_xelp::PREFIX{continue}
                if op==0x31{
                    if w[2]>>28==12{for lane in 0..8{if flags[lane]{stores.push((r[3][lane],r[117][lane]));}}}
                    continue
                }
                let dst=(w[1]>>24)as usize;let dt=(w[1]>>4)&15;let st=(w[1]>>8)&15;
                let imm=w[1]&if op==0x61{0x4000}else{0x8000}!=0;
                let source=|bits:u32,lane:usize|{let reg=((bits>>8)&255)as usize;if bits&0x00460000!=0{r[reg][lane]}else{r[reg][((bits>>5)&7)as usize]}};
                let mut out=[0u32;8];
                for lane in 0..8{
                    let a=if op==0x61&&imm{w[3]}else{source(w[2],lane)};
                    let b=if imm{w[3]}else if op==0x61{0}else{source(w[3],lane)};
                    out[lane]=match op{
                        0x61=>match(dt,st){(10,2)=>(a as f32).to_bits(),(2,10)=>f32::from_bits(a)as u32,_=>a},
                        0x40=>if dt==10{(f32::from_bits(a)+f32::from_bits(b)).to_bits()}else{a.wrapping_add(b)},
                        0x41=>if dt==10{(f32::from_bits(a)*f32::from_bits(b)).to_bits()}else{a.wrapping_mul(b&65535)},
                        0x47=>f32::from_bits(a).trunc().to_bits(),
                        0x69=>a.wrapping_shl(b),0x66=>a|b,
                        0x62=>if flags[lane]{a}else{b},
                        0x70=>{flags[lane]=match(w[2]>>28,dt){(4,10)=>f32::from_bits(a)>=f32::from_bits(b),(6,10)=>f32::from_bits(a)<=f32::from_bits(b),(5,2)=>a<b,_=>panic!("cmp")};u32::from(flags[lane])},
                        _=>panic!("op {op:x}")
                    };
                }
                if dst!=0{r[dst]=out;}
            }stores
        }
        #[test]fn native_envelope_coordinates_grayscale_packing_and_tail_mask(){
            let kernel=generated_pixel_envelope(&body()).unwrap();
            assert_eq!(&kernel[kernel.len()-16..],&[0x80000131u32,4,0x70207f0c,0].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>());
            if let Ok(path)=std::env::var("GENERATED_PIXEL_TEST_DUMP"){std::fs::write(path,&kernel).unwrap();}
            for width in [1u32,7,8,9,17,32,1025]{
                for first in [0,5]{for gy in 0..3{
                    let mut seen=vec![false;width as usize];
                    for gx in 0..width.div_ceil(8){
                        for(offset,rgba)in execute_envelope(&kernel,width,(width*4+63)&!63,first,gx,gy){
                            let x=(offset-(first+gy)*((width*4+63)&!63))/4;
                            assert!(x<width);assert!(!seen[x as usize]);seen[x as usize]=true;
                            let gray=((-2.+x as f32*0.125).clamp(0.,1.)*255.)as u32;
                            assert_eq!(rgba,0xff000000|gray|gray<<8|gray<<16,"width={width} x={x}");
                        }
                    }assert!(seen.into_iter().all(|v|v));
                }}
            }
        }
    }
}
'''


if __name__ == '__main__':
    main()
