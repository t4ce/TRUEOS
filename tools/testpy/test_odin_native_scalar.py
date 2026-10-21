#!/usr/bin/env python3
"""Exercise the production scalar encoder, concurrency and timeout ownership without hardware."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main():
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
    )]
    declarations += [item(constants, 'direct_rcs_encode_mocs_index')]
    declarations += [item(payloads, name) for name in (
        'direct_rcs_write_interface_descriptor_at', 'direct_rcs_write_buffer_surface_state',
        'direct_rcs_write_buffer_surface_state_with_mocs_index',
    )]
    declarations += [item(commands, name) for name in (
        'direct_rcs_push', 'direct_rcs_push_pipe_control_full', 'direct_rcs_push_pipe_control',
        'direct_rcs_push_pipe_control_post_sync_marker_at',
    )]
    declarations += [item('src/intel/gpgpu/rcs/worklists.rs', 'direct_rcs_push_gpgpu_dispatch_epilogue')]
    source = r'''
#![allow(dead_code)]
mod gpu { #[path="VALIDATOR"] pub(crate) mod native_xelp; }
mod intel {
    #[derive(Clone, Copy)] pub struct Dev {device_id:u16, revision_id:u8}
    fn claimed_device()->Option<Dev>{Some(Dev{device_id:0x4680,revision_id:12})}
    fn dma_flush(_: *mut u8,_:usize){}
    mod gpgpu {
        use std::sync::{Mutex, OnceLock};
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::*};
        #[derive(Clone,Copy)] struct DirectRcsGpuVa {batch:u64,result:u64}
        #[derive(Clone,Copy)] struct DirectRcsState {batch_virt:*mut u8,result_virt:*mut u8,gpu_va:DirectRcsGpuVa}
        unsafe impl Send for DirectRcsState {} unsafe impl Sync for DirectRcsState {}
        static DIRECT_RCS_SUBMIT_LOCK:Mutex<()>=Mutex::new(());
        static STATE:OnceLock<DirectRcsState>=OnceLock::new();
        static LOST:AtomicBool=AtomicBool::new(false);
        static TIMEOUT:AtomicBool=AtomicBool::new(false);
        static SUBMITS:AtomicUsize=AtomicUsize::new(0);
        struct Submission;
        impl Submission {fn may_have_submitted(&self)->bool{true} fn can_poll(&self)->bool{true}}
        fn direct_rcs_state_once(_:super::Dev)->Option<DirectRcsState>{
            if LOST.load(SeqCst){return None}
            Some(*STATE.get_or_init(||DirectRcsState{
                batch_virt:Box::leak(vec![0u32;DIRECT_RCS_BATCH_BYTES/4].into_boxed_slice()).as_mut_ptr().cast(),
                result_virt:Box::leak(vec![0u32;DIRECT_RCS_RESULT_BYTES/4].into_boxed_slice()).as_mut_ptr().cast(),
                gpu_va:DirectRcsGpuVa{batch:0x200000,result:0x300000},
            }))
        }
        fn direct_rcs_forcewake(_:super::Dev)->bool{true}
        fn direct_rcs_map_state(_:super::Dev,_:DirectRcsState)->bool{true}
        fn direct_rcs_init_ppgtt(_:DirectRcsState)->bool{true}
        fn direct_rcs_push_gpgpu_prologue_with_instruction_base(_: &mut[u32],_:&mut usize,b:u64,i:u64,_:u32)->bool{assert_eq!(b,i);true}
        fn direct_rcs_submit_batch_state(_:super::Dev,s:DirectRcsState)->Submission {
            SUBMITS.fetch_add(1,SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(1));
            unsafe {
                let p=s.batch_virt.add(NATIVE_SCALAR_PAYLOAD).cast::<u32>();
                let (a,b)=(f32::from_bits(p.read()),f32::from_bits(p.add(8).read()));
                s.result_virt.add(NATIVE_SCALAR_RESULT).cast::<u32>().write((a+b).to_bits());
            }
            Submission
        }
        fn direct_rcs_poll_result_slot_timeout_ms(_:DirectRcsState,slot:usize,want:u32,_:u64)->u32{
            assert_eq!(slot,0);if TIMEOUT.load(SeqCst){0}else{want}
        }
        fn quarantine_direct_rcs_context(_: &str){LOST.store(true,SeqCst);}
        DECLARATIONS
        include!("OPERATIONS");
        fn code()->Vec<u8>{
            crate::gpu::native_xelp::PREFIX.into_iter()
                .chain([0x00030140,0x04050aa0,0x0a460105,0x00460205])
                .chain(crate::gpu::native_xelp::SUFFIX.into_iter().flatten())
                .flat_map(u32::to_le_bytes).collect()
        }
        #[test] fn payload_walker_and_ordered_epilogue(){
            let s=direct_rcs_state_once(super::claimed_device().unwrap()).unwrap();
            assert!(encode_native_scalar(s,&code(),10f32.to_bits(),100f32.to_bits()));
            let batch=unsafe{std::slice::from_raw_parts(s.batch_virt.cast::<u32>(),DIRECT_RCS_BATCH_BYTES/4)};
            assert_eq!(batch[NATIVE_SCALAR_IDD/4],NATIVE_SCALAR_CODE_OFFSET as u32);
            assert_eq!(batch[NATIVE_SCALAR_IDD/4+5],0);
            assert_eq!(batch[NATIVE_SCALAR_IDD/4+7],3);
            assert_eq!(&batch[NATIVE_SCALAR_PAYLOAD/4..NATIVE_SCALAR_PAYLOAD/4+8],&[10f32.to_bits();8]);
            assert_eq!(&batch[NATIVE_SCALAR_PAYLOAD/4+8..NATIVE_SCALAR_PAYLOAD/4+16],&[100f32.to_bits();8]);
            assert_eq!(&batch[NATIVE_SCALAR_PAYLOAD/4+16..NATIVE_SCALAR_PAYLOAD/4+24],&[0;8]);
            assert_eq!(batch[NATIVE_SCALAR_SURFACE/4+8],s.gpu_va.result as u32+NATIVE_SCALAR_RESULT as u32);
            assert_eq!(batch[NATIVE_SCALAR_SURFACE/4+2],3);
            let walker=batch.iter().position(|&w|w==GPGPU_WALKER_CMD).unwrap();
            assert_eq!(batch[walker+2],128);assert_eq!(batch[walker+3],NATIVE_SCALAR_PAYLOAD as u32);
            assert_eq!(batch[walker+4],0);assert_eq!(batch[walker+13],1);assert_eq!(batch[walker+14],1);
            let release=walker+17;
            assert_eq!(batch[release],PIPE_CONTROL_CMD|PIPE_CONTROL_HDC_PIPELINE_FLUSH);
            assert_eq!(batch[release+1],PIPE_CONTROL_FLUSH_BITS);
            assert_eq!(batch[release+6],PIPE_CONTROL_CMD);
            assert_eq!(batch[release+7]&PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE,PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE);
            assert_eq!(batch[release+10],NATIVE_SCALAR_COOKIE);
        }
        #[test] fn independent_callers_keep_their_own_results(){
            let tasks:Vec<_>=(0..8).map(|i|std::thread::spawn(move||{
                let actual=native_scalar_f32(&code(),(i as f32).to_bits(),100f32.to_bits()).unwrap();
                assert_eq!(f32::from_bits(actual),i as f32+100.0);
            })).collect();
            for task in tasks {task.join().unwrap();}
        }
        #[test] fn rejects_before_submission_and_quarantines_timeout(){
            let before=SUBMITS.load(SeqCst);
            assert_eq!(native_scalar_f32(&[0;160],0,0),Err(-22));
            assert_eq!(SUBMITS.load(SeqCst),before);
            TIMEOUT.store(true,SeqCst);
            assert_eq!(native_scalar_f32(&code(),0,0),Err(-32));
            assert!(LOST.load(SeqCst));
            assert_eq!(native_scalar_f32(&code(),0,0),Err(-19));
            assert_eq!(SUBMITS.load(SeqCst),before+1);
            TIMEOUT.store(false,SeqCst);LOST.store(false,SeqCst);
        }
    }
}
'''
    source = source.replace('VALIDATOR', str(ROOT/'src/gpu/native_xelp.rs'))
    source = source.replace('OPERATIONS', str(ROOT/'src/intel/gpgpu/operations/native_scalar.rs'))
    source = source.replace('DECLARATIONS', '\n'.join(declarations))
    with tempfile.TemporaryDirectory(prefix='trueos-odin-scalar-') as directory:
        directory = Path(directory)
        (directory/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(directory/'test.rs'),'-o',str(directory/'test')],check=True,timeout=12)
        subprocess.run([str(directory/'test'),'--test-threads=1'],check=True,timeout=12)
    print('PASS production scalar payload, SIMD8 mask, ordered release, concurrent callers and timeout quarantine. No hardware dispatch tested.')


if __name__ == '__main__':
    main()
