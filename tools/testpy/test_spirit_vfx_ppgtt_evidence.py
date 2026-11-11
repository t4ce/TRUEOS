#!/usr/bin/env python3
"""Exercise owned encoding snapshots, PPGTT walks and MMIO attribution."""

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
    static FLUSHES: RefCell<Vec<(usize,usize)>> = const {RefCell::new(Vec::new())};
    static LOGS: RefCell<Vec<String>> = const {RefCell::new(Vec::new())};
}
#[macro_export] macro_rules! log_error {
    (target:$target:expr; $($args:tt)*) => {
        crate::LOGS.with(|logs| logs.borrow_mut().push(format!($($args)*)))
    };
}
#[macro_export] macro_rules! log_info {
    (target:$target:expr; $($args:tt)*) => { crate::log_error!(target:$target; $($args)*) };
}
#[macro_export] macro_rules! log_important {
    (target:$target:expr; $($args:tt)*) => { crate::log_error!(target:$target; $($args)*) };
}
fn dma_flush(virt:*mut u8,bytes:usize) {
    FLUSHES.with(|flushes|flushes.borrow_mut().push((virt as usize,bytes)));
}
mod intel {
pub fn claimed_device()->Option<crate::Dev> {Some(crate::Dev)}
pub fn mmio_read(dev:crate::Dev,offset:usize)->u32 {crate::mmio_read(dev,offset)}
pub mod render {
    std::thread_local! {pub static TGL:std::cell::Cell<bool>=const {std::cell::Cell::new(true)};}
    pub fn tgl_spirit_compute_contract()->bool {TGL.with(std::cell::Cell::get)}
}}
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
use std::sync::atomic::{AtomicU32,AtomicU64,Ordering};
'''
source += "\n".join(constant(CONSTANTS, name) for name in (
    "GEN8_PAGE_RW", "GEN8_PAGE_PWT", "GEN8_PAGE_PCD", "DIRECT_RCS_PPGTT_PD_COUNT",
    "DIRECT_RCS_PPGTT_PT_COUNT", "DIRECT_RCS_PPGTT_PML4_OFFSET",
    "DIRECT_RCS_PPGTT_PDPT_OFFSET", "DIRECT_RCS_PPGTT_PD_OFFSET", "DIRECT_RCS_PPGTT_PT_OFFSET",
    "DIRECT_RCS_PPGTT_BYTES", "DIRECT_RCS_PPGTT_LIMIT_BYTES", "RCS_RING_BASE", "RCS_RING_START",
    "RCS_RING_HEAD", "RCS_RING_TAIL", "DIRECT_RCS_CONTEXT_BYTES",
    "DIRECT_RCS_LRC_STATE_OFFSET_DWORDS", "SPIRIT_VFX_COMPLETION_TIMEOUT_MS",
    "SPIRIT_VFX_PRE_MARKER_SLOT", "SPIRIT_VFX_POST_MARKER_SLOT",
    "SPIRIT_VFX_PRE_MARKER", "SPIRIT_VFX_POST_MARKER",
    "SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_GPU", "SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES",
    "SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES", "SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES",
    "SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES",
    "DIRECT_RCS_BATCH_BYTES", "SPIRIT_VFX_CONTROL_DWORDS",
    "SPIRIT_VFX_BACKGROUND_CROSS_THREAD_BYTES", "SPIRIT_VFX_SPRITE_CROSS_THREAD_BYTES",
))
source += "\n".join(constant(SPIRIT, name) for name in (
    "SPIRIT_VFX_INITIAL_TRACE_FRAMES", "SPIRIT_VFX_PERIODIC_TRACE_FRAMES",
    "SPIRIT_VFX_ENCODING_LOG_LIMIT",
))
source += "\n" + item(RUNTIME, "exact_ppgtt_pte_offset")
source += "\n".join(item(SPIRIT, name) for name in (
    "SpiritVfxBuffer", "SpiritVfxSubmitted", "SpiritVfxSubmission", "SpiritVfxPending",
    "SpiritVfxCompletion",
    "SpiritVfxPpgttEvidence", "SpiritVfxEncodingEvidence", "spirit_vfx_owned_dwords",
    "spirit_vfx_capture_encoding_evidence", "spirit_vfx_log_encoding_evidence",
    "spirit_vfx_ppgtt_evidence", "spirit_vfx_log_mapping_evidence",
    "submit_spirit_vfx_frame", "poll_spirit_vfx_submission", "spirit_vfx_trace_frame",
))
source += "\n".join(line for line in (Path(__file__).resolve().parents[2] / SPIRIT).read_text().splitlines() if line.startswith((
    "static SPIRIT_VFX_PENDING:", "static SPIRIT_VFX_NEXT_TAG:",
    "static SPIRIT_VFX_ENCODING_LOGS:",
    "static SPIRIT_VFX_SAVED_CONTEXT_LOGS:",
)))
source += r'''
#[derive(Clone,Copy,Debug)] struct DirectRcsGpuVa {ring:u64,batch:u64,context:u64}
#[derive(Clone,Copy,Debug)] struct DirectRcsState {
    ppgtt_virt:*mut u8,ppgtt_phys:u64,batch_phys:u64,batch_virt:*mut u8,
    context_phys:u64,context_virt:*mut u8,gpu_va:DirectRcsGpuVa,
}
unsafe impl Send for DirectRcsState {}
unsafe impl Send for SpiritVfxBuffer {}
#[derive(Clone,Copy)] struct UploadedKernelArtifact {gpu:u64,phys:u64}
#[derive(Clone,Copy)] pub(crate) struct GpgpuRgba8Surface {gpu:u64,phys:u64,width:u32,height:u32}
#[derive(Clone,Copy)] pub(crate) struct SpiritVfxControl;
#[derive(Clone,Copy,Debug)] pub(crate) struct GpgpuRgba8ReleaseFence;
fn gpgpu_rgba8_release(_:GpgpuRgba8Surface)->GpgpuRgba8ReleaseFence {GpgpuRgba8ReleaseFence}
#[derive(Clone,Copy)] struct RetirementProof {marker_observed:bool,saved_head_bytes:u32,published_tail_bytes:u32}
impl RetirementProof {fn complete(self)->bool {self.marker_observed&&self.saved_head_bytes==self.published_tail_bytes}}
std::thread_local! {
    static PROOF:std::cell::Cell<(u32,u32)>=const {std::cell::Cell::new((0,32))};
    static POST:std::cell::Cell<u32>=const {std::cell::Cell::new(0)};
    static QUARANTINES:std::cell::Cell<u32>=const {std::cell::Cell::new(0)};
}
fn direct_rcs_read_result_slot(_:DirectRcsState,slot:usize)->u32 {
    if slot==SPIRIT_VFX_POST_MARKER_SLOT {POST.with(std::cell::Cell::get)}
    else {assert_eq!(slot,SPIRIT_VFX_PRE_MARKER_SLOT);SPIRIT_VFX_PRE_MARKER}
}
fn direct_rcs_elapsed_ms_since(_:u64)->u64 {SPIRIT_VFX_COMPLETION_TIMEOUT_MS+1}
fn execution_rcs_retirement_proof(_:DirectRcsState,marker_observed:bool)->RetirementProof {
    let (saved_head_bytes,published_tail_bytes)=PROOF.with(std::cell::Cell::get);
    RetirementProof{marker_observed,saved_head_bytes,published_tail_bytes}
}
fn quarantine_execution_rcs_context(_: &str) {QUARANTINES.with(|count|count.set(count.get()+1));}
fn complete_execution_rcs_submission() {}
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
    fn try_lock(&self)->Option<std::sync::MutexGuard<'_,T>> {self.0.try_lock().ok()}
}
static EXECUTION_RCS_SUBMIT_LOCK:Mutex<()>=Mutex::new(());
static EXECUTION_RCS_DETACHED_TAG:AtomicU64=AtomicU64::new(0);
std::thread_local! {
    static ADMISSION:std::cell::Cell<Option<(SpiritVfxSubmitted,Option<SpiritVfxEncodingEvidence>)>>=
        const {std::cell::Cell::new(None)};
}
fn submit_spirit_vfx_batch(_:GpgpuRgba8Surface,_:GpgpuRgba8Surface,_:SpiritVfxControl,_:u32)
    ->Option<(SpiritVfxSubmitted,Option<SpiritVfxEncodingEvidence>)> {
    ADMISSION.with(|admission|admission.take())
}

