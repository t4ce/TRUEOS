#!/usr/bin/env python3
"""Exercise production coverage batching without a GPU or copied batching logic."""
from pathlib import Path
import re
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[2]

def block(source, marker):
    start=source.index(marker);end=source.index('{',start)+1;depth=1
    while depth:
        depth+=(source[end]=='{')-(source[end]=='}');end+=1
    return source[start:end]

def main():
    coverage=(ROOT/'src/intel/render/coverage_mask.rs').read_text()
    primary=(ROOT/'src/intel/render/primary.rs').read_text()
    constants=(ROOT/'src/intel/render/constants.rs').read_text()
    parts=[re.search(r'pub\(crate\) const RESIDENT_SCENE_MAX_DRAWS:[^;]+;',constants).group(),
           re.search(r'const RESIDENT_COVERAGE_MASK_MAX_DRAWS:[^;]+;',coverage).group(),
           '#[derive(Copy,Clone,Debug,PartialEq)]\n'+block(primary,'pub(crate) struct ResidentSceneGpuTimestamp'),
           '#[derive(Copy,Clone)]\n'+block(primary,'struct ResidentSceneGeometryResult'),
           block(coverage,'fn sum_coverage_gpu_intervals('),
           block(coverage,'fn submit_coverage_mask_batches<T>(')]
    tests=r'''
fn result(completed:bool)->ResidentSceneGeometryResult {
    let time=Some(ResidentSceneGpuTimestamp {ticks:10,hz:100_000_000,ns:100});
    ResidentSceneGeometryResult {completed,prepare_us:2,gpu_poll_us:3,gpu_poll_iters:4,
        gpu_timestamp:time,gpu_target_ready_timestamp:time,gpu_clear_setup_timestamp:time,
        gpu_writeback_timestamp:time,pipeline_stats:None,vue_capture:false}
}
#[test] fn boundaries_through_3000_preserve_every_prior_chunk_and_clear_once() {
    for count in [1,340,341,343,680,681,3000] {
        let draws:Vec<_>=(0..count).collect();let mut mask=vec![false;count];let mut calls=0;
        let total=submit_coverage_mask_batches(&draws,|batch,clear| {
            assert!(!batch.is_empty() && batch.len()<=RESIDENT_SCENE_MAX_DRAWS);
            assert_eq!(clear,if calls==0 {Some([0;4])} else {None});
            if clear.is_some() {mask.fill(false);}
            for &slot in batch {assert!(!mask[slot]);mask[slot]=true;}
            calls+=1;Ok(result(true))
        }).unwrap();
        assert!(mask.iter().all(|&written|written));assert!(total.completed);
        assert_eq!(calls,count.div_ceil(RESIDENT_SCENE_MAX_DRAWS));
        assert_eq!(total.prepare_us,2*calls as u64);
        assert_eq!(total.gpu_poll_us,3*calls as u64);
        assert_eq!(total.gpu_poll_iters,4*calls as u64);
        assert_eq!(total.gpu_target_ready_timestamp.unwrap().ns,100*calls as u64);
    }
}
#[test] fn invalid_count_is_rejected_before_any_submission() {
    for count in [0,3001] {
        let draws=vec![0;count];let mut calls=0;
        assert!(submit_coverage_mask_batches(&draws,|_,_| {calls+=1;Ok(result(true))}).is_err());
        assert_eq!(calls,0);
    }
}
#[test] fn unretired_submission_stops_later_chunks() {
    for stop_at in [1,2] {
        let draws=vec![0;1000];let mut calls=0;
        let total=submit_coverage_mask_batches(&draws,|_,_| {
            calls+=1;Ok(result(calls!=stop_at))
        }).unwrap();
        assert_eq!(calls,stop_at);assert!(!total.completed);
    }
}
#[test] fn rejected_later_chunk_stops_without_claiming_completion() {
    let draws=vec![0;1000];let mut calls=0;
    let outcome=submit_coverage_mask_batches(&draws,|_,_| {
        calls+=1;if calls==2 {Err("render-storage-busy")} else {Ok(result(true))}
    });
    assert_eq!(outcome.err(),Some("render-storage-busy"));assert_eq!(calls,2);
}
#[test] fn unavailable_or_mismatched_timestamps_do_not_report_partial_gpu_time() {
    let t=Some(ResidentSceneGpuTimestamp{ticks:10,hz:100,ns:100_000_000});
    assert!(sum_coverage_gpu_intervals(t,None).is_none());
    assert!(sum_coverage_gpu_intervals(None,t).is_none());
    assert!(sum_coverage_gpu_intervals(t,Some(ResidentSceneGpuTimestamp{hz:200,..t.unwrap()})).is_none());
}
'''
    with tempfile.TemporaryDirectory(prefix='microfont-coverage-batches-') as tmp:
        tmp=Path(tmp);source=tmp/'test.rs';exe=tmp/'tests'
        source.write_text('\n'.join(parts)+tests)
        subprocess.run(['rustc','--edition=2024','--test','-A','dead_code',str(source),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
if __name__=='__main__':main()
