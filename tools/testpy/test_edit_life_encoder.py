#!/usr/bin/env python3
"""Execute Edit's actual batch encoder and inspect its buffers and bindings."""

from pathlib import Path
import re
import subprocess
import tempfile

from test_edit_life_native import constant_closure, function


ROOT = Path(__file__).resolve().parents[2]

FIXTURE = r'''
#![allow(dead_code, unused_variables)]
use std::sync::Mutex;
mod producer {
use super::*;
#[derive(Clone,Copy)] struct GpuVa { batch:u64, result:u64 }
#[derive(Clone,Copy)] struct DirectRcsState { batch_virt:*mut u8, result_virt:*mut u8, gpu_va:GpuVa }
#[derive(Clone,Copy)] struct UploadedKernelArtifact { bin_sha256:[u8;32], gpu:u64, bytes:usize }
#[derive(Clone,Copy,Debug,PartialEq)] struct GpgpuRgba8Surface { gpu:u64, bytes:usize, pitch_bytes:u32 }
#[derive(Clone,Copy)] struct Allocation(GpgpuRgba8Surface);
impl Allocation { fn surface(&self)->GpgpuRgba8Surface { self.0 } }
struct Rect { x:i32, y:i32, width:u32, height:u32 }
struct EditLifeState {
    cells:[Allocation;2], read:usize, glyphs:Allocation, previous:Allocation, font:Allocation,
    canonical:Allocation, geometry_uniforms:Vec<u32>, width:u32, height:u32,
    viewport:Rect, grid_width:u32, grid_height:u32, columns:u32, rows:u32,
    cell_width:u32, cell_height:u32,
}
struct Artifact { bin_sha256:[u8;32] }
struct Contract { entry_offset:u32 }
const EDIT_LIFE_RGBA8_ARTIFACT:Artifact=Artifact{bin_sha256:[3;32]};
const EDIT_LIFE_RGBA8_ADLS_BIN:[u8;1]=[0];
const EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT:Contract=Contract{entry_offset:0};
const GPGPU_VFE_DW5_UOS:u32=0x07820000;
const MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD:u32=1;
const MEDIA_STATE_FLUSH_CMD:u32=2;
const GPGPU_WALKER_SIMD16_MASK:u32=0xffff;
static BINDINGS:Mutex<Vec<(usize,u64,usize)>>=Mutex::new(Vec::new());
fn edit_life_abi_matches()->bool { true }
fn dma_flush(_: *const u8, _:usize) {}
fn direct_rcs_write_interface_descriptor_at(_:DirectRcsState,idd:usize,binding:usize,
    entry:u32,count:u32,cross:u32)->bool {
    assert_eq!(idd,EDIT_LIFE_IDD); assert_eq!(binding,EDIT_LIFE_BINDING);
    assert_eq!(count,8); assert_eq!(cross,4); true
}
fn direct_rcs_write_buffer_surface_state(_:DirectRcsState,offset:usize,gpu:u64,bytes:usize)->bool {
    BINDINGS.lock().unwrap().push((offset,gpu,bytes)); true
}
fn direct_rcs_push(batch:&mut[u32],cursor:&mut usize,value:u32)->bool {
    batch[*cursor]=value; *cursor+=1; true
}
fn direct_rcs_push_pipe_control_timestamp_at(batch:&mut[u32],cursor:&mut usize,gpu:u64,slot:usize)->bool {
    direct_rcs_push(batch,cursor,0xaaaa)
}
fn direct_rcs_push_gpgpu_prologue_with_instruction_base(batch:&mut[u32],cursor:&mut usize,
    base:u64,instruction:u64,vfe:u32)->bool { direct_rcs_push(batch,cursor,0xbbbb) }
fn direct_rcs_push_gpgpu_walker_2d(batch:&mut[u32],cursor:&mut usize,payload:usize,
    indirect:usize,width:u32,rows:u32,mask:u32)->bool { direct_rcs_push(batch,cursor,0xcccc) }
fn direct_rcs_push_gpgpu_dispatch_timestamped_epilogue(batch:&mut[u32],cursor:&mut usize,
    gpu:u64,time_slot:usize,post_slot:usize,marker:u32)->bool { direct_rcs_push(batch,cursor,0xdddd) }
fn direct_rcs_push_gpgpu_dispatch_epilogue(batch:&mut[u32],cursor:&mut usize,
    gpu:u64,post_slot:usize,marker:u32)->bool { direct_rcs_push(batch,cursor,0xeeee) }
@CONSTANTS@
@ENCODER@
fn allocation(gpu:u64,bytes:usize,pitch_bytes:u32)->Allocation {
    Allocation(GpgpuRgba8Surface{gpu,bytes,pitch_bytes})
}
fn words(bytes:&[u8],offset:usize,count:usize)->Vec<u32> {
    bytes[offset..offset+count*4].chunks_exact(4)
        .map(|value|u32::from_le_bytes(value.try_into().unwrap())).collect()
}
pub(super) fn run() {
    let life=EditLifeState {
        cells:[allocation(0x100000,1024,1024),allocation(0x110000,1024,1024)],read:1,
        glyphs:allocation(0x120000,2048,2048),previous:allocation(0x130000,1024,1024),
        font:allocation(0x140000,11584,11584),
        // 600 canonical texels require 2432-byte rows with 64-byte alignment.
        canonical:allocation(0x150000,2432*220,2432),
        geometry_uniforms:(0..32).map(|index|1000+index).collect(),
        width:1700,height:800,viewport:Rect{x:11,y:32,width:1689,height:748},
        grid_width:600,grid_height:220,columns:100,rows:20,cell_width:17,cell_height:38,
    };
    let source=GpgpuRgba8Surface{gpu:0x160000,bytes:8192*800,pitch_bytes:8192};
    let dst=GpgpuRgba8Surface{gpu:0x170000,bytes:8192*800,pitch_bytes:8192};
    let upload=UploadedKernelArtifact{bin_sha256:[3;32],gpu:EDIT_LIFE_RGBA8_GPU,bytes:1};
    for mode in 0..5 {
        for probe in [false,true] {
            let mut batch_storage=vec![0xfdfdfdfdfdfdfdfdu64;512];
            let mut result_storage=vec![0xfefefefefefefefeu64;512];
            let batch=unsafe{std::slice::from_raw_parts_mut(batch_storage.as_mut_ptr().cast::<u8>(),4096)};
            let result=unsafe{std::slice::from_raw_parts_mut(result_storage.as_mut_ptr().cast::<u8>(),4096)};
            let state=DirectRcsState{batch_virt:batch.as_mut_ptr(),result_virt:result.as_mut_ptr(),
                gpu_va:GpuVa{batch:0x08900000,result:0x08840000}};
            BINDINGS.lock().unwrap().clear();
            assert!(encode_edit_life_batch(state,upload,&life,dst,source,mode,true,600,9,7,probe));
            let input=if mode==2 {life.canonical.surface()} else {life.font.surface()};
            let output=if mode==4 {life.canonical.surface()} else {dst};
            let expected=[life.cells[1].surface(),life.cells[0].surface(),life.glyphs.surface(),
                life.previous.surface(),input,source,output];
            let bindings=BINDINGS.lock().unwrap();
            assert_eq!(bindings.len(),8);
            for (index,surface) in expected.iter().enumerate() {
                assert_eq!(bindings[index],(EDIT_LIFE_SURFACES+index*64,surface.gpu,surface.bytes));
            }
            assert_eq!(bindings[7],(EDIT_LIFE_SURFACES+7*64,
                state.gpu_va.batch+EDIT_LIFE_UNIFORMS as u64,128));
            let payload=words(&batch,EDIT_LIFE_PAYLOAD,32);
            assert_eq!(payload[1],9,"global row offset must survive each bounded walker");
            for (index,(_,gpu,_)) in bindings.iter().enumerate() {
                assert_eq!(u64::from(payload[12+index*2])|(u64::from(payload[13+index*2])<<32),*gpu);
            }
            let params=words(&batch,EDIT_LIFE_UNIFORMS,32);
            assert_eq!(&params[..16],&[mode,1,1700,800,11,32,1689,748,600,220,100,20,
                source.pitch_bytes,output.pitch_bytes,17,38]);
            assert_eq!(params[16],1016); assert_eq!(params[17],1017);
            assert_eq!(params[18],2432,"copy must use actual padded canonical pitch");
            assert_eq!(&params[19..],&life.geometry_uniforms[19..]);
            assert!(batch[EDIT_LIFE_BATCH_USED_BYTES..].iter().all(|&byte|byte==0xfd));
            assert!(result[..EDIT_LIFE_RESULT_USED_BYTES].iter().all(|&byte|byte==0));
            assert!(result[EDIT_LIFE_RESULT_USED_BYTES..].iter().all(|&byte|byte==0xfe));
        }
    }
    println!("native EditLife encoder passed: all 5 modes, actual bindings, padded canonical pitch, payload, bounded footprint");
}
}
fn dma_flush(_: *const u8, _:usize) {}
fn main() { producer::run(); }
'''


def main() -> None:
    native = (ROOT / "src/intel/gpgpu/operations/edit_life.rs").read_text()
    encoder = function(native, "encode_edit_life_batch")
    constants = constant_closure(native, encoder)
    constants = re.sub(r"const EDIT_LIFE_RGBA8_ARTIFACT:[^;]+;", "", constants)
    fixture = FIXTURE.replace("@CONSTANTS@", constants).replace("@ENCODER@", encoder)
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-encoder-") as temporary:
        source = Path(temporary) / "encoder.rs"
        binary = Path(temporary) / "encoder"
        source.write_text(fixture)
        subprocess.run(["rustc", "--edition=2024", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
