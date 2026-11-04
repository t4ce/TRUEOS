#!/usr/bin/env python3
"""Exercise actual native Edit dispatch/prologue code without GPU hardware."""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def block(source: str, marker: str) -> str:
    start = source.index(marker)
    opening = source.index("{", start)
    depth = 1
    end = opening + 1
    while depth:
        if source[end] == "{":
            depth += 1
        elif source[end] == "}":
            depth -= 1
        end += 1
    return source[start:end]


def function(source: str, name: str) -> str:
    return block(source, "fn " + name + "(")


def constant_closure(source: str, users: str) -> str:
    declarations = dict(re.findall(r"(?m)^const ([A-Z][A-Z_0-9]*):([^;]+;)", source))
    pending = set(re.findall(r"\b[A-Z][A-Z_0-9]*\b", users)) & declarations.keys()
    found = set()
    while pending:
        name = pending.pop()
        found.add(name)
        pending |= (set(re.findall(r"\b[A-Z][A-Z_0-9]*\b", declarations[name]))
                    & declarations.keys()) - found
    return "\n".join("const " + name + ":" + declarations[name] for name in sorted(found))


def main() -> None:
    native = (ROOT / "src/intel/gpgpu/operations/edit_life.rs").read_text()
    commands = (ROOT / "src/intel/gpgpu/rcs/commands.rs").read_text()
    worklists = (ROOT / "src/intel/gpgpu/rcs/worklists.rs").read_text()
    constants = (ROOT / "src/intel/gpgpu/rcs/constants.rs").read_text()
    dispatch = function(native, "dispatch")
    rows = function(native, "edit_life_dispatch_rows")
    prologue = function(worklists, "direct_rcs_push_gpgpu_prologue_with_instruction_base")
    packet_helpers = "\n".join(function(commands, name) for name in (
        "direct_rcs_push", "direct_rcs_push_pipe_control_full", "direct_rcs_push_pipe_control"))
    arithmetic = "\n".join(function(native, name) for name in (
        "edit_life_reciprocal", "edit_life_geometry_uniforms", "edit_life_prepare_ppgtt", "edit_life_phase_us"))
    timing = "#[derive(Default)]\n" + block(native, "struct EditLifePassTiming")
    native_constants = constant_closure(native, dispatch + rows + arithmetic
        + "EDIT_LIFE_MAX_PAGE_PIXELS EDIT_LIFE_BATCH_USED_BYTES EDIT_LIFE_RESULT_USED_BYTES")
    packet_constants = constant_closure(constants, prologue + packet_helpers)
    encoder = function(native, "encode_edit_life_batch")
    assert "direct_rcs_push_gpgpu_prologue_with_instruction_base" in encoder
    assert "direct_rcs_push_pipe_control_full" not in encoder, "Edit must use the supported shared prologue"
    assert "DIRECT_RCS_GPU_VA_BATCH_BASE" not in encoder, "Editor payload must use its own lane"
    assert "let uniforms_gpu = state.gpu_va.batch" in encoder
    assert re.search(r"direct_rcs_push_gpgpu_prologue_with_instruction_base\(batch, &mut cursor,\s*"
                     r"state.gpu_va.batch, upload.gpu, GPGPU_VFE_DW5_UOS\)", encoder)
    assert "core::ptr::write_volatile(payload.add(1), first_row)" in encoder
    assert "direct_rcs_init_ppgtt" not in dispatch, "Chunking must not rebuild page tables"
    assert "EDIT_LIFE_RCS_SUBMIT_LOCK.lock()" in native
    assert "allocation.owner = Rgba8SurfaceOwner::EditLife" in native
    assert "direct_rcs_init_ppgtt" not in function(native, "render")
    assert "core::ptr::write_bytes(state.batch_virt, 0, EDIT_LIFE_BATCH_USED_BYTES)" in encoder
    assert "super::dma_flush(state.batch_virt, EDIT_LIFE_BATCH_USED_BYTES)" in encoder
    assert "super::dma_flush(state.result_virt, EDIT_LIFE_RESULT_USED_BYTES)" in encoder
    assert "buffers[7] = (uniforms_gpu, EDIT_LIFE_UNIFORM_BYTES)" in encoder

    harness = r'''
#![allow(dead_code, unused_variables)]
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
#[macro_export] macro_rules! log_error { ($($token:tt)*) => {}; }
#[derive(Clone, Copy)] struct Dev;
mod native {
use super::*;
#[derive(Clone, Copy)] struct DirectRcsState { ppgtt_phys: u64 }
const STATE: DirectRcsState = DirectRcsState { ppgtt_phys: 0x1000 };
#[derive(Clone, Copy)] struct GpgpuRect { width: u32, height: u32 }
static EDIT_LIFE_PPGTT_ROOT: AtomicU64 = AtomicU64::new(0);
static EDIT_LIFE_PPGTT_ATTEMPTED: AtomicBool = AtomicBool::new(false);
static INITIALIZATIONS: AtomicUsize = AtomicUsize::new(0);
static INITIALIZE_OK: AtomicBool = AtomicBool::new(true);
static GPU_CLOCK: AtomicU64 = AtomicU64::new(100);
fn direct_rcs_init_ppgtt(_: DirectRcsState) -> bool {
    INITIALIZATIONS.fetch_add(1, Ordering::Relaxed);
    INITIALIZE_OK.load(Ordering::Relaxed)
}
fn direct_rcs_now_tick() -> u64 { 1 }
fn direct_rcs_elapsed_us_since(_: u64) -> u64 { 2 }
fn direct_rcs_read_render_timestamp(_: Dev) -> u64 { GPU_CLOCK.fetch_add(60, Ordering::Relaxed) }
fn direct_rcs_timestamp_frequency_hz(_: Dev) -> u32 { 1_000_000 }
fn direct_rcs_timestamp_interval_us(start: u64, end: u64, hz: u64) -> Option<(u64,u64)> {
    (start != 0 && end > start && hz != 0).then_some((end-start,end-start))
}
fn direct_rcs_read_result_qword(_: DirectRcsState, slot: usize) -> u64 {
    let start = TRACE.lock().unwrap().submit_stamp;
    start + match slot { 40 => 10, 42 => 20, 44 => 30, 46 => 40, _ => panic!("wrong timestamp slot") }
}
#[derive(Clone, Copy)] struct UploadedKernelArtifact;
#[derive(Clone, Copy)] struct GpgpuRgba8Surface;
#[derive(Default)] struct DirectRcsDispatchOutcome { submitted: bool, observed: u32 }
#[derive(Debug, PartialEq)] enum DirectRcsLane { EditLife }
enum DirectRcsSubmissionState { Submitted, Rejected }
impl DirectRcsSubmissionState {
    fn may_have_submitted(&self) -> bool { matches!(self, Self::Submitted) }
    fn can_poll(&self) -> bool { matches!(self, Self::Submitted) }
}
const UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS: u64 = 1_000;
struct EditLifeState { grid_width: u32, grid_height: u32, width: u32, height: u32, columns: u32, rows: u32 }
#[derive(Default)] struct Trace { chunks: Vec<(u32, u32, u32)>, inflight: bool, polls: usize, fail_poll: usize, quarantines: usize, submit_stamp: u64 }
static TRACE: Mutex<Trace> = Mutex::new(Trace { chunks: Vec::new(), inflight: false, polls: 0, fail_poll: 0, quarantines: 0, submit_stamp: 0 });
fn encode_edit_life_batch(state: DirectRcsState, upload: UploadedKernelArtifact, life: &EditLifeState,
    dst: GpgpuRgba8Surface, source: GpgpuRgba8Surface, mode: u32, reset: bool,
    width: u32, first_row: u32, rows: u32, probe: bool) -> bool {
    let mut trace = TRACE.lock().unwrap();
    assert!(!trace.inflight, "batch cannot be rewritten before the preceding chunk retires");
    trace.chunks.push((width, first_row, rows));
    true
}
fn direct_rcs_submit_batch_on_lane_state(dev: Dev, state: DirectRcsState, lane: DirectRcsLane) -> DirectRcsSubmissionState {
    assert_eq!(lane, DirectRcsLane::EditLife);
    let mut trace = TRACE.lock().unwrap();
    assert!(!trace.inflight);
    trace.inflight = true;
    trace.submit_stamp = GPU_CLOCK.load(Ordering::Relaxed)-60;
    DirectRcsSubmissionState::Submitted
}
fn direct_rcs_poll_result_slot_timeout_ms_on_lane(state: DirectRcsState, slot: usize, marker: u32,
    timeout: u64, lane: DirectRcsLane) -> u32 {
    assert_eq!(lane, DirectRcsLane::EditLife);
    assert_eq!(slot, EDIT_LIFE_POST_SLOT);
    assert_eq!(marker, EDIT_LIFE_POST_MARKER);
    let mut trace = TRACE.lock().unwrap();
    assert!(trace.inflight);
    trace.polls += 1;
    if trace.polls == trace.fail_poll { return 0; }
    trace.inflight = false;
    marker
}
fn direct_rcs_poll_result_slot_timeout_ms_on_lane_with_timestamp(state: DirectRcsState,
    slot: usize, marker: u32, timeout: u64, lane: DirectRcsLane, dev: Option<Dev>) -> (u32,u64) {
    let observed = direct_rcs_poll_result_slot_timeout_ms_on_lane(state,slot,marker,timeout,lane);
    (observed,TRACE.lock().unwrap().submit_stamp+50)
}
fn quarantine_direct_rcs_lane(lane: DirectRcsLane, reason: &'static str) {
    assert_eq!(lane, DirectRcsLane::EditLife);
    TRACE.lock().unwrap().quarantines += 1;
}
''' + native_constants + "\n" + arithmetic + "\n" + timing + "\n" + rows + "\nimpl EditLifeState {\n" + dispatch + r'''
}
pub(super) fn run() {
    let mut cases = 0;
    for mode in 0..4 {
        for width in [1u32, 15, 16, 17, 79, 511, 512, 513, 1919, 1920, 3840, 4096] {
            for height in [1u32, 2, 31, 512, 1080, 2160] {
                *TRACE.lock().unwrap() = Trace::default();
                let life = EditLifeState { grid_width: width, grid_height: height, width, height, columns: width, rows: height };
                let mut timing = EditLifePassTiming::default();
                let result = life.dispatch(Dev, STATE, UploadedKernelArtifact, GpgpuRgba8Surface, GpgpuRgba8Surface, mode, true, Some(&mut timing));
                assert!(result.submitted);
                assert_eq!(result.observed, EDIT_LIFE_POST_MARKER);
                let trace = TRACE.lock().unwrap();
                assert!(!trace.inflight);
                assert_eq!(trace.polls, trace.chunks.len());
                assert_eq!(trace.quarantines, 0);
                assert_eq!(timing.chunks as usize,trace.chunks.len());
                assert_eq!(timing.samples as usize,trace.chunks.len());
                assert_eq!(timing.walker_us,trace.chunks.len() as u64*10);
                assert_eq!(timing.queue_us,timing.walker_us);
                assert_eq!(timing.prologue_us,timing.walker_us);
                assert_eq!(timing.release_us,timing.walker_us);
                assert_eq!(timing.marker_us,timing.walker_us);
                assert_eq!(timing.save_us,timing.walker_us);
                let maximum = match mode { 0 => EDIT_LIFE_SEED_MAX_LANES, 2 => EDIT_LIFE_COMPOSITE_MAX_LANES, _ => EDIT_LIFE_STEP_MAX_LANES };
                let mut next_row = 0;
                for &(chunk_width, first_row, rows) in &trace.chunks {
                    assert_eq!(chunk_width, width);
                    assert_eq!(first_row, next_row, "chunks must neither skip nor repeat rows");
                    assert!(rows > 0);
                    assert!(u64::from(width).div_ceil(16) * 16 * u64::from(rows) <= maximum,
                        "bound counts padded hardware lanes, not just visible pixels");
                    next_row += rows;
                }
                assert_eq!(next_row, height);
                cases += 1;
            }
        }
    }
    assert_eq!(edit_life_dispatch_rows(0, 0, 1, 0), None);
    assert_eq!(edit_life_dispatch_rows(0, 1, 1, 1), None);
    assert_eq!(edit_life_dispatch_rows(4, 1, 1, 0), None);
    assert_eq!(edit_life_dispatch_rows(0, 16_385, 1, 0), None);
    *TRACE.lock().unwrap() = Trace { fail_poll: 2, ..Trace::default() };
    let life = EditLifeState { grid_width: 512, grid_height: 512, width: 1, height: 1, columns: 1, rows: 1 };
    let failed = life.dispatch(Dev, STATE, UploadedKernelArtifact, GpgpuRgba8Surface, GpgpuRgba8Surface, 0, true, None);
    assert!(failed.submitted);
    assert_eq!(failed.observed, 0);
    let trace = TRACE.lock().unwrap();
    assert_eq!(trace.chunks.len(), 2, "failure must stop before another chunk or buffer flip");
    assert_eq!(trace.polls, 2);
    assert_eq!(trace.quarantines, 1);
    drop(trace);
    assert!(edit_life_prepare_ppgtt(STATE));
    assert!(edit_life_prepare_ppgtt(STATE));
    assert_eq!(INITIALIZATIONS.load(Ordering::Relaxed),1,"same private PPGTT must only initialize once");
    assert!(!edit_life_prepare_ppgtt(DirectRcsState{ppgtt_phys:0x2000}));
    assert_eq!(INITIALIZATIONS.load(Ordering::Relaxed),1,"changed root must fail closed, not rebuild");
    EDIT_LIFE_PPGTT_ROOT.store(0,Ordering::Relaxed);
    EDIT_LIFE_PPGTT_ATTEMPTED.store(false,Ordering::Relaxed);
    INITIALIZATIONS.store(0,Ordering::Relaxed);
    INITIALIZE_OK.store(false,Ordering::Relaxed);
    assert!(!edit_life_prepare_ppgtt(STATE));
    assert!(!edit_life_prepare_ppgtt(STATE));
    assert_eq!(INITIALIZATIONS.load(Ordering::Relaxed),1,"failed initialization must not repair itself");
    assert_eq!(EDIT_LIFE_BATCH_USED_BYTES,7296);
    assert_eq!(EDIT_LIFE_RESULT_USED_BYTES,256);
    assert_eq!(edit_life_reciprocal(0),None);
    assert_eq!(edit_life_reciprocal(257),None);
    for d in 1..=256u32 {
        let magic=edit_life_reciprocal(d).unwrap();
        for quotient in [0,1,2,255,65535,EDIT_LIFE_MAX_PAGE_PIXELS as u32/d] {
            for offset in [-1i64,0,1,i64::from(d)-1] {
                let n=i64::from(quotient)*i64::from(d)+offset;
                if !(0..=EDIT_LIFE_MAX_PAGE_PIXELS as i64).contains(&n) { continue; }
                let n=n as u32;
                let actual=if d==1 {n} else {((u64::from(n)*u64::from(magic))>>32) as u32};
                assert_eq!(actual,n/d,"exact reciprocal d={d} n={n}");
            }
        }
    }
    for (width,height) in [(1u32,1u32),(17,31),(1023,511),(1920,1080),(4096,2160)] {
        let gw=width.div_ceil(3).min(512); let gh=height.div_ceil(3).min(512);
        let table=edit_life_geometry_uniforms(GpgpuRect{width,height},gw,gh,17,31).unwrap();
        assert_eq!(table.len(),1044);
        assert_eq!(table[16],edit_life_reciprocal(17).unwrap());
        assert_eq!(table[17],edit_life_reciprocal(31).unwrap());
        assert_eq!(u128::from(table[18]),u128::from(width)*87/u128::from(gw));
        assert_eq!(u128::from(table[19]),u128::from(height)*87/u128::from(gh));
        for (origin,count,extent) in [(20,gw,width),(532,gh,height)] {
            for at in 0..count {
                let original=(((u128::from(at)*2+1)*u128::from(extent)*128/u128::from(count)) as i64)-128;
                assert_eq!(i64::from(table[origin+at as usize] as i32),original);
            }
        }
    }
    println!("native EditLife dispatch passed: {cases} padded partitions, exact lane/retirement/timestamp phases, retained PPGTT, footprint and geometry uniforms");
}
}
mod packets {
''' + packet_constants + "\n" + packet_helpers + "\n" + prologue + r'''
fn direct_rcs_push_state_base_address(batch: &mut [u32], cursor: &mut usize,
    surface: u64, dynamic: u64, instruction: u64) -> bool {
    assert_eq!(surface, 0x0890_0000);
    assert_eq!(dynamic, surface);
    assert_eq!(instruction, 0x0D90_0000);
    // The separately validated state-base encoder is outside this packet test.
    true
}
pub(super) fn run() {
    let mut batch = [0u32; 128];
    let mut cursor = 0;
    assert!(direct_rcs_push_gpgpu_prologue_with_instruction_base(&mut batch, &mut cursor,
        0x0890_0000, 0x0D90_0000, 0x0782_0000));
    let mut at = 0;
    let mut controls = 0;
    while at < cursor {
        if batch[at] & 0xffff_0000 == PIPE_CONTROL_CMD & 0xffff_0000 {
            assert_eq!(batch[at] & (1 << 11), 0, "reserved Gen12 PIPE_CONTROL DW0 bit11");
            controls += 1;
            at += 6;
        } else if batch[at] == MEDIA_VFE_STATE_CMD { at += 9; }
        else { at += 1; }
    }
    assert_eq!(controls, 5);
    println!("native EditLife shared prologue passed: five supported Gen12 PIPE_CONTROL packets");
}
}
fn main() { native::run(); packets::run(); }
'''
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-native-") as temporary:
        source = Path(temporary) / "native.rs"
        source.write_text(harness)
        binary = Path(temporary) / "native"
        subprocess.run(["rustc", "--edition=2024", str(source), "-o", str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