fn table()->(Vec<u64>,DirectRcsState) {
    let mut bytes=vec![0u64;DIRECT_RCS_PPGTT_BYTES/8];
    let state=DirectRcsState {ppgtt_virt:bytes.as_mut_ptr().cast(),ppgtt_phys:0x100000000,
        batch_phys:0x1f00000,batch_virt:core::ptr::null_mut(),context_phys:0x80002000,
        context_virt:core::ptr::null_mut(),
        gpu_va:DirectRcsGpuVa{ring:0x8000000,batch:0x8100000,context:0x8002000}};
    bytes[DIRECT_RCS_PPGTT_PML4_OFFSET/8]=(state.ppgtt_phys+DIRECT_RCS_PPGTT_PDPT_OFFSET as u64)|0x1b;
    for index in 0..DIRECT_RCS_PPGTT_PD_COUNT {
        bytes[DIRECT_RCS_PPGTT_PDPT_OFFSET/8+index]=
            (state.ppgtt_phys+DIRECT_RCS_PPGTT_PD_OFFSET as u64+index as u64*4096)|0x1b;
    }
    for index in 0..DIRECT_RCS_PPGTT_PT_COUNT {
        bytes[DIRECT_RCS_PPGTT_PD_OFFSET/8+index]=
            (state.ppgtt_phys+DIRECT_RCS_PPGTT_PT_OFFSET as u64+index as u64*4096)|0x1b;
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
    bytes[0]|=0x20;bytes[DIRECT_RCS_PPGTT_PDPT_OFFSET/8+(gpu>>30)as usize]|=0x20;
    bytes[DIRECT_RCS_PPGTT_PD_OFFSET/8+(gpu>>21)as usize]|=0x20;
    let result=spirit_vfx_ppgtt_evidence(state,gpu,phys,0x1b).unwrap();
    assert_eq!(result.read_levels,4);
    assert!(result.topology_match&&result.leaf_match);
    assert_eq!(result.entries[3],(phys&!0xfff)|0x7b,"retain accessed/dirty bits");
}

#[test] fn corrupt_parent_stops_before_any_unowned_or_lower_level_read() {
    let (mut bytes,state)=table();let gpu=0x490a0000;
    let offsets=[0,DIRECT_RCS_PPGTT_PDPT_OFFSET/8+(gpu>>30)as usize,
        DIRECT_RCS_PPGTT_PD_OFFSET/8+(gpu>>21)as usize];
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
    let dst=GpgpuRgba8Surface{gpu:0x6000000,phys:0x184d7000,width:256,height:256};
    for (gpu,phys,flags) in [(kernel.gpu,kernel.phys,3),(control.gpu,control.phys,3),
        (dst.gpu,dst.phys,0x1b),(state.gpu_va.batch+0x6000,state.batch_phys+0x6000,3)] {
        map(&mut bytes,gpu,phys,flags);
    }
    let submitted=SpiritVfxSubmitted{state,first_kernel:kernel,control,source:dst,dst,
        submit_ambiguous:false,frame:0,revision:0,background_mode:11,shader_mode:1,started_tick:0,
        encoding_evidence_logged:false};
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

fn encoding_mmio() {
    crate::READS.with(|reads|*reads.borrow_mut()=VecDeque::from([
        (0x2038,0x8000000),(0x2274,1),(0x2270,0),(0x2274,1),
        (0x20e4,4),(0x20e0,0x4000),(0xb134,0xb0000040),
        (0x4010,0x40404040),(0xb028,0x28282828),(0x20d8,0xd8d8d8d8),
        (0x2038,0x8000000)]));
}

#[test] fn owned_word_reader_checks_whole_range_alignment_and_address_overflow() {
    crate::FLUSHES.with(|flushes|flushes.borrow_mut().clear());
    let mut words=[0x11111111u32,0x22222222,0x33333333,0x44444444];
    let base=words.as_mut_ptr().cast();
    assert_eq!(spirit_vfx_owned_dwords::<2>(base,16,8),Some([0x33333333,0x44444444]));
    for (virt,bytes,offset) in [(base,15,8),(base,16,12),(base,16,1),
        (base,16,usize::MAX),(core::ptr::null_mut(),16,0),
        ((usize::MAX&!3)as *mut u8,8,0),(base,usize::MAX,0)] {
        assert!(spirit_vfx_owned_dwords::<2>(virt,bytes,offset).is_none());
    }
    crate::FLUSHES.with(|flushes|assert_eq!(*flushes.borrow(),vec![(base as usize+8,8)],
        "invalid ranges must never be flushed or read"));
    assert_eq!(words,[0x11111111,0x22222222,0x33333333,0x44444444]);
}

