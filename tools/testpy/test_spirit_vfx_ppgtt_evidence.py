#!/usr/bin/env python3
"""Exercise the production timeout-only PPGTT walk and MMIO attribution."""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item, constant


SPIRIT = "src/intel/gpgpu/operations/spirit_vfx.rs"
RUNTIME = "src/intel/gpgpu/rcs/runtime.rs"
CONSTANTS = "src/intel/gpgpu/rcs/constants.rs"

source = r'''
#![allow(dead_code)]
use std::cell::RefCell;
use std::collections::VecDeque;
thread_local! {
    static READS: RefCell<VecDeque<(usize,u32)>> = const {RefCell::new(VecDeque::new())};
    static LOGS: RefCell<Vec<String>> = const {RefCell::new(Vec::new())};
}
#[macro_export] macro_rules! log_error {
    (target:$target:expr; $($args:tt)*) => {
        crate::LOGS.with(|logs| logs.borrow_mut().push(format!($($args)*)))
    };
}
#[derive(Clone,Copy)] struct Dev;
const GEN8_PAGE_PRESENT:u64=1;
fn mmio_read(_:Dev,offset:usize)->u32 {
    READS.with(|reads| {
        let (expected,value)=reads.borrow_mut().pop_front().expect("unexpected MMIO read");
        assert_eq!(offset,expected); value
    })
}
mod diagnostic {
use std::collections::VecDeque;
'''
source += "\n".join(constant(CONSTANTS, name) for name in (
    "GEN8_PAGE_RW", "GEN8_PAGE_PWT", "GEN8_PAGE_PCD", "DIRECT_RCS_PPGTT_PT_COUNT",
    "DIRECT_RCS_PPGTT_BYTES", "DIRECT_RCS_PPGTT_LIMIT_BYTES", "RCS_RING_BASE", "RCS_RING_START",
    "SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_GPU", "SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES",
    "SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES", "SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES",
    "SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES",
))
source += "\n" + item(RUNTIME, "exact_ppgtt_pte_offset")
source += "\n".join(item(SPIRIT, name) for name in (
    "SpiritVfxBuffer", "SpiritVfxSubmitted", "SpiritVfxPpgttEvidence",
    "spirit_vfx_ppgtt_evidence", "spirit_vfx_log_mapping_evidence",
))
source += r'''
#[derive(Clone,Copy,Debug)] struct DirectRcsGpuVa {ring:u64,batch:u64}
#[derive(Clone,Copy,Debug)] struct DirectRcsState {
    ppgtt_virt:*mut u8,ppgtt_phys:u64,batch_phys:u64,gpu_va:DirectRcsGpuVa,
}
#[derive(Clone,Copy)] struct UploadedKernelArtifact {gpu:u64,phys:u64}
#[derive(Clone,Copy)] struct GpgpuRgba8Surface {gpu:u64,phys:u64}

fn table()->(Vec<u64>,DirectRcsState) {
    let mut bytes=vec![0u64;DIRECT_RCS_PPGTT_BYTES/8];
    let state=DirectRcsState {ppgtt_virt:bytes.as_mut_ptr().cast(),ppgtt_phys:0x100000000,
        batch_phys:0x1f00000,gpu_va:DirectRcsGpuVa{ring:0x8000000,batch:0x8100000}};
    bytes[0]=(state.ppgtt_phys+4096)|0x1b;
    bytes[4096/8]=(state.ppgtt_phys+8192)|0x1b;
    for index in 0..DIRECT_RCS_PPGTT_PT_COUNT {
        bytes[8192/8+index]=(state.ppgtt_phys+12288+index as u64*4096)|0x1b;
    }
    (bytes,state)
}
fn map(bytes:&mut [u64],gpu:u64,phys:u64,flags:u64) {
    bytes[exact_ppgtt_pte_offset(gpu&!0xfff,0).unwrap()/8]=(phys&!0xfff)|flags;
}

#[test] fn high_physical_pages_and_final_va_byte_preserve_raw_evidence() {
    let (mut bytes,state)=table();
    let gpu=DIRECT_RCS_PPGTT_LIMIT_BYTES-1;
    let phys=0x201234fff;
    map(&mut bytes,gpu,phys,0x1b|0x60);
    bytes[0]|=0x20;bytes[4096/8]|=0x20;bytes[8192/8+511]|=0x20;
    let result=spirit_vfx_ppgtt_evidence(state,gpu,phys,0x1b).unwrap();
    assert_eq!(result.read_levels,4);
    assert!(result.topology_match&&result.leaf_match);
    assert_eq!(result.entries[3],(phys&!0xfff)|0x7b,"retain accessed/dirty bits");
}

#[test] fn corrupt_parent_stops_before_any_unowned_or_lower_level_read() {
    let (mut bytes,state)=table();let gpu=0x90a0000;
    let offsets=[0,4096/8,8192/8+(gpu>>21)as usize];
    for (level,offset) in offsets.into_iter().enumerate() {
        let original=bytes[offset];
        for corrupt in [original&!1,original|0x80,0xdeadbeef000|0x1b] {
            bytes[offset]=corrupt;
            let result=spirit_vfx_ppgtt_evidence(state,gpu,0x18613000,3).unwrap();
            assert_eq!(result.read_levels,level as u8+1);
            assert_eq!(result.entries[level],corrupt);
            assert!(result.entries[level+1..].iter().all(|&raw|raw==0));
            assert!(!result.topology_match&&!result.leaf_match);
        }
        bytes[offset]=original;
    }
}

#[test] fn leaf_absence_wrong_physical_and_wrong_pat_are_distinct_raw_results() {
    let (mut bytes,state)=table();let gpu=0x6000000;let phys=0x184d7000;
    for (raw,expected,valid) in [(0,0x1b,false),(phys|3,0x1b,false),
        ((phys+4096)|0x1b,0x1b,false),(phys|0x1b,0x1b,true),
        (phys|0x83,3,false),(phys|0x9b,0x1b,false)] {
        bytes[exact_ppgtt_pte_offset(gpu,0).unwrap()/8]=raw;
        let result=spirit_vfx_ppgtt_evidence(state,gpu,phys,expected).unwrap();
        assert_eq!(result.entries[3],raw);
        assert!(result.topology_match);assert_eq!(result.leaf_match,valid);
    }
}

#[test] fn invalid_inputs_do_not_touch_a_null_or_misaligned_table() {
    let (_bytes,state)=table();
    for gpu in [DIRECT_RCS_PPGTT_LIMIT_BYTES,u64::MAX] {
        assert!(spirit_vfx_ppgtt_evidence(state,gpu,0x1000,3).is_none());
    }
    let mut invalid=state;invalid.ppgtt_virt=core::ptr::null_mut();
    assert!(spirit_vfx_ppgtt_evidence(invalid,0x1000,0x1000,3).is_none());
    invalid.ppgtt_virt=1usize as *mut u8;
    assert!(spirit_vfx_ppgtt_evidence(invalid,0x1000,0x1000,3).is_none());
    invalid.ppgtt_virt=(usize::MAX&!7) as *mut u8;
    assert!(spirit_vfx_ppgtt_evidence(invalid,0x1000,0x1000,3).is_none());
    invalid=state;invalid.ppgtt_phys=u64::MAX&!0xfff;
    assert!(spirit_vfx_ppgtt_evidence(invalid,0x1000,0x1000,3).is_none());
    assert!(spirit_vfx_ppgtt_evidence(state,0x1001,0x1000,3).is_none());
}

#[test] fn live_root_is_attributed_only_with_stable_matching_spirit_ring() {
    let (mut bytes,state)=table();
    let kernel=UploadedKernelArtifact{gpu:0xd430000,phys:0x18557000};
    let control=SpiritVfxBuffer{gpu:0x90a0000,phys:0x18613000,virt:core::ptr::null_mut(),bytes:4096};
    let dst=GpgpuRgba8Surface{gpu:0x6000000,phys:0x184d7000};
    for (gpu,phys,flags) in [(kernel.gpu,kernel.phys,3),(control.gpu,control.phys,3),
        (dst.gpu,dst.phys,0x1b),(state.gpu_va.batch+0x6000,state.batch_phys+0x6000,3)] {
        map(&mut bytes,gpu,phys,flags);
    }
    let submitted=SpiritVfxSubmitted{state,first_kernel:kernel,control,source:dst,dst,
        submit_ambiguous:false,frame:0,revision:0,background_mode:11,shader_mode:1,started_tick:0};
    for (before,after,hi_before,hi_after,expected) in [
        (0x8000000,0x8000000,1,1,"Some(true)"),
        (0x8000000,0x800000,1,1,"None"),
        (0x800000,0x800000,1,1,"None"),
        (0x8000000,0x8000000,0,1,"None"),
        (0x8000000,0x8000000,0,0,"Some(false)"),
    ] {
        crate::READS.with(|reads|*reads.borrow_mut()=VecDeque::from([
            (0x2038,before),(0x2274,hi_before),(0x2270,0),(0x2274,hi_after),
            (0x20e4,4),(0xb134,0xb0000040),(0x2038,after)]));
        crate::LOGS.with(|logs|logs.borrow_mut().clear());
        spirit_vfx_log_mapping_evidence(crate::Dev,submitted);
        crate::READS.with(|reads|assert!(reads.borrow().is_empty()));
        crate::LOGS.with(|logs| {
            let logs=logs.borrow();assert_eq!(logs.len(),6,"one summary and five bounded records");
            assert!(logs[0].contains(&format!("root_matches={expected}")));
            assert!(logs[0].contains("preemption_20e4=0x00000004 l3alloc_b134=0xB0000040"));
            for line in &logs[1..] {assert!(line.contains("leaf_match: true"),"{line}");}
        });
    }
}
}
'''

with tempfile.TemporaryDirectory(prefix="trueos-spirit-ppgtt-evidence-") as temporary:
    src, executable = Path(temporary) / "tests.rs", Path(temporary) / "tests"
    src.write_text(source)
    subprocess.run(["rustc", "--edition=2024", "--test", str(src), "-o", str(executable)], check=True)
    subprocess.run([str(executable)], check=True)
