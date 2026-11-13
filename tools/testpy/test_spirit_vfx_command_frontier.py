#!/usr/bin/env python3
"""Decode actual Spirit batches and all live RCS compute-entry boundaries.

Compile the complete production encoder, descriptor/payload writers and command
helpers. Artifact bytes and DMA cache maintenance are host fixtures; commands,
resource layouts and all command constants come from the kernel source.
"""

from pathlib import Path
import json
import re
import subprocess
import tempfile

import test_clip_position3_uv_texture as production


ROOT = Path(__file__).resolve().parents[2]
production.ROOT = ROOT


def compute_entry_calls() -> str:
    sites = (
        ("effects", "direct_rcs_encode_skybox_sample_rgb565_batch"),
        ("two_d", "direct_rcs_finish_two_buffer_dispatch_batch"),
        ("shadertoy", "direct_rcs_encode_shadertoy_batch"),
        ("cpp_audio_visualizer", "direct_rcs_encode_cpp_audio_visualizer_rgba8_batch"),
        ("particle_craft", "direct_rcs_encode_particle_craft_batch"),
        ("cpp_demo", "direct_rcs_encode_cpp_demo_rgba8_batch"),
        ("lz4_blocks", "encode_lz4_batch"),
        ("worklists", "direct_rcs_encode_rect_worklist_command_stream"),
        ("worklists", "direct_rcs_push_gpgpu_prologue_with_instruction_base"),
        ("spirit_vfx", "direct_rcs_encode_spirit_vfx_batch"),
    )
    wrappers = []
    entries = []
    for index, (file, name) in enumerate(sites):
        source = production.item(f"src/intel/gpgpu/rcs/{file}.rs", name)
        marker = "direct_rcs_push_compute_3d_entry("
        assert source.count(marker) == 1, f"{name} must use one explicit mode entry"
        start = source.index(marker)
        prefix = source[:start]
        assert "PIPELINE_SELECT_GPGPU" not in prefix, name
        assert "direct_rcs_push_pipe_control_full(" not in prefix, name
        depth, end = 1, start + len(marker)
        while depth:
            depth += (source[end] == "(") - (source[end] == ")")
            end += 1
        call = source[start:end].replace("&mut cursor", "cursor")
        wrappers.append(f"fn entry_{index}(batch:&mut[u32],cursor:&mut usize)->bool {{{call}}}")
        full_flush = file == "spirit_vfx" or name == "direct_rcs_push_gpgpu_prologue_with_instruction_base"
        entries.append(f'("{name}",entry_{index} as Entry,{str(full_flush).lower()})')
    return "\n".join(wrappers) + "\ntype Entry=fn(&mut[u32],&mut usize)->bool;\n" + (
        f"fn entry_sites()->[(&'static str,Entry,bool);{len(entries)}] {{[" + ",".join(entries) + "]}"
    )