#[test] fn snapshots_copy_only_fixed_owned_regions_and_observe_later_mutation() {
    let (_table,mut state)=table();
    let mut batch:Vec<u32>=(0..DIRECT_RCS_BATCH_BYTES/4).map(|word|word as u32).collect();
    state.batch_virt=batch.as_mut_ptr().cast();
    let mut control_words:[u32;SPIRIT_VFX_CONTROL_DWORDS]=core::array::from_fn(|i|0x1000+i as u32);
    let control=SpiritVfxBuffer{phys:0x18613000,gpu:0x90a0000,
        virt:control_words.as_mut_ptr().cast(),bytes:control_words.len()*4};
    // These are actual payload words, not permission to follow a pointer.
    batch[SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES/4+12]=u32::MAX;
    batch[SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES/4+13]=u32::MAX;
    encoding_mmio();
    let before=spirit_vfx_capture_encoding_evidence(crate::Dev,state,control,true).unwrap();
    crate::READS.with(|reads|assert!(reads.borrow().is_empty()));
    assert_eq!(before.background_idd.unwrap(),core::array::from_fn(|i|(SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES/4+i)as u32));
    assert_eq!(before.background_cross_thread.unwrap()[12..14],[u32::MAX;2]);
    assert_eq!(before.background_local_ids.unwrap()[0],((SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES+64)/4)as u32);
    assert_eq!(before.sprite_idd.unwrap()[7],(SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES/4+7)as u32);
    assert_eq!(before.sprite_cross_thread.unwrap()[23],(SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES/4+23)as u32);
    assert_eq!(before.sprite_local_ids.unwrap()[23],((SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES+96)/4+23)as u32);
    assert_eq!(before.control.unwrap()[32],0x1020,"include the final clock field");
    assert_eq!(before.global_mocs4_4010,0x40404040);
    assert_eq!(before.l3cc_pair_b028,0x28282828);
    assert_eq!(before.cs_debug_mode2_20d8,0xd8d8d8d8);
    batch[SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES/4]=0xdeadbeef;
    unsafe {core::ptr::write_volatile(control_words.as_mut_ptr().add(32),0xabcdef01);}
    encoding_mmio();
    let after=spirit_vfx_capture_encoding_evidence(crate::Dev,state,control,true).unwrap();
    assert_ne!(before.background_idd,after.background_idd);
    assert_ne!(before.control,after.control);
    assert_eq!(before.control.unwrap()[32],0x1020,"earlier capture must be retained by value");
    assert_eq!(after.control.unwrap()[32],0xabcdef01);
    encoding_mmio();
    let sprite_only=spirit_vfx_capture_encoding_evidence(crate::Dev,state,control,false).unwrap();
    assert!(sprite_only.background_idd.is_none()&&sprite_only.background_cross_thread.is_none()
        &&sprite_only.background_local_ids.is_none());
    assert!(sprite_only.sprite_idd.is_some()&&sprite_only.control.is_some());
    assert_eq!(batch[SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES/4],0xdeadbeef);
}

