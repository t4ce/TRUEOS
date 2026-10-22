#!/usr/bin/env python3
"""Exercise the production slot-0 BCS queue's retirement and ownership rules."""
from pathlib import Path
import subprocess
import tempfile

import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    source = r'''#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[macro_export] macro_rules! log_error {
    (target:$target:literal; $($tokens:tt)*)=>{{let _=format_args!($($tokens)*);}}
}
mod intel {
'''
    source += '\n'.join(extract.item('src/intel/copy/blt.rs', name) for name in (
        'GucBcs0RgbaSurface', 'GucBcs0RgbaCopy', 'GucBcs0CopySubmission',
        'GucBcs0CopySubmitError', 'GucBcs0CopyCompletion'))
    source += r'''
pub fn token(sequence:u32)->GucBcs0CopySubmission {GucBcs0CopySubmission{sequence}}
mod display {
'''
    source += extract.item('src/intel/display.rs', 'Ui4AsyncCompositionError')
    source += extract.item('src/intel/display.rs', 'Ui4AsyncCompositionPoll')
    source += '\nmod ui4_bcs_queue {\n'
    queue_source = (ROOT / 'src/intel/display/ui4_bcs_queue.rs').read_text()
    source += queue_source.replace('//!', '//')
    source += r'''
#[cfg(test)] mod tests {
use super::*;
use crate::r::services::vcpy_service::{FAKE, Fake};
static TEST_LOCK: Mutex<()> = Mutex::new(());
fn surface()->GucBcs0RgbaSurface {
    GucBcs0RgbaSurface{phys:0x2000,gpu:0x2000,bytes:4096,width:512,height:2,pitch_bytes:2048}
}
fn copies(count:usize)->(Vec<GucBcs0RgbaCopy>,Vec<bool>) {
    ((0..count).map(|index| GucBcs0RgbaCopy {
        source: GucBcs0RgbaSurface {phys:0x10000+index as u64*0x1000,
            gpu:0x10000+index as u64*0x1000,..surface()},
        source_x:0,source_y:0,destination_x:0,destination_y:0,width:1,height:1,
    }).collect(), (0..count).map(|index| index%3==1).collect())
}
fn reset() {
    *PENDING.lock()=None;
    *FAKE.lock()=Fake::new();
    SEQUENCE.store(0,Ordering::Relaxed);
}
fn pending(sequence:u64) {assert!(matches!(poll(sequence),Ui4AsyncCompositionPoll::Pending));}

#[test] fn large_plan_keeps_order_cache_flags_and_waits_for_every_chunk() {
    let _guard=TEST_LOCK.lock(); reset();
    let (plan,flags)=copies(321);
    FAKE.lock().completions.extend([
        GucBcs0CopyCompletion::Pending,GucBcs0CopyCompletion::Complete,
        GucBcs0CopyCompletion::Complete,GucBcs0CopyCompletion::Complete,
    ]);
    let sequence=queue(surface(),plan.clone(),flags.clone()).unwrap();
    assert_eq!(PENDING.lock().as_ref().unwrap().copies.len(),321);
    assert!(matches!(queue(surface(),copies(1).0,copies(1).1),Err(Ui4AsyncCompositionError::Busy)));
    pending(sequence);
    assert_eq!(FAKE.lock().attempts.len(),1);
    pending(sequence);
    assert_eq!(FAKE.lock().attempts.len(),2);
    pending(sequence);
    assert_eq!(FAKE.lock().attempts.len(),3);
    assert!(matches!(poll(sequence),Ui4AsyncCompositionPoll::Ready));
    assert!(PENDING.lock().is_none());
    let fake=FAKE.lock();
    assert_eq!(fake.attempts.iter().map(|(_,copies,_)| copies.len()).collect::<Vec<_>>(),[160,160,1]);
    assert_eq!(fake.attempts.iter().flat_map(|(_,copies,_)| copies.clone()).collect::<Vec<_>>(),plan);
    assert_eq!(fake.attempts.iter().flat_map(|(_,_,flags)| flags.clone()).collect::<Vec<_>>(),flags);
    drop(fake);
    assert!(matches!(poll(sequence),Ui4AsyncCompositionPoll::Failed));
}

#[test] fn busy_between_chunks_keeps_the_unsubmitted_chunk_and_plan() {
    let _guard=TEST_LOCK.lock(); reset();
    FAKE.lock().submissions.extend([
        Ok(crate::intel::token(1)),Err(GucBcs0CopySubmitError::Busy),Ok(crate::intel::token(2)),
    ]);
    FAKE.lock().completions.extend([GucBcs0CopyCompletion::Complete,GucBcs0CopyCompletion::Complete]);
    let (plan,flags)=copies(161);
    let sequence=queue(surface(),plan,flags).unwrap();
    pending(sequence);
    {
        let state=PENDING.lock(); let plan=state.as_ref().unwrap();
        assert_eq!(plan.next,160); assert!(plan.active.is_none());
        assert_eq!(plan.copies.len(),161);
    }
    pending(sequence);
    let fake=FAKE.lock();
    assert_eq!(fake.attempts[1],fake.attempts[2]);
    assert_eq!(fake.polls.len(),1);
    drop(fake);
    assert!(matches!(poll(sequence),Ui4AsyncCompositionPoll::Ready));
}

#[test] fn ambiguous_first_submit_keeps_a_token_and_pins_source_ownership() {
    let _guard=TEST_LOCK.lock(); reset();
    FAKE.lock().submissions.push_back(Err(GucBcs0CopySubmitError::SubmitFailed));
    let (plan,flags)=copies(161);
    let sequence=queue(surface(),plan,flags).unwrap();
    for _ in 0..3 {pending(sequence);}
    let state=PENDING.lock(); let plan=state.as_ref().unwrap();
    assert!(plan.ownership_uncertain); assert_eq!(plan.copies.len(),161);
    drop(state);
    assert!(matches!(queue(surface(),copies(1).0,copies(1).1),Err(Ui4AsyncCompositionError::Busy)));
    assert_eq!(FAKE.lock().attempts.len(),1);
    assert!(FAKE.lock().polls.is_empty());
}

#[test] fn uncertain_completion_keeps_the_active_batch_and_stops_polling_it() {
    let _guard=TEST_LOCK.lock();
    for completion in [GucBcs0CopyCompletion::Failed,GucBcs0CopyCompletion::InvalidSubmission] {
        reset(); FAKE.lock().completions.push_back(completion);
        let (plan,flags)=copies(161);
        let sequence=queue(surface(),plan,flags).unwrap();
        for _ in 0..3 {pending(sequence);}
        let state=PENDING.lock(); let plan=state.as_ref().unwrap();
        assert!(plan.ownership_uncertain); assert!(plan.active.is_some());
        assert_eq!(plan.next,160); assert_eq!(plan.copies.len(),161);
        drop(state);
        assert_eq!(FAKE.lock().attempts.len(),1);
        assert_eq!(FAKE.lock().polls.len(),1);
    }
}

#[test] fn ambiguous_later_submit_pins_the_entire_plan_without_retry() {
    let _guard=TEST_LOCK.lock(); reset();
    FAKE.lock().submissions.extend([
        Ok(crate::intel::token(1)),Err(GucBcs0CopySubmitError::SubmitFailed),
    ]);
    FAKE.lock().completions.push_back(GucBcs0CopyCompletion::Complete);
    let (plan,flags)=copies(161);
    let sequence=queue(surface(),plan,flags).unwrap();
    for _ in 0..3 {pending(sequence);}
    let state=PENDING.lock(); let plan=state.as_ref().unwrap();
    assert!(plan.ownership_uncertain); assert_eq!(plan.copies.len(),161);
    drop(state);
    assert_eq!(FAKE.lock().attempts.len(),2);
    assert_eq!(FAKE.lock().polls.len(),1);
}

#[test] fn safe_rejection_leaves_no_plan_when_no_gpu_batch_is_active() {
    let _guard=TEST_LOCK.lock();
    for rejection in [GucBcs0CopySubmitError::Unavailable,GucBcs0CopySubmitError::InvalidRequest] {
        reset(); FAKE.lock().submissions.push_back(Err(rejection));
        let (plan,flags)=copies(1);
        assert!(queue(surface(),plan,flags).is_err());
        assert!(PENDING.lock().is_none());
        reset();
        FAKE.lock().submissions.extend([Ok(crate::intel::token(1)),Err(rejection)]);
        FAKE.lock().completions.push_back(GucBcs0CopyCompletion::Complete);
        let (plan,flags)=copies(161);
        let sequence=queue(surface(),plan,flags).unwrap();
        assert!(matches!(poll(sequence),Ui4AsyncCompositionPoll::Failed));
        assert!(PENDING.lock().is_none());
        let (plan,flags)=copies(1);
        assert!(queue(surface(),plan,flags).is_ok());
    }
}
}
}}}
mod r {pub mod services {pub mod vcpy_service {
use crate::intel::*;
use std::collections::VecDeque;
pub struct Fake {
    pub submissions:VecDeque<Result<GucBcs0CopySubmission,GucBcs0CopySubmitError>>,
    pub completions:VecDeque<GucBcs0CopyCompletion>,
    pub attempts:Vec<(GucBcs0RgbaSurface,Vec<GucBcs0RgbaCopy>,Vec<bool>)>,
    pub polls:Vec<GucBcs0CopySubmission>,
}
impl Fake {
    pub const fn new()->Self {Self{submissions:VecDeque::new(),completions:VecDeque::new(),
        attempts:Vec::new(),polls:Vec::new()}}
}
pub static FAKE: crate::Mutex<Fake>=crate::Mutex::new(Fake::new());
pub(crate) fn queue_rgba_copies_with_scanout_cache(destination:GucBcs0RgbaSurface,
    copies:&[GucBcs0RgbaCopy],flags:&[bool])->Result<GucBcs0CopySubmission,GucBcs0CopySubmitError> {
    let mut fake=FAKE.lock();
    fake.attempts.push((destination,copies.to_vec(),flags.to_vec()));
    let token=crate::intel::token(fake.attempts.len() as u32);
    fake.submissions.pop_front().unwrap_or(Ok(token))
}
pub(crate) fn poll_rgba_copies(submission:GucBcs0CopySubmission)->GucBcs0CopyCompletion {
    let mut fake=FAKE.lock(); fake.polls.push(submission);
    fake.completions.pop_front().unwrap_or(GucBcs0CopyCompletion::Pending)
}
}}}
'''
    with tempfile.TemporaryDirectory(prefix='ui4-bcs-queue-') as directory:
        path = Path(directory) / 'tests.rs'
        path.write_text(source)
        binary = Path(directory) / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
