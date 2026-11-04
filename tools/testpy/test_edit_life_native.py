#!/usr/bin/env python3
"""Exercise actual native Edit dispatch/prologue code without GPU hardware."""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def function(source: str, name: str) -> str:
    start = source.index("fn " + name + "(")
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
    native_constants = constant_closure(native, dispatch + rows)
    packet_constants = constant_closure(constants, prologue + packet_helpers)
    encoder = function(native, "encode_edit_life_batch")
    assert "direct_rcs_push_gpgpu_prologue_with_instruction_base" in encoder
    assert "direct_rcs_push_pipe_control" not in encoder, "Edit must use the supported shared prologue"
    assert "DIRECT_RCS_GPU_VA_BATCH_BASE" not in encoder, "Editor payload must use its own lane"
    assert "let uniforms_gpu = state.gpu_va.batch" in encoder
    assert re.search(r"direct_rcs_push_gpgpu_prologue_with_instruction_base\(batch, &mut cursor,\s*"
                     r"state.gpu_va.batch, upload.gpu, GPGPU_VFE_DW5_UOS\)", encoder)
    assert "core::ptr::write_volatile(payload.add(1), first_row)" in encoder
    assert "direct_rcs_init_ppgtt" not in dispatch, "Chunking must not rebuild page tables"
    assert "EDIT_LIFE_RCS_SUBMIT_LOCK.lock()" in native
    assert "allocation.owner = Rgba8SurfaceOwner::EditLife" in native

    harness = r'''
#![allow(dead_code, unused_variables)]
use std::sync::Mutex;
#[macro_export] macro_rules! log_error { ($($token:tt)*) => {}; }
#[derive(Clone, Copy)] struct Dev;
mod native {
use super::*;
#[derive(Clone, Copy)] struct DirectRcsState;
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
#[derive(Default)] struct Trace { chunks: Vec<(u32, u32, u32)>, inflight: bool, polls: usize, fail_poll: usize, quarantines: usize }
static TRACE: Mutex<Trace> = Mutex::new(Trace { chunks: Vec::new(), inflight: false, polls: 0, fail_poll: 0, quarantines: 0 });
fn encode_edit_life_batch(state: DirectRcsState, upload: UploadedKernelArtifact, life: &EditLifeState,
    dst: GpgpuRgba8Surface, source: GpgpuRgba8Surface, mode: u32, reset: bool,
    width: u32, first_row: u32, rows: u32) -> bool {
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
fn quarantine_direct_rcs_lane(lane: DirectRcsLane, reason: &'static str) {
    assert_eq!(lane, DirectRcsLane::EditLife);
    TRACE.lock().unwrap().quarantines += 1;
}
''' + native_constants + "\n" + rows + "\nimpl EditLifeState {\n" + dispatch + r'''
}
pub(super) fn run() {
    let mut cases = 0;
    for mode in 0..4 {
        for width in [1u32, 15, 16, 17, 79, 511, 512, 513, 1919, 1920, 3840, 4096] {
            for height in [1u32, 2, 31, 512, 1080, 2160] {
                *TRACE.lock().unwrap() = Trace::default();
                let life = EditLifeState { grid_width: width, grid_height: height, width, height, columns: width, rows: height };
                let result = life.dispatch(Dev, DirectRcsState, UploadedKernelArtifact, GpgpuRgba8Surface, GpgpuRgba8Surface, mode, true);
                assert!(result.submitted);
                assert_eq!(result.observed, EDIT_LIFE_POST_MARKER);
                let trace = TRACE.lock().unwrap();
                assert!(!trace.inflight);
                assert_eq!(trace.polls, trace.chunks.len());
                assert_eq!(trace.quarantines, 0);
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
    let failed = life.dispatch(Dev, DirectRcsState, UploadedKernelArtifact, GpgpuRgba8Surface, GpgpuRgba8Surface, 0, true);
    assert!(failed.submitted);
    assert_eq!(failed.observed, 0);
    let trace = TRACE.lock().unwrap();
    assert_eq!(trace.chunks.len(), 2, "failure must stop before another chunk or buffer flip");
    assert_eq!(trace.polls, 2);
    assert_eq!(trace.quarantines, 1);
    println!("native EditLife dispatch passed: {cases} padded row partitions, exact lane and retirement gating");
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
