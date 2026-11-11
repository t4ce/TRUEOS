#!/usr/bin/env python3
"""Exercise actual 3D entry packets and resident secondary prefix validation."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]
C = "src/intel/render/constants.rs"
P = "src/intel/render/pipeline.rs"
PRIMARY = "src/intel/render/primary.rs"
source = r'''
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool,Ordering};
static TGL:AtomicBool=AtomicBool::new(false);
mod intel {
    pub fn align_up(v:usize,a:usize)->Option<usize> {v.checked_add(a-1).map(|v|v&!(a-1))}
    pub mod render {pub fn tgl_spirit_compute_contract()->bool {crate::TGL.load(crate::Ordering::Acquire)}}
}
fn device_is_gfx12(device:u16)->bool {matches!(device,0x4680|0xa780|0x9a49)}
'''
for name in (
    "PIPE_CONTROL_CMD", "PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER",
    "PIPE_CONTROL_L3_READ_ONLY_CACHE_INVALIDATE_HEADER", "PIPE_CONTROL_VF_CACHE_INVALIDATE",
    "PIPE_CONTROL_DC_FLUSH_ENABLE", "PIPE_CONTROL_FLUSH_ENABLE",
    "PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH", "PIPE_CONTROL_DEPTH_CACHE_FLUSH",
    "PIPE_CONTROL_DEPTH_STALL", "PIPE_CONTROL_CS_STALL", "PIPE_CONTROL_FLUSH_BITS",
    "PIPE_CONTROL_TLB_INVALIDATE", "PIPE_CONTROL_INVALIDATE_BITS",
    "PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE", "PIPELINE_SELECT_3D",
    "PIPE_CONTROL_SCENE_RELEASE_MARKER_BITS", "PIPE_CONTROL_DEST_GGTT",
    "MI_STORE_DATA_IMM_GGTT_DW1", "MI_BATCH_BUFFER_END", "MI_NOOP",
    "RESULT_SLOT_BATCH_ENTRY_DWORD", "RCS_EXEC_RESULT_DRAW_BATCH_ENTRY",
    "RESIDENT_SCENE_MAX_DRAWS", "STATE_BASE_ADDRESS_CMD",
    "RENDER_MOCS", "GPU_VA_DRAW_STATE_BASE", "GPU_VA_VERTEX_BASE",
):
    source += constant(C, name) + "\n"
source += constant(P, "RENDER_3D_ENTRY_MAX_DWORDS")
for name in (
    "RESIDENT_SECONDARY_ENTRY_PREFIX_DWORDS", "RESULT_SLOT_POST_OPENING_DWORD",
    "RCS_EXEC_RESULT_DRAW_POST_OPENING",
):
    source += constant(PRIMARY, name) + "\n"
for path, name in (
    (P, "render_pipe_control_packet"), (P, "render_3d_entry_packets"),
    (P, "render_pipe_control_tests"),
    (PRIMARY, "resident_secondary_marker"), (PRIMARY, "finish_resident_secondary_breadcrumbs"),
    (P, "encode_3d_no_draw_probe_batch"),
):
    source += item(path, name) + "\n"
source += r'''
#[derive(Clone,Copy)]struct RenderWarmState {device_id:u16,draw_state_len:usize,vertex_len:usize}
fn parse_entry(words:&[u32],mut is_3d:bool)->bool {
    let mut cursor=0;let mut previous_hdc_stall=false;let mut rt_flush=false;let mut invalidated=false;
    while cursor<words.len() {
        let header=words[cursor];
        if header==PIPELINE_SELECT_3D {
            if !is_3d {assert!(previous_hdc_stall,"GPGPU->3D needs a common stalling HDC fence");}
            is_3d=true;cursor+=1;previous_hdc_stall=false;continue;
        }
        assert_eq!(header&0xffff0000,PIPE_CONTROL_CMD&0xffff0000);
        assert!(cursor+6<=words.len());let flags=words[cursor+1];
        let three_d_bits=PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH|PIPE_CONTROL_DEPTH_CACHE_FLUSH|PIPE_CONTROL_DEPTH_STALL;
        assert!(is_3d || flags&three_d_bits==0,"3D-only flush before known3D entry");
        previous_hdc_stall=header&PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER!=0 && flags&PIPE_CONTROL_CS_STALL!=0;
        rt_flush|=flags&PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH!=0;
        invalidated|=flags&PIPE_CONTROL_INVALIDATE_BITS==PIPE_CONTROL_INVALIDATE_BITS;
        cursor+=6;
    }
    assert!(rt_flush && invalidated,"entry must retain old 3D flush/invalidate boundaries");is_3d
}
fn secondary(tgl:bool)->(Vec<u32>,usize,usize) {
    TGL.store(tgl,Ordering::Release);
    let (entry,used)=render_3d_entry_packets(0x4680,PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER);
    let mut batch=vec![0xfeedface;4+used+4+2];batch[4..4+used].copy_from_slice(&entry[..used]);
    let result=0x1_0084_0000u64;let addr=result+RESULT_SLOT_BATCH_ENTRY_DWORD as u64*4;
    batch[4+used..4+used+4].copy_from_slice(&[MI_STORE_DATA_IMM_GGTT_DW1,addr as u32,(addr>>32)as u32,RCS_EXEC_RESULT_DRAW_BATCH_ENTRY]);
    batch[4+used+4..].copy_from_slice(&[MI_BATCH_BUFFER_END,MI_NOOP]);
    (batch,(used+6)*4,used)
}
#[test]fn native_entry_is_valid_from_fresh_and_restored_pipeline_modes() {
    TGL.store(true,Ordering::Release);
    for header in [0,PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER] {
        let (entry,used)=render_3d_entry_packets(0x4680,header);assert_eq!(used,19);
        assert_eq!(&entry[..7],&[0x7a000204,0x00100000,0,0,0,0,PIPELINE_SELECT_3D]);
        assert!(parse_entry(&entry[..used],true));assert!(parse_entry(&entry[..used],false));
        assert_eq!(&entry[7..13],&render_pipe_control_packet(true,header,PIPE_CONTROL_FLUSH_BITS,0,0));
        assert_eq!(&entry[13..19],&render_pipe_control_packet(true,0,PIPE_CONTROL_INVALIDATE_BITS,0,0));
    }
}
#[test]fn desktop_entry_bytes_and_headers_stay_exactly_unchanged() {
    TGL.store(false,Ordering::Release);
    for device in [0x4680,0xa780,0x1912] {
        for header in [0,PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER] {
            let (entry,used)=render_3d_entry_packets(device,header);assert_eq!(used,13);
            let expected=[render_pipe_control_packet(device_is_gfx12(device),header,PIPE_CONTROL_FLUSH_BITS,0,0).as_slice(),
                render_pipe_control_packet(device_is_gfx12(device),0,PIPE_CONTROL_INVALIDATE_BITS,0,0).as_slice(),
                &[PIPELINE_SELECT_3D]].concat();
            assert_eq!(&entry[..used],expected);assert_eq!(&entry[used..],&[0;6]);
        }
    }
}
#[test]fn breadcrumbs_derive_the_13_and_19_dword_prefixes_and_preserve_payload() {
    for native in [false,true] {
        let (mut batch,bytes,marker)=secondary(native);let before=batch.clone();
        let result=0x1_0084_0000u64;let index=37;
        assert_eq!(finish_resident_secondary_breadcrumbs(&mut batch,bytes,index,result,0x4680),Ok(bytes+16));
        assert_eq!(marker,if native {19} else {13});
        assert_eq!(&batch[4..4+marker],&before[4..4+marker]);
        assert_eq!(&batch[..4],&[MI_STORE_DATA_IMM_GGTT_DW1,(result+14*4)as u32,1,RCS_EXEC_RESULT_DRAW_BATCH_ENTRY+((index as u32)<<8)]);
        assert_eq!(&batch[4+marker..8+marker],&[MI_STORE_DATA_IMM_GGTT_DW1,(result+15*4)as u32,1,RCS_EXEC_RESULT_DRAW_POST_OPENING+((index as u32)<<8)]);
        assert_eq!(&batch[8+marker..],&before[8+marker..]);
    }
}
#[test]fn malformed_opening_or_marker_is_rejected_before_any_breadcrumb_write() {
    for native in [false,true] {
        let (batch,bytes,marker)=secondary(native);
        for offset in 4..8+marker {
            let mut corrupt=batch.clone();corrupt[offset]^=1;let before=corrupt.clone();
            assert_eq!(finish_resident_secondary_breadcrumbs(&mut corrupt,bytes,0,0x1_0084_0000,0x4680),Err("scene-frame-secondary-opening-layout"));
            assert_eq!(corrupt,before);
        }
        let mut short=batch.clone();assert!(finish_resident_secondary_breadcrumbs(&mut short,bytes-12,0,0x1_0084_0000,0x4680).is_err());
        assert_eq!(short,batch);
        assert!(finish_resident_secondary_breadcrumbs(&mut short,bytes-1,0,0x1_0084_0000,0x4680).is_err());
        assert_eq!(short,batch);
    }
}
#[test]fn actual_no_draw_encoder_uses_the_selected_entry_and_preserves_remaining_packets() {
    let mut encoded=Vec::new();
    for native in [false,true] {
        TGL.store(native,Ordering::Release);let mut batch=vec![0xfeedface;256];
        let warm=RenderWarmState{device_id:0x4680,draw_state_len:65536,vertex_len:4096};
        let bytes=encode_3d_no_draw_probe_batch(&mut batch,warm,0x840000,0xc0def00d,None).unwrap();
        let (entry,used)=render_3d_entry_packets(0x4680,0);
        assert_eq!(&batch[..used],&entry[..used]);
        assert_eq!(batch[used],STATE_BASE_ADDRESS_CMD);encoded.push(batch[used..bytes/4].to_vec());
    }
    assert_eq!(encoded[0],encoded[1]);
}
'''
with tempfile.TemporaryDirectory(prefix="trueos-render-3d-entry-") as directory:
    path = Path(directory)
    (path / "tests.rs").write_text(source)
    subprocess.run(["rustc", "--edition=2024", "--test", str(path / "tests.rs"),
                    "-o", str(path / "tests")], check=True, cwd=ROOT)
    subprocess.run([str(path / "tests"), "--test-threads=1"], check=True)