#[test] fn desktop_skips_extra_encoding_mmio_and_owned_memory_reads() {
    let (_table,state)=table();
    let invalid=SpiritVfxBuffer{phys:0,gpu:0,virt:1usize as *mut u8,bytes:4096};
    crate::intel::render::TGL.with(|tgl|tgl.set(false));
    crate::READS.with(|reads|reads.borrow_mut().clear());
    crate::FLUSHES.with(|flushes|flushes.borrow_mut().clear());
    assert!(spirit_vfx_capture_encoding_evidence(crate::Dev,state,invalid,true).is_none());
    crate::READS.with(|reads|assert!(reads.borrow().is_empty()));
    crate::FLUSHES.with(|flushes|assert!(flushes.borrow().is_empty()));
    crate::intel::render::TGL.with(|tgl|tgl.set(true));
}

#[test] fn first_two_actual_admissions_keep_timeout_flags_and_rejections_do_not_consume_slots() {
    SPIRIT_VFX_ENCODING_LOGS.store(0,Ordering::Release);
    EXECUTION_RCS_DETACHED_TAG.store(0,Ordering::Release);
    *SPIRIT_VFX_PENDING.lock()=None;
    let (_table,state)=table();
    let surface=GpgpuRgba8Surface{gpu:0x6000000,phys:0x184d7000,width:256,height:256};
    assert!(submit_spirit_vfx_frame(surface,surface,SpiritVfxControl,60).is_none());
    assert_eq!(SPIRIT_VFX_ENCODING_LOGS.load(Ordering::Acquire),0);
    let control=SpiritVfxBuffer{phys:0x18613000,gpu:0x90a0000,virt:core::ptr::null_mut(),bytes:4096};
    encoding_mmio();
    let capture=spirit_vfx_capture_encoding_evidence(crate::Dev,state,control,true).unwrap();
    let submitted=SpiritVfxSubmitted{state,first_kernel:UploadedKernelArtifact{gpu:0xd430000,phys:0x18557000},
        control,source:surface,dst:surface,submit_ambiguous:true,frame:7,revision:19,
        background_mode:11,shader_mode:1,started_tick:0,encoding_evidence_logged:false};
    ADMISSION.with(|admission|admission.set(Some((submitted,Some(capture)))));
    crate::LOGS.with(|logs|logs.borrow_mut().clear());
    let handle=submit_spirit_vfx_frame(surface,surface,SpiritVfxControl,60).unwrap();
    assert_eq!(SPIRIT_VFX_ENCODING_LOGS.load(Ordering::Acquire),1);
    assert!(SPIRIT_VFX_PENDING.lock().unwrap().submitted.encoding_evidence_logged);
    crate::LOGS.with(|logs| {
        let logs=logs.borrow();assert_eq!(logs.len(),10,"nine evidence lines and accepted record");
        assert!(logs[0].contains(&format!("tag={} frame=7 phase=before-commit background_present=true",handle.tag)));
        assert!(logs[0].contains("root=0x100000000 context_phys=0x80002000 context_gpu=0x8002000"));
        assert!(logs[1].contains("root_matches=Some(true)"));
        assert!(logs[1].contains("global_mocs4_4010=0x40404040 l3cc_pair_b028=0x28282828 cs_debug_mode2_20d8=0xD8D8D8D8"));
        assert!(logs[1].contains("capture=observational"));
        assert!(logs[8].contains("resource=control offset=0x0 dwords=None"));
    });
    // Pending work and a busy submission lock cannot consume the second slot.
    assert!(submit_spirit_vfx_frame(surface,surface,SpiritVfxControl,60).is_none());
    assert_eq!(SPIRIT_VFX_ENCODING_LOGS.load(Ordering::Acquire),1);
    POST.with(|marker|marker.set(SPIRIT_VFX_POST_MARKER));
    PROOF.with(|proof|proof.set((16,16)));
    assert!(matches!(poll_spirit_vfx_submission(handle),SpiritVfxCompletion::Complete(_)));
    assert_eq!(EXECUTION_RCS_DETACHED_TAG.load(Ordering::Acquire),0);
    assert!(SPIRIT_VFX_PENDING.lock().is_none());
    {
        let _busy=EXECUTION_RCS_SUBMIT_LOCK.lock();
        assert!(submit_spirit_vfx_frame(surface,surface,SpiritVfxControl,60).is_none());
        assert_eq!(SPIRIT_VFX_ENCODING_LOGS.load(Ordering::Acquire),1);
    }
    // An encoder/submission rejection after the first retirement also leaves
    // the second slot available. The next unambiguous admission is captured.
    assert!(submit_spirit_vfx_frame(surface,surface,SpiritVfxControl,60).is_none());
    assert_eq!(SPIRIT_VFX_ENCODING_LOGS.load(Ordering::Acquire),1);
    let mut second=submitted;second.submit_ambiguous=false;second.frame=8;
    ADMISSION.with(|admission|admission.set(Some((second,Some(capture)))));
    crate::LOGS.with(|logs|logs.borrow_mut().clear());
    let second_handle=submit_spirit_vfx_frame(surface,surface,SpiritVfxControl,60).unwrap();
    assert_eq!(SPIRIT_VFX_ENCODING_LOGS.load(Ordering::Acquire),2);
    assert!(SPIRIT_VFX_PENDING.lock().unwrap().submitted.encoding_evidence_logged,
        "second job must retain its own timeout-recapture flag");
    crate::LOGS.with(|logs| {
        let logs=logs.borrow();assert_eq!(logs.len(),10);
        assert!(logs[0].contains(&format!("tag={} frame=8 phase=before-commit",second_handle.tag)));
    });
    // The actual poller recaptures the second job once at its first timeout.
    // Later polls retain the pending job without more reads or log growth.
    POST.with(|marker|marker.set(0));PROOF.with(|proof|proof.set((28,32)));
    QUARANTINES.with(|count|count.set(0));
    encoding_mmio();
    crate::READS.with(|reads| {
        let mut reads=reads.borrow_mut();
        for observation in [(0x20e0,0x4000),(RCS_RING_TAIL,32),(RCS_RING_HEAD,28),
            (RCS_RING_START,0x8000000),(RCS_RING_BASE+0x580,0x400)] {
            reads.push_front(observation);
        }
        reads.extend([(0x2038,0x8000000),(0x2274,1),(0x2270,0),(0x2274,1),
            (0x20e4,4),(0xb134,0xb0000040),(0x2038,0x8000000)]);
    });
    crate::LOGS.with(|logs|logs.borrow_mut().clear());
    assert!(matches!(poll_spirit_vfx_submission(second_handle),SpiritVfxCompletion::Pending));
    crate::READS.with(|reads|assert!(reads.borrow().is_empty()));
    crate::LOGS.with(|logs| {
        let logs=logs.borrow();assert_eq!(logs.len(),17);
        assert_eq!(logs.iter().filter(|line|line.contains("phase=first-timeout")).count(),9);
        assert!(logs[1].contains(&format!("tag={} frame=8 phase=first-timeout",second_handle.tag)));
    });
    assert!(SPIRIT_VFX_PENDING.lock().unwrap().timeout_logged);
    assert_eq!(EXECUTION_RCS_DETACHED_TAG.load(Ordering::Acquire),second_handle.tag);
    assert!(matches!(poll_spirit_vfx_submission(second_handle),SpiritVfxCompletion::Pending));
    crate::LOGS.with(|logs|assert_eq!(logs.borrow().len(),17));
    QUARANTINES.with(|count|assert_eq!(count.get(),1));
    for _ in 0..3 {
        EXECUTION_RCS_DETACHED_TAG.store(0,Ordering::Release);
        *SPIRIT_VFX_PENDING.lock()=None;
        ADMISSION.with(|admission|admission.set(Some((submitted,Some(capture)))));
        crate::LOGS.with(|logs|logs.borrow_mut().clear());
        submit_spirit_vfx_frame(surface,surface,SpiritVfxControl,60).unwrap();
        assert_eq!(SPIRIT_VFX_ENCODING_LOGS.load(Ordering::Acquire),2);
        assert!(!SPIRIT_VFX_PENDING.lock().unwrap().submitted.encoding_evidence_logged);
        crate::LOGS.with(|logs|assert_eq!(logs.borrow().len(),1,"later jobs have no encoding dump"));
    }
    EXECUTION_RCS_DETACHED_TAG.store(0,Ordering::Release);
    *SPIRIT_VFX_PENDING.lock()=None;
}
}
'''

with tempfile.TemporaryDirectory(prefix="trueos-spirit-ppgtt-evidence-") as temporary:
    src, executable = Path(temporary) / "tests.rs", Path(temporary) / "tests"
    src.write_text(source)
    subprocess.run(["rustc", "--edition=2024", "--test", str(src), "-o", str(executable)], check=True)
    subprocess.run([str(executable)], check=True)
