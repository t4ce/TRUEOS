#!/usr/bin/env python3
"""Extract the private production tracker and broker hooks into a host harness."""
from pathlib import Path
import argparse
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source=(ROOT/'src/r/fs/request_broker.rs').read_text()

def item(signature):
 start=source.index(signature); opening=source.index('{',start); depth=1;pos=opening+1
 while depth:
  if source[pos]=='{':depth+=1
  if source[pos]=='}':depth-=1
  pos+=1
 return source[start:pos]

parts=[item(x) for x in [
 'struct OwnedRequest {','pub(crate) fn arm_startup_diagnostics(', 'fn diagnostic_ticket(',
 'fn operation_code(', 'pub(crate) fn progress_lines(', 'fn retire_request(',
 'fn complete_request(', 'fn finish<T>(']]
head=r'''
#![allow(dead_code)]
extern crate alloc;
use core::sync::atomic::AtomicU64;
static CLOCK:AtomicU64=AtomicU64::new(0);
static CLOCK_CALLS:AtomicU64=AtomicU64::new(0);
static RUN:AtomicU64=AtomicU64::new(10);
mod allcaps { pub mod hv { pub const CODEX_PROGRESS_PROBES_ENABLED:bool=true; pub const VM_ID_LIMIT:usize=64; } }
mod hv { pub(crate) fn vm_run_generation(owner:u8)->Option<u64>{(owner<64).then(||crate::RUN.load(core::sync::atomic::Ordering::Relaxed))} }
mod r { pub mod input_timing { pub(crate) fn now_ms()->u64{crate::CLOCK_CALLS.fetch_add(1,core::sync::atomic::Ordering::Relaxed);crate::CLOCK.load(core::sync::atomic::Ordering::Relaxed)} } }
#[macro_export] macro_rules! log_error { (target: $target:literal; $($args:tt)*)=>{{let _=format_args!($($args)*);}} }
#[macro_export] macro_rules! log_trace { (target: $target:literal; $($args:tt)*)=>{{let _=format_args!($($args)*);}} }
mod broker {
use alloc::{string::String,sync::Arc,vec::Vec};
use core::sync::atomic::{AtomicU64,Ordering};
use std::collections::BTreeMap;
#[path="TRACKER"] mod native_request_progress;
use native_request_progress as progress;
static PROGRESS:progress::Tracker=progress::Tracker::new();
static REPORTED_FULL:AtomicU64=AtomicU64::new(0);
static REPORTED_MISSED:AtomicU64=AtomicU64::new(0);
static REPORTED_EXPIRED:AtomicU64=AtomicU64::new(0);
static REPORTED_RELEASED:AtomicU64=AtomicU64::new(0);
type FsResult<T>=Result<T,()>;
type Completion<T>=Arc<CompletionCell<FsResult<T>>>;
struct CompletionCell<T>{value:std::sync::Mutex<Option<T>>, observe_publish:bool}
impl<T> CompletionCell<T> {
 fn new(observe_publish:bool)->Self{Self{value:std::sync::Mutex::new(None),observe_publish}}
 fn complete(&self,value:T)->Result<(),T>{
  if self.observe_publish {let row=PROGRESS.sample(crate::CLOCK.load(Ordering::Relaxed),&mut 0,4).into_iter().flatten().next().unwrap();assert_eq!(row.phase,progress::PUBLISH);}
  let mut slot=self.value.lock().unwrap();if slot.is_some(){Err(value)}else{*slot=Some(value);Ok(())}
 }
}
struct Request {completion:Completion<usize>}
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T>{const fn new(t:T)->Self{Self(std::sync::Mutex::new(t))}fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}
struct OwnerState{requests:BTreeMap<u64,(u8,Option<u32>)>}
static BROKER_OWNERS:Mutex<OwnerState>=Mutex::new(OwnerState{requests:BTreeMap::new()});
struct Wait;
impl Wait {fn notify_all(&self){}}
static BROKER_WAIT:Wait=Wait;
'''.replace('TRACKER',str(ROOT/'src/r/fs/native_request_progress.rs'))
tests=r'''
#[test] fn production_hooks_own_pending_work_and_publish_before_retirement() {
 crate::CLOCK.store(100,Ordering::Relaxed);arm_startup_diagnostics(0,10,6);
 let completion=Arc::new(CompletionCell::new(true));let consumer=completion.clone();
 let mut owned=OwnedRequest{request:Request{completion:completion.clone()},owner:Some(0),progress:diagnostic_ticket(Some(0),19,"read-file")};
 BROKER_OWNERS.lock().requests.insert(19,(0,None));drop(consumer);drop(completion);
 let row=PROGRESS.sample(101,&mut 0,4).into_iter().flatten().next().unwrap();assert_eq!((row.id,row.phase),(19,progress::QUEUED));
 owned.progress.advance(progress::SERVICE,0,||102);owned.progress.advance(progress::FILESYSTEM,0,||103);
 crate::CLOCK.store(104,Ordering::Relaxed);
 finish(19,"read-file",owned.request.completion.clone(),Ok(42),&mut owned.progress);
 let row=PROGRESS.sample(105,&mut 0,4).into_iter().flatten().next().unwrap();assert_eq!((row.phase,row.status),(progress::PUBLISHED,1));
 assert_eq!(*owned.request.completion.value.lock().unwrap(),Some(Ok(42)));assert!(BROKER_OWNERS.lock().requests.contains_key(&19));
 retire_request(19,&mut owned.progress);assert!(!BROKER_OWNERS.lock().requests.contains_key(&19));
 let row=PROGRESS.sample(106,&mut 0,4).into_iter().flatten().next().unwrap();assert_eq!(row.phase,progress::FINISHED);
 drop(owned);assert!(PROGRESS.sample(107,&mut 0,4).iter().all(Option::is_none));
}
#[test] fn production_snapshot_bounds_privacy_expiry_summary_and_quiet_clock() {
 crate::CLOCK.store(1000,Ordering::Relaxed);arm_startup_diagnostics(1,10,7);
 let tickets:Vec<_>=(100..116).map(|id|diagnostic_ticket(Some(1),id,"typed-stat")).collect();
 let before=crate::CLOCK_CALLS.load(Ordering::Relaxed);assert!(progress_lines(&mut 0,0).is_empty());assert_eq!(before,crate::CLOCK_CALLS.load(Ordering::Relaxed));
 let lines=progress_lines(&mut 0,usize::MAX);assert_eq!(lines.len(),4);
 for line in lines {assert!(line.starts_with("codex-fs-native "));for word in line.split_whitespace().skip(1){let(key,value)=word.split_once('=').unwrap();assert!(key.bytes().all(|b|b.is_ascii_lowercase()||b==b'_'));assert!(value.bytes().all(|b|b.is_ascii_digit()));}}
 crate::CLOCK.store(500000,Ordering::Relaxed);let lines=progress_lines(&mut 0,4);assert_eq!(lines.len(),1);assert!(lines[0].starts_with("codex-fs-native-summary "));assert!(lines[0].contains("expired=16"));
 drop(tickets);let before=crate::CLOCK_CALLS.load(Ordering::Relaxed);assert!(progress_lines(&mut 0,4).is_empty());assert_eq!(before,crate::CLOCK_CALLS.load(Ordering::Relaxed));
}
#[test] fn production_generation_filter_and_all_static_operations() {
 crate::CLOCK.store(200000,Ordering::Relaxed);arm_startup_diagnostics(2,10,8);
 crate::RUN.store(11,Ordering::Relaxed);let before=crate::CLOCK_CALLS.load(Ordering::Relaxed);
 drop(diagnostic_ticket(Some(2),1000,"stat"));drop(diagnostic_ticket(None,1001,"stat"));drop(diagnostic_ticket(Some(63),1002,"stat"));assert_eq!(before,crate::CLOCK_CALLS.load(Ordering::Relaxed));crate::RUN.store(10,Ordering::Relaxed);
 let names=["prepare-volatile-files","read-file","read-file-len","read-file-range","stat","typed-stat","write-file-begin","write-file-begin-typed","create-dir-all","write-file-chunk","write-file-finish","write-file-abort","html-tree","json-all","list-dir","typed-list-dir","remove","exists"];
 for (index,name) in names.into_iter().enumerate(){assert_eq!(operation_code(name),1+index as u32);}assert_eq!(operation_code("unexpected/secret"),0);
}
}
'''
# Verify actual integration locations and all unmodified filesystem call arms.
observer=(ROOT/'src/r/services/codex_progress_service.rs').read_text();hv=(ROOT/'src/hv/mod.rs').read_text()
assert observer.index('request_broker::progress_lines')<observer.index('if active == 0 && !had_active')
assert hv.index('request_broker::arm_startup_diagnostics')<hv.index('let Some(guest_state)',hv.index('pub fn stage_blueprint_launch'))
process=item('async fn process_request(')
assert process.count('finish(')==20 and process.count('&mut progress);')==23
assert process.index('progress::SERVICE')<process.index('BROKER_OWNERS.lock()')
assert process.index('progress::FILESYSTEM')<process.index('Request::PrepareVolatileFiles { id, root, completion } =>')
assert 'progress: diagnostic_ticket(Some(owner), id, "write-file-abort")' in source


