#!/usr/bin/env python3
"""Exercise production text-paint planning and async ownership on the host.

Use the actual BcsPlan, validators and text_paint. Only hardware submission,
CPU pixel access and timers are replaced, making ordering and cancellation
observable without constructing a second text-paint implementation.
"""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import ROOT, constant, item


def harness_source() -> str:
    blt = "src/intel/copy/blt.rs"
    constants = "\n".join(constant(blt, name) for name in (
        "DIRECT_BLT_PPGTT_PT_COUNT", "DIRECT_BLT_PPGTT_LIMIT_BYTES",
        "BCS0_GGTT_BASE", "BCS0_GGTT_LIMIT",
    ))
    declarations = "\n".join(item(blt, name) for name in (
        "GucBcs0RgbaSurface", "GucBcs0RgbaCopy", "GucBcs0RgbaFill",
        "GucBcs0CopySubmission", "GucBcs0CopySubmitError", "GucBcs0CopyCompletion",
        "guc_blt_valid_surface", "guc_blt_valid_copy", "guc_blt_valid_legacy_copy",
        "guc_blt_valid_fill", "guc_blt_valid_fill_rect",
        "guc_blt_source_mappings_compatible", "guc_blt_gpu_ranges_overlap",
        "guc_blt_physical_ranges_overlap",
    ))
    cpu_policy = "\n".join(item("src/ui4/text_blit.rs", name) for name in (
        "copy_pixels", "fill_pixels", "fill_on_cpu", "glyph_copy_on_cpu",
    ))
    planner = (ROOT / "src/intel/copy/bcs_plan.rs").read_text()
    paint = (ROOT / "src/ui4/text_paint.rs").read_text()
    # Inline module contents so the harness can retain the production tests.
    planner = "\n".join(line.replace("//!", "//", 1) if line.startswith("//!") else line
                        for line in planner.splitlines())
    paint = "\n".join(line.replace("//!", "//", 1) if line.startswith("//!") else line
                      for line in paint.splitlines())
    source = r'''#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::task::{Context, Poll, Waker};
pub struct Duration;
impl Duration { pub fn from_millis(_: u64) -> Self { Self } }
pub struct Timer;
impl Timer { pub fn after(_: Duration) -> impl Future<Output=()> { std::future::pending() } }
#[derive(Clone, Debug, PartialEq, Eq)]
enum Event { Publish, Submit(Vec<u32>), Poll(bool), CpuCopy, CpuFill }
thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
    static EMULATOR: Cell<bool> = const { Cell::new(false) };
    static QUEUE_ERROR: Cell<Option<intel::GucBcs0CopySubmitError>> = const { Cell::new(None) };
    static COMPLETION: Cell<intel::GucBcs0CopyCompletion> = const { Cell::new(intel::GucBcs0CopyCompletion::Complete) };
}
fn event(e: Event) { EVENTS.with(|events| events.borrow_mut().push(e)); }
fn events() -> Vec<Event> { EVENTS.with(|events| events.borrow().clone()) }
fn reset() {
    EVENTS.with(|events| events.borrow_mut().clear());
    EMULATOR.set(false); QUEUE_ERROR.set(None);
    COMPLETION.set(intel::GucBcs0CopyCompletion::Complete);
}
mod allcaps { pub mod text_blit {
    pub const CPU_COPY_MAX_PIXELS: usize = 270_336 / 5;
    pub const CPU_MONO_MAX_PIXELS: usize = 270_336 / 5;
} }
mod phys { pub fn phys_to_virt(value: usize) -> usize { value } }
mod virtio_gpu_logo { pub fn output_dimensions() -> Option<(u32,u32)> {
    super::EMULATOR.get().then_some((64,64))
} }
mod intel {
    pub const WARM_ALIGN: usize = 4096;
    pub fn dma_cache_flush_range(_: *const u8, _: usize) { super::event(super::Event::Publish); }
    pub(crate) use blt::{GucBcs0RgbaCopy,GucBcs0RgbaSurface,GucBcs0RgbaFill,
        GucBcs0CopySubmission,GucBcs0CopySubmitError,GucBcs0CopyCompletion,
        queue_guc_bcs0_plan_batch,poll_guc_bcs0_rgba_copies};
    mod blt {
'''
    source += constants + "\n" + declarations
    source += r'''
        pub(crate) fn queue_guc_bcs0_plan_batch(batch: &super::bcs_plan::BcsBatch)
          -> Result<GucBcs0CopySubmission,GucBcs0CopySubmitError> {
            if let Some(error) = crate::QUEUE_ERROR.get() { return Err(error); }
            let mut body = vec![0; batch.encoded_dwords()];
            let count = batch.encode_body(&mut body).expect("real batch body encoding");
            assert_eq!(count,body.len());
            let mut opcodes = Vec::new();
            let mut offset = 0;
            while offset < body.len() {
                let header = body[offset];
                if header >> 29 == 2 { opcodes.push((header >> 22) & 0x7f); }
                offset += (header & 0xff) as usize + 2;
            }
            assert_eq!(offset,body.len());
            crate::event(crate::Event::Submit(opcodes));
            Ok(GucBcs0CopySubmission { sequence: 1 })
        }
        pub(crate) fn poll_guc_bcs0_rgba_copies(_: GucBcs0CopySubmission) -> GucBcs0CopyCompletion {
            let completion = crate::COMPLETION.get();
            crate::event(crate::Event::Poll(completion == GucBcs0CopyCompletion::Complete));
            completion
        }
    }
    pub(crate) mod bcs_plan {
'''
    source += planner + "\n} }\nmod ui4 { mod text_blit {\n"
    source += "use crate::intel::{GucBcs0RgbaSurface,GucBcs0RgbaCopy,GucBcs0RgbaFill};\n"
    source += cpu_policy
    source += r'''
        pub fn valid_copies(_: GucBcs0RgbaSurface, _: &[GucBcs0RgbaCopy]) -> bool { true }
        pub unsafe fn copy(_: GucBcs0RgbaSurface, _: &[GucBcs0RgbaCopy]) -> bool {
            crate::event(crate::Event::CpuCopy); true
        }
        pub unsafe fn fill(_: GucBcs0RgbaSurface, _: &[GucBcs0RgbaFill]) -> bool {
            crate::event(crate::Event::CpuFill); true
        }
        pub unsafe fn try_glyph_copy(s: GucBcs0RgbaSurface, copies: &[GucBcs0RgbaCopy]) -> bool {
            glyph_copy_on_cpu(s,copies) && unsafe { copy(s,copies) }
        }
        pub unsafe fn try_fill(s: GucBcs0RgbaSurface, fills: &[GucBcs0RgbaFill]) -> bool {
            fill_on_cpu(s,fills) && unsafe { fill(s,fills) }
        }
    }
    pub(crate) mod text_paint {
'''
    source += paint + "\n} }\n"
    source += r'''
fn surface(gpu: u64) -> intel::GucBcs0RgbaSurface {
    intel::GucBcs0RgbaSurface { phys: gpu + 0x100000000, gpu, bytes: 16384,
        width: 64,height: 64,pitch_bytes: 256 }
}
fn copy(y: u32,width: u32,height: u32) -> intel::GucBcs0RgbaCopy {
    intel::GucBcs0RgbaCopy { source: surface(0x3000000), source_x: 0,source_y: 0,
        destination_x: 0,destination_y: y,width,height }
}
fn ready<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("completed mock hardware unexpectedly suspended"),
    }
}
#[test]
fn fragmented_fill_runs_keep_original_glyph_stream_with_clear_prefix() {
    reset();
    let plan = ui4::text_paint::RgbaPaintPlan::new(
        vec![copy(0,64,16),copy(16,64,16),copy(32,64,16)],
        |copy| (copy.destination_y != 16).then_some(0xaabbccdd),
    );
    let mut poisoned = false;
    let stats = ready(plan.paint(surface(0x5000000),Some(0),&mut poisoned)).unwrap();
    assert!(!poisoned);
    assert_eq!(stats.bcs_batches,1);
    assert_eq!(stats.copy_batches,1); assert_eq!(stats.fill_batches,1);
    assert_eq!(stats.source_mappings,1);
    assert_eq!(events(),vec![Event::Publish,Event::Submit(vec![0x44,0x53,0x53,0x53]),Event::Poll(true)]);
}
#[test]
fn existing_fragmentation_policy_still_permits_beneficial_mixed_batches() {
    reset();
    let plan = ui4::text_paint::RgbaPaintPlan::new(
        [vec![copy(0,64,16);128],vec![copy(16,64,16)]].concat(),
        |copy| (copy.destination_y == 0).then_some(0xaabbccdd),
    );
    let mut poisoned = false;
    let stats = ready(plan.paint(surface(0x5000000),Some(0),&mut poisoned)).unwrap();
    assert_eq!(stats.bcs_batches,1); assert_eq!(stats.copy_batches,1); assert_eq!(stats.fill_batches,1);
    assert_eq!(events(),vec![Event::Publish,
        Event::Submit([vec![0x44;129],vec![0x53]].concat()),Event::Poll(true)]);
    assert!(!poisoned);
}
#[test]
fn large_alternating_fill_glyph_work_preserves_pure_legacy_policy() {
    reset();
    let copies: Vec<_> = (0..256).map(|index| copy(if index % 2 == 0 {0} else {16},64,16)).collect();
    let plan = ui4::text_paint::RgbaPaintPlan::new(copies,
        |copy| (copy.destination_y == 0).then_some(0xaabbccdd));
    let mut poisoned = false;
    let stats = ready(plan.paint(surface(0x5000000),None,&mut poisoned)).unwrap();
    assert_eq!(stats.bcs_batches,2); assert_eq!(stats.fill_batches,0);
    assert_eq!(events(),vec![Event::Publish,Event::Submit(vec![0x53;160]),Event::Poll(true),
        Event::Submit(vec![0x53;96]),Event::Poll(true)]);
    assert!(!poisoned);
}

#[test]
fn gpu_cpu_gpu_steps_retire_before_cpu_and_publish_before_next_gpu() {
    reset();
    let plan = ui4::text_paint::RgbaPaintPlan::new(
        [vec![copy(0,64,16);64],vec![copy(16,1,1)],vec![copy(32,64,16);64]].concat(),
        |copy| (copy.destination_y == 16).then_some(0xaabbccdd),
    );
    let mut poisoned = false;
    let stats = ready(plan.paint(surface(0x5000000),None,&mut poisoned)).unwrap();
    assert_eq!(stats.bcs_batches,2); assert_eq!(stats.cpu_pixels,1);
    assert_eq!(events(),vec![Event::Publish,Event::Submit(vec![0x53;64]),Event::Poll(true),
        Event::CpuFill,Event::Publish,Event::Submit(vec![0x53;64]),Event::Poll(true)]);
    assert!(!poisoned);
}
#[test]
fn initial_clear_retires_before_small_cpu_glyph_update() {
    reset();
    let mut poisoned = false;
    let stats = ready(ui4::text_paint::paint_glyph_copies(
        surface(0x5000000),vec![copy(0,1,1)],Some(0),&mut poisoned)).unwrap();
    assert_eq!(stats.bcs_batches,1); assert_eq!(stats.cpu_pixels,1);
    assert_eq!(events(),vec![Event::Publish,Event::Submit(vec![0x44]),Event::Poll(true),Event::CpuCopy]);
}
#[test]
fn cancellation_after_admission_keeps_destination_poisoned() {
    reset(); COMPLETION.set(intel::GucBcs0CopyCompletion::Pending);
    let mut poisoned = false;
    let mut future = Box::pin(ui4::text_paint::paint_glyph_copies(
        surface(0x5000000),vec![copy(0,64,16)],Some(0),&mut poisoned));
    assert!(future.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending());
    drop(future);
    assert!(poisoned);
    assert_eq!(events(),vec![Event::Publish,Event::Submit(vec![0x44,0x53]),Event::Poll(false)]);
}
#[test]
fn busy_rejection_does_not_pin_but_ambiguous_admission_does() {
    for (error,expected_pin) in [(intel::GucBcs0CopySubmitError::Busy,false),
                               (intel::GucBcs0CopySubmitError::SubmitFailed,true)] {
        reset(); QUEUE_ERROR.set(Some(error));
        let mut poisoned = false;
        let result = ready(ui4::text_paint::paint_glyph_copies(
            surface(0x5000000),vec![copy(0,64,16)],None,&mut poisoned));
        assert!(result.is_err()); assert_eq!(poisoned,expected_pin);
        assert_eq!(events(),vec![Event::Publish]);
    }
}
#[test]
fn failed_or_invalid_completion_keeps_destination_pinned() {
    for completion in [intel::GucBcs0CopyCompletion::Failed,intel::GucBcs0CopyCompletion::InvalidSubmission] {
        reset(); COMPLETION.set(completion);
        let mut poisoned = false;
        let result = ready(ui4::text_paint::paint_glyph_copies(
            surface(0x5000000),vec![copy(0,64,16)],None,&mut poisoned));
        assert!(result.is_err()); assert!(poisoned);
    }
}
#[test]
fn emulator_keeps_clear_and_glyphs_cpu_local() {
    reset(); EMULATOR.set(true);
    let mut poisoned = false;
    let stats = ready(ui4::text_paint::paint_glyph_copies(
        surface(0x5000000),vec![copy(0,64,16)],Some(0),&mut poisoned)).unwrap();
    assert_eq!(stats.bcs_batches,0); assert_eq!(stats.cpu_pixels,4096+1024);
    assert_eq!(events(),vec![Event::CpuFill,Event::CpuCopy]);
    assert!(!poisoned);
}
#[test]
fn small_direct_update_keeps_existing_cpu_route_without_submission() {
    reset();
    let mut poisoned = false;
    let stats = ready(ui4::text_paint::paint_glyph_copies(
        surface(0x5000000),vec![copy(0,1,1)],None,&mut poisoned)).unwrap();
    assert_eq!(stats.bcs_batches,0); assert_eq!(stats.cpu_pixels,1);
    assert_eq!(events(),vec![Event::CpuCopy]);
}
#[test]
fn large_legacy_paint_uses_bounded_plan_batches_and_retains_mapping_dedup() {
    reset();
    let mut poisoned = false;
    let stats = ready(ui4::text_paint::paint_glyph_copies(
        surface(0x5000000),vec![copy(0,64,16);161],None,&mut poisoned)).unwrap();
    assert_eq!(stats.bcs_batches,2); assert_eq!(stats.source_mappings,2);
    let submitted: Vec<_> = events().into_iter().filter_map(|event|
        if let Event::Submit(ops) = event { Some(ops.len()) } else { None }).collect();
    assert_eq!(submitted,vec![160,1]); assert!(!poisoned);
}
#[test]
fn later_invalid_gpu_group_is_rejected_before_earlier_cpu_pixels_change() {
    reset();
    let mut invalid = copy(16,64,16); invalid.source.gpu = 0;
    let plan = ui4::text_paint::RgbaPaintPlan::new(
        vec![copy(0,1,1),invalid],
        |copy| (copy.destination_y == 0).then_some(0xaabbccdd),
    );
    let mut poisoned = false;
    assert!(ready(plan.paint(surface(0x5000000),None,&mut poisoned)).is_err());
    assert!(events().is_empty()); assert!(!poisoned);
}
#[test]
fn pinned_destination_is_never_prepared_or_submitted_again() {
    reset();
    let mut poisoned = true;
    assert!(ready(ui4::text_paint::paint_glyph_copies(
        surface(0x5000000),vec![copy(0,64,16)],Some(0),&mut poisoned)).is_err());
    assert!(events().is_empty()); assert!(poisoned);
}

'''
    return source


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-bcs-text-paint-") as temporary:
        root = Path(temporary)
        source, executable = root / "test.rs", root / "tests"
        source.write_text(harness_source())
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], check=True)
        subprocess.run([str(executable)], check=True)


if __name__ == "__main__":
    main()