def harness_source() -> str:
    commands = "src/intel/gpgpu/rcs/commands.rs"
    payloads = "src/intel/gpgpu/rcs/payloads.rs"
    spirit = "src/intel/gpgpu/rcs/spirit_vfx.rs"
    constants = "src/intel/gpgpu/rcs/constants.rs"
    declarations = [production.item(commands, name) for name in (
        "direct_rcs_push", "direct_rcs_push_pipe_control_full",
        "direct_rcs_push_pipe_control", "direct_rcs_push_state_base_address",
        "direct_rcs_push_state_base_address_with_mocs_index",
        "direct_rcs_push_sba_address", "direct_rcs_push_sba_size",
        "direct_rcs_push_store_marker_at", "direct_rcs_push_gpgpu_walker_2d",
        "direct_rcs_push_pipe_control_post_sync_marker_at",
    )]
    declarations.extend(production.item(payloads, name) for name in (
        "direct_rcs_write_interface_descriptor_at",
        "direct_rcs_write_buffer_surface_state",
        "direct_rcs_write_buffer_surface_state_with_mocs_index",
    ))
    declarations.extend(production.item(spirit, name) for name in (
        "direct_rcs_write_spirit_vfx_binding", "direct_rcs_write_spirit_vfx_payload",
        "direct_rcs_push_spirit_vfx_idd_load", "direct_rcs_push_spirit_vfx_dependency",
        "direct_rcs_encode_spirit_vfx_batch",
    ))
    declarations.append(production.item("src/intel/gpgpu/rcs/worklists.rs",
        "direct_rcs_push_compute_3d_entry"))
    declarations.append(production.item("src/intel/gpgpu/rcs/worklists.rs",
        "direct_rcs_push_gpgpu_prologue_with_instruction_base"))
    declarations.append(compute_entry_calls())
    declarations.append(production.item(constants, "direct_rcs_encode_mocs_index"))
    declarations.extend(production.constant(spirit, name) for name in (
        "SPIRIT_VFX_STATE_COMPUTE_MODE_CMD", "SPIRIT_VFX_FORCE_GPU_NON_COHERENT",
        "SPIRIT_VFX_TGL_VFE_URB_DW5",
    ))
    # Follow references so changing any production constant changes the batch
    # under test. Artifact inputs below are separate from command encoding.
    pending = set(re.findall(r"\b[A-Z][A-Z0-9_]+\b", "\n".join(declarations)))
    extracted = {}
    while pending:
        name = pending.pop()
        if name in extracted:
            continue
        try:
            declaration = production.constant(constants, name)
        except ValueError:
            continue
        extracted[name] = declaration
        pending.update(set(re.findall(r"\b[A-Z][A-Z0-9_]+\b", declaration)) - extracted.keys())
    policy = "#[path=" + json.dumps(str(ROOT / "src/intel/render/fresh_context.rs")) + "] mod fresh_context;\n"
    return "#![allow(dead_code)]\n" + policy + r"""
fn dma_flush(_: *mut u8, _: usize) {}
mod intel {
    #[derive(Clone,Copy)] pub struct Dev {pub bus:u8,pub slot:u8,pub function:u8}
    pub fn claimed_device()->Option<Dev> {Some(Dev{bus:0,slot:2,function:0})}
    pub mod render {pub(crate) use crate::fresh_context::tgl_spirit_compute_contract;}
}
mod pci {
    std::thread_local! {
        pub static ID:std::cell::Cell<u32>=const {std::cell::Cell::new(0x4680_8086)};
        pub static REV:std::cell::Cell<u8>=const {std::cell::Cell::new(0x0c)};
    }
    pub fn config_read_u32(bus:u8,slot:u8,function:u8,offset:u8)->u32 {
        assert_eq!((bus,slot,function),(0,2,0));
        match offset {0=>ID.with(std::cell::Cell::get),8=>REV.with(std::cell::Cell::get)as u32,_=>panic!("PCI offset")}
    }
}
fn physical(vendor:u16,device:u16,revision:u8) {
    pci::ID.with(|id|id.set((device as u32)<<16|vendor as u32));
    pci::REV.with(|rev|rev.set(revision));
}
mod encoder {
""" + (
        "\n".join(extracted.values()) + "\n" + "\n".join(declarations)
    ) + r"""
#[derive(Clone, Copy)] struct DirectRcsGpuVa { batch:u64, result:u64 }
#[derive(Clone, Copy)] struct DirectRcsState {
    batch_virt:*mut u8, result_virt:*mut u8, gpu_va:DirectRcsGpuVa,
}
#[derive(Clone, Copy, PartialEq)] enum GpgpuArtifactAddressSpace { CallerPpgtt }
#[derive(Clone, Copy)] struct UploadedKernelArtifact {
    bin_sha256:&'static str, gpu:u64, bytes:usize, address_space:GpgpuArtifactAddressSpace,
}
#[derive(Clone, Copy)] struct SpiritVfxBuffer {gpu:u64, bytes:usize}
#[derive(Clone, Copy)] struct GpgpuRgba8Surface {
    gpu:u64, bytes:usize, width:u32, height:u32, pitch_bytes:u32,
}
struct ArtifactContract {entry_offset:u64}
const SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_CPP_ABI_CONTRACT:ArtifactContract=ArtifactContract{entry_offset:64};
const SPIRIT_VFX_SPRITE_RGBA8_ADLS_CPP_ABI_CONTRACT:ArtifactContract=ArtifactContract{entry_offset:64};
const SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN_SHA256:&str="background-fixture";
const SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN_SHA256:&str="sprite-fixture";
const SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN:&[u8]=&[0];
const SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN:&[u8]=&[0];
fn align_up(value:usize,alignment:usize)->Option<usize> {
    value.checked_add(alignment-1).map(|rounded|rounded&!(alignment-1))
}
fn encoded(background:bool)->Vec<u32> {
    let mut batch=vec![0xfeedfaceu32;DIRECT_RCS_BATCH_BYTES/4];
    let mut result=vec![0xfeedfaceu32;DIRECT_RCS_RESULT_BYTES/4];
    let state=DirectRcsState{batch_virt:batch.as_mut_ptr().cast(),
        result_virt:result.as_mut_ptr().cast(),
        gpu_va:DirectRcsGpuVa{batch:0x08100000,result:0x08200000}};
    let background_upload=background.then_some(UploadedKernelArtifact {
        bin_sha256:SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN_SHA256,
        gpu:SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_GPU,
        bytes:SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN.len(),
        address_space:GpgpuArtifactAddressSpace::CallerPpgtt,
    });
    let sprite=UploadedKernelArtifact {
        bin_sha256:SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN_SHA256,
        gpu:SPIRIT_VFX_SPRITE_RGBA8_ADLS_GPU,
        bytes:SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN.len(),
        address_space:GpgpuArtifactAddressSpace::CallerPpgtt,
    };
    let control=SpiritVfxBuffer{gpu:0x090a0000,bytes:4096};
    let source=GpgpuRgba8Surface{gpu:0x30000000,bytes:128*128*4,
        width:128,height:128,pitch_bytes:128*4};
    let dst=GpgpuRgba8Surface{gpu:0x06000000,bytes:256*256*4,
        width:256,height:256,pitch_bytes:256*4};
    assert!(direct_rcs_encode_spirit_vfx_batch(state,background_upload,sprite,control,source,dst));
    batch
}
fn command_offsets(batch:&[u32])->Vec<usize> {
    let mut offsets=Vec::new();let mut cursor=0;
    while cursor<1024 {
        let header=batch[cursor];offsets.push(cursor);
        if header==MI_BATCH_BUFFER_END {return offsets;}
        let length=match header {
            PIPELINE_SELECT_3D|PIPELINE_SELECT_GPGPU|MI_NOOP=>1,
            MEDIA_VFE_STATE_CMD|MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD|GPGPU_WALKER_CMD|
            MEDIA_STATE_FLUSH_CMD|STATE_BASE_ADDRESS_CMD|MI_STORE_DATA_IMM_GGTT_DW1|
            SPIRIT_VFX_STATE_COMPUTE_MODE_CMD=>(header&0xff)as usize+2,
            _ if header&0xffff0000==PIPE_CONTROL_CMD&0xffff0000=>(header&0xff)as usize+2,
            _=>panic!("unexpected command at {cursor:#x}: {header:#x}"),
        };
        cursor+=length;
    }
    panic!("batch never terminates");
}

#[test]
fn observed_bbaddr_identifies_background_walker_not_retained_ipehr() {
    let batch=encoded(true);let offsets=command_offsets(&batch);
    let walkers:Vec<_>=offsets.iter().copied().filter(|&i|batch[i]==GPGPU_WALKER_CMD).collect();
    assert_eq!(walkers,vec![0x120/4,0x18c/4]);
    let bbaddr=0x08100121u32;
    assert_eq!(bbaddr&1,1,"hardware says batch pointer valid");
    let offset=((bbaddr&!3)-0x08100000)as usize/4;
    assert_eq!(offset,walkers[0]);
    assert_eq!(batch[offset],0x7105000d);
    assert_eq!(batch[0xdc/4],MEDIA_VFE_STATE_CMD);
    assert_eq!(batch[0x100/4],MI_STORE_DATA_IMM_GGTT_DW1);
    assert_eq!(batch[0x100/4+3],SPIRIT_VFX_PRE_MARKER);
    // Each walker has its own matching indirect payload and descriptor load.
    for (walker,payload,bytes,idd) in [
        (walkers[0],SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES,SPIRIT_VFX_BACKGROUND_INDIRECT_BYTES,
            SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES),
        (walkers[1],SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES,SPIRIT_VFX_SPRITE_INDIRECT_BYTES,
            SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES),
    ] {
        assert_eq!(batch[walker-4],MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
        assert_eq!(batch[walker-1],idd as u32);
        assert_eq!(batch[walker+2],bytes as u32);
        assert_eq!(batch[walker+3],payload as u32);
    }
}

#[test]
fn both_variants_stall_before_vfe_and_flush_before_entering_compute() {
    for background in [false,true] {
        let batch=encoded(background);let offsets=command_offsets(&batch);
        for (position,&offset) in offsets.iter().enumerate() {
            if batch[offset]==MEDIA_VFE_STATE_CMD {
                let previous=offsets[position-1];
                assert_eq!(previous+6,offset);
                assert_eq!(batch[previous]&0xffff0000,PIPE_CONTROL_CMD&0xffff0000);
                assert_ne!(batch[previous+1]&PIPE_CONTROL_CS_STALL,0);
            }
            if batch[offset]==PIPELINE_SELECT_GPGPU {
                let previous=offsets[position-1];
                assert_eq!(previous+6,offset);
                assert_ne!(batch[previous]&PIPE_CONTROL_HDC_PIPELINE_FLUSH,0);
                assert_ne!(batch[previous+1]&PIPE_CONTROL_CS_STALL,0);
                // The final transition follows SBA/invalidation. Its RT/depth
                // flush occurs earlier in the same 3D interval, before SBA.
                let start=offsets[..position].iter().rposition(|&i|batch[i]==PIPELINE_SELECT_3D)
                    .map(|p|p+1).unwrap_or(0);
                let required=PIPE_CONTROL_CS_STALL|PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH|
                    PIPE_CONTROL_DEPTH_CACHE_FLUSH;
                assert!(offsets[start..position].iter().any(|&i|
                    batch[i]&0xffff0000==PIPE_CONTROL_CMD&0xffff0000&&
                    batch[i]&PIPE_CONTROL_HDC_PIPELINE_FLUSH!=0&&batch[i+1]&required==required));
            }
        }
    }
}

#[test]
fn sprite_only_keeps_one_walker_and_uses_sprite_instruction_base() {
    let batch=encoded(false);let offsets=command_offsets(&batch);
    let walkers:Vec<_>=offsets.iter().copied().filter(|&i|batch[i]==GPGPU_WALKER_CMD).collect();
    assert_eq!(walkers,vec![0x120/4]);
    let walker=walkers[0];
    assert_eq!(batch[walker-1],SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES as u32);
    let sba=*offsets.iter().find(|&&i|batch[i]==STATE_BASE_ADDRESS_CMD).unwrap();
    assert_eq!(batch[sba+10]&0xfffff000,SPIRIT_VFX_SPRITE_RGBA8_ADLS_GPU as u32);
    assert_eq!(batch[SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES/4],64);
    assert_eq!(batch[SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES/4+2],IDD_THREAD_PREEMPTION_DISABLE);
}

#[test]
fn tgl_contract_changes_only_entry_sequence_compute_mode_and_vfe_urb() {
    for background in [false,true] {
        super::physical(0x8086,0x4680,12);let legacy=encoded(background);
        let legacy_offsets=command_offsets(&legacy);
        let sba=*legacy_offsets.iter().find(|&&i|legacy[i]==STATE_BASE_ADDRESS_CMD).unwrap();
        super::physical(0x8086,0x9a49,1);let tgl=encoded(background);
        let offsets=command_offsets(&tgl);
        let scm=sba-7;
        assert_eq!(&tgl[scm..scm+2],&[0x61050000,0x00180010]);
        assert_eq!(tgl[scm+2],STATE_BASE_ADDRESS_CMD);
        let position=offsets.iter().position(|&i|i==scm).unwrap();
        let flush=offsets[position-1];let select=offsets[position-2];
        assert_eq!(tgl[select],PIPELINE_SELECT_3D);
        assert_eq!(tgl[flush]&0xffff0000,PIPE_CONTROL_CMD&0xffff0000);
        assert_ne!(tgl[flush]&PIPE_CONTROL_HDC_PIPELINE_FLUSH,0);
        assert_ne!(tgl[flush+1]&PIPE_CONTROL_CS_STALL,0);
        let end=*legacy_offsets.last().unwrap()+1;
        let mut expected=legacy.clone();
        expected[..end].fill(0);
        // Omit the unsafe leading RT/depth flush and forced GPGPU select.
        // Everything from the common HDC stall through 3D is retained.
        expected[..scm].copy_from_slice(&legacy[7..sba]);
        expected[scm..scm+2].copy_from_slice(&[0x61050000,0x00180010]);
        expected[scm+2..scm+2+end-sba].copy_from_slice(&legacy[sba..end]);
        let vfe=*offsets.iter().find(|&&i|tgl[i]==MEDIA_VFE_STATE_CMD).unwrap();
        expected[vfe+5]=0x04000000;
        assert_eq!(tgl,expected,"all other commands, descriptors and payload bytes stay identical");
        let first_walker=*offsets.iter().find(|&&i|tgl[i]==GPGPU_WALKER_CMD).unwrap();
        assert_eq!(first_walker*4,0x10c,"known-mode entry removes7DWORDs from the prior TGL stream");
        // STATE_COMPUTE_MODE is masked: change only ForceNonCoherent bits4:3.
        let write=tgl[scm+1];let mask=write>>16;
        assert_eq!(mask,0x18);
        for old in 0..=u16::MAX as u32 {
            let after=(old&!mask)|(write&mask);
            assert_eq!((after>>3)&3,2);
            assert_eq!(after&!mask,old&!mask);
        }
    }
}

#[test]
fn tgl_entry_is_valid_from_cold_3d_and_restored_gpgpu_without_3d_flush_in_compute() {
    super::physical(0x8086,0x9a49,1);
    for background in [false,true] {
        let batch=encoded(background);let offsets=command_offsets(&batch);
        assert_eq!(batch[0]&0xffff0000,PIPE_CONTROL_CMD&0xffff0000);
        assert_ne!(batch[0]&PIPE_CONTROL_HDC_PIPELINE_FLUSH,0);
        assert_eq!(batch[1],PIPE_CONTROL_CS_STALL);
        assert_eq!(batch[6],PIPELINE_SELECT_3D);
        for initial in [PIPELINE_SELECT_3D,PIPELINE_SELECT_GPGPU] {
            let mut mode=initial;let mut entered_3d=false;let mut flushed_3d=false;
            let mut compute_entries=0;
            for &offset in &offsets {
                let header=batch[offset];
                if header&0xffff0000==PIPE_CONTROL_CMD&0xffff0000 {
                    let flags=batch[offset+1];
                    let graphics=PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH|
                        PIPE_CONTROL_DEPTH_CACHE_FLUSH|PIPE_CONTROL_DEPTH_STALL;
                    if mode==PIPELINE_SELECT_GPGPU {
                        assert_eq!(flags&graphics,0,"3D flush in restored GPGPU at{offset:#x}");
                    }
                    if flags&graphics!=0 {
                        assert!(entered_3d,"initial mode must never be assumed");
                        assert_eq!(mode,PIPELINE_SELECT_3D);
                        assert_ne!(flags&PIPE_CONTROL_CS_STALL,0);
                        assert_ne!(header&PIPE_CONTROL_HDC_PIPELINE_FLUSH,0);
                        flushed_3d=true;
                    }
                } else if header==PIPELINE_SELECT_3D {
                    entered_3d=true;mode=header;
                } else if header==PIPELINE_SELECT_GPGPU {
                    assert!(entered_3d&&flushed_3d,"3D caches drain before entering compute");
                    compute_entries+=1;mode=header;
                } else if matches!(header,STATE_BASE_ADDRESS_CMD|SPIRIT_VFX_STATE_COMPUTE_MODE_CMD) {
                    assert!(entered_3d);assert_eq!(mode,PIPELINE_SELECT_3D);
                } else if matches!(header,MEDIA_VFE_STATE_CMD|MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD|GPGPU_WALKER_CMD) {
                    assert_eq!(mode,PIPELINE_SELECT_GPGPU);
                }
            }
            assert_eq!(compute_entries,1,"no speculative initial compute roundtrip");
        }
    }
}

#[test]
fn tgl_vfe_allocation_covers_each_real_walker_payload_within_32kib() {
    super::physical(0x8086,0x9a49,1);
    for background in [false,true] {
        let batch=encoded(background);let offsets=command_offsets(&batch);
        let vfe=*offsets.iter().find(|&&i|batch[i]==MEDIA_VFE_STATE_CMD).unwrap();
        let dw3=batch[vfe+3];let dw5=batch[vfe+5];
        assert_eq!((dw3>>16)+1,168,"front-end thread limit stays unchanged");
        let entries=(dw3>>8)&0xff;
        let entry_bytes=((dw5>>16)*32+63)&!63;
        let curbe_bytes=((dw5&0xffff)*32+63)&!63;
        assert_eq!(entries,1);
        assert_eq!(curbe_bytes,0,"indirect payloads do not allocate CURBE");
        assert_eq!(dw5,0x04000000,"match Intel NEO's Gen12LP VFE allocation");
        assert!(entries*entry_bytes+curbe_bytes<=32*1024);
        let mut payload_sizes=Vec::new();
        for walker in offsets.iter().copied().filter(|&i|batch[i]==GPGPU_WALKER_CMD) {
            assert_eq!(batch[walker-4],MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
            let idd=batch[walker-1]as usize/4;
            let group_threads=batch[idd+6]&0x3ff;
            let dimensions=batch[walker+4];
            let thread_grid=((dimensions&0x3f)+1)*(((dimensions>>8)&0x3f)+1)*
                (((dimensions>>16)&0x3f)+1);
            assert_eq!(group_threads,thread_grid);
            assert_eq!(group_threads,1,"SIMD16 is sixteen workitems in one hardware thread");
            assert_eq!(dimensions>>30,1,"walker and baked kernels stay SIMD16");
            assert_eq!(batch[idd+2],IDD_THREAD_PREEMPTION_DISABLE);
            assert_eq!(batch[idd+6]>>16,0,"no SLM or barrier is introduced");
            let per_thread_grfs=batch[idd+5]>>16;
            let cross_thread_grfs=batch[idd+7]&0xff;
            let payload=(per_thread_grfs*group_threads+cross_thread_grfs)*32;
            assert_eq!(payload,batch[walker+2],"IDD reads exactly the walker indirect payload");
            assert!(entry_bytes>=payload,"VFE URB entry must cover indirect data");
            payload_sizes.push(payload);
        }
        assert_eq!(payload_sizes,if background {vec![160,192]}else{vec![192]});
    }
}

#[test]
fn desktop_and_other_physical_targets_preserve_complete_legacy_batch_bytes() {
    for background in [false,true] {
        super::physical(0x8086,0x4680,12);let legacy=encoded(background);
        // The larger knot artifact moved the sprite instruction window from
        // D450000 to D470000. Normalize only that reviewed relocation before
        // comparing every byte with the pre-TGL encoder snapshot.
        let mut normalized=legacy.clone();
        if background {
            let sprite_entry=SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES/4;
            assert_eq!(legacy[sprite_entry],SPIRIT_VFX_SPRITE_TEXT_OFFSET_BYTES as u32);
            normalized[sprite_entry]=0x20040;
        } else {
            let offsets=command_offsets(&legacy);
            let sba=*offsets.iter().find(|&&i|legacy[i]==STATE_BASE_ADDRESS_CMD).unwrap();
            assert_eq!(legacy[sba+10]&0xfffff000,SPIRIT_VFX_SPRITE_RGBA8_ADLS_GPU as u32);
            normalized[sba+10]=0x0d450000|(legacy[sba+10]&0xfff);
        }
        let hash=normalized.iter().flat_map(|dw|dw.to_le_bytes()).fold(0xcbf29ce484222325u64,
            |h,b|(h^u64::from(b)).wrapping_mul(0x100000001b3));
        assert_eq!(hash,if background {0x8429439a404d7777}else{0xee095d261120bd81});
        for (vendor,device,revision) in [(0x8086,0x4680,12),(0x8086,0xa780,4),
            (0x8086,0x9a49,0),(0x8086,0x9a49,2),(0x1234,0x9a49,1)] {
            super::physical(vendor,device,revision);
            assert_eq!(encoded(background),legacy);
        }
    }
}

fn entry_bytes(entry:Entry)->Vec<u32> {
    let mut words=vec![0xf00dcafe;64];let mut cursor=0;
    assert!(entry(&mut words,&mut cursor));
    assert!(words[cursor..].iter().all(|&word|word==0xf00dcafe));
    words.truncate(cursor);words
}

#[test]
fn every_live_compute_entry_is_safe_from_both_initial_modes() {
    super::physical(0x8086,0x9a49,1);
    for (name,entry,_) in entry_sites() {
        let words=entry_bytes(entry);
        assert_eq!(words.len(),13,"{name}: no speculative GPGPU roundtrip");
        assert_eq!(words[0],0x7a000204,"{name}: HDC flush, no reserved bit11");
        assert_eq!(words[1],0x00100000,"{name}: initial CS stall only");
        assert_eq!(words[6],0x69041310,"{name}: explicit 3D select");
        assert_eq!(words[7],0x7a000204);
        assert_eq!(words[8],0x00103001,"{name}: full RT/depth/HDC stalled flush");
        for initial in [PIPELINE_SELECT_3D,PIPELINE_SELECT_GPGPU] {
            let mut mode=initial;let mut explicit=false;let mut cursor=0;
            while cursor<words.len() {
                if words[cursor]==PIPELINE_SELECT_3D {
                    mode=PIPELINE_SELECT_3D;explicit=true;cursor+=1;continue;
                }
                assert_eq!(words[cursor]&0xffff0000,PIPE_CONTROL_CMD&0xffff0000);
                assert_eq!(words[cursor]&(1<<11),0,"{name}: Gen12 reserved bit11");
                assert_ne!(words[cursor+1]&PIPE_CONTROL_CS_STALL,0);
                if words[cursor+1]&(PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH|
                    PIPE_CONTROL_DEPTH_CACHE_FLUSH|PIPE_CONTROL_DEPTH_STALL)!=0 {
                    assert!(explicit,"{name}: cannot assume initial 3D");
                    assert_eq!(mode,PIPELINE_SELECT_3D);
                }
                cursor+=6;
            }
            assert_eq!(mode,PIPELINE_SELECT_3D);
        }
    }
}

#[test]
fn every_legacy_compute_prefix_preserves_its_exact_original_words() {
    for (name,entry,full) in entry_sites() {
        // Original hardware command words, including legacy callers' bit11.
        let (header,first,last)=if full {(0x7a000204,0x00103001,0x00103001)}
            else {(0x7a000a04,0x00101001,0x00101000)};
        let expected=[header,first,0,0,0,0,0x69041312,
            0x7a000204,0x00100000,0,0,0,0,0x69041310,header,last,0,0,0,0];
        for (vendor,device,revision) in [(0x8086,0x4680,12),(0x8086,0xa780,4),
            (0x8086,0x9a49,0),(0x8086,0x9a49,2),(0x1234,0x9a49,1)] {
            super::physical(vendor,device,revision);
            assert_eq!(entry_bytes(entry),expected,"{name}: legacy target changed");
        }
    }
}

#[test]
fn native_shared_prologue_enters_compute_only_after_3d_sba_and_cache_release() {
    for tgl in [false,true] {
        super::physical(0x8086,if tgl {0x9a49}else{0x4680},if tgl {1}else{12});
        let mut words=vec![0;1024];let mut cursor=0;
        assert!(direct_rcs_push_gpgpu_prologue_with_instruction_base(&mut words,&mut cursor,
            0x08900000,0x0d900000,GPGPU_VFE_DW5_UOS));
        words[cursor]=MI_BATCH_BUFFER_END;
        let offsets=command_offsets(&words);
        let sba=*offsets.iter().find(|&&i|words[i]==STATE_BASE_ADDRESS_CMD).unwrap();
        let entry=if tgl {13}else{20};
        assert_eq!(sba,entry);
        assert_eq!(words[sba+10]&0xfffff000,0x0d900000);
        let end=offsets.iter().position(|&i|words[i]==MEDIA_VFE_STATE_CMD).unwrap();
        let vfe=offsets[end];
        assert_eq!(words[vfe+5],GPGPU_VFE_DW5_UOS,"no VFE change outside Spirit");
        let select=offsets[end-2];let stall=offsets[end-1];
        assert_eq!(words[select],PIPELINE_SELECT_GPGPU);
        assert_eq!(words[stall+1],PIPE_CONTROL_CS_STALL);
        assert_ne!(words[stall]&PIPE_CONTROL_HDC_PIPELINE_FLUSH,0);
        assert_eq!(words[offsets[end-3]+1],PIPE_CONTROL_INVALIDATE_BITS);
        assert_eq!(offsets.iter().filter(|&&i|words[i]==PIPELINE_SELECT_GPGPU).count(),
            if tgl {1}else{2});
    }
}
}
"""


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-frontier-") as temporary:
        directory = Path(temporary)
        rust_source = directory / "tests.rs"
        executable = directory / "tests"
        rust_source.write_text(harness_source())
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust_source),
                        "-o", str(executable)], cwd=ROOT, check=True)
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