def check_baseline(path):
    """Optional historical conservation check; ordinary tests need no snapshot."""
    base = path.read_text()
    begin = base.index('async fn process_request(')
    end = base.index('\nfn complete_request(', begin)
    old = base[begin:end].strip()
    new = process.replace('let OwnedRequest { request, owner, mut progress } = owned;',
                          'let OwnedRequest { request, owner } = owned;')
    new = re.sub(r'^    progress.advance\(progress::(?:SERVICE|FILESYSTEM), 0, crate::r::input_timing::now_ms\);\n',
                 '', new, flags=re.M)
    new = re.sub(r'^            progress.advance\(progress::RANGE_SCRATCH, 0, crate::r::input_timing::now_ms\);\n', '', new, flags=re.M)
    new = new.replace('file_read_range_observed_async(', 'file_read_range_async(')
    new = re.sub(r'^                \|phase\| \{\n.*?^                \},\n', '', new, flags=re.M | re.S)
    new = new.replace(', &mut progress);', ');').replace('retire_request(request_id);',
                                                       'complete_request(request_id);')
    assert new.strip() == old, 'request behavior changed beyond observational hooks'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path,
                        help='optional pre-probe request_broker.rs for conservation comparison')
    args = parser.parse_args()
    if args.baseline is not None:
        check_baseline(args.baseline)
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / 'rust-toolchain.toml').read_text(), re.MULTILINE)
    assert channel, 'missing pinned Rust toolchain'
    # Production globals are exercised by one serial host test process.
    with tempfile.TemporaryDirectory(prefix='trueos-native-fs-progress-') as directory:
        work = Path(directory)
        path = work / 'hooks.rs'
        path.write_text(head + '\n'.join(parts) + tests)
        subprocess.run([
            'rustc', '+' + channel.group(1), '--test', '-Dwarnings', '--edition=2024',
            '--target', 'x86_64-unknown-linux-gnu', str(path), '-o', str(work / 'hooks'),
        ], cwd=work, check=True)
        subprocess.run([str(work / 'hooks'), '--test-threads=1'], cwd=work, check=True)
    print('Native FS production hooks and integration checks passed.')


if __name__ == '__main__':
    main()
